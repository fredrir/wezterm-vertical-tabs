//! A PTY surface owned by the existing GUI window, outside its tab projection.
use super::ui_host::{Bounds, Message, QuickTerminalConfig, TerminalBorder, TerminalProgram};
use super::{TermWindow, TermWindowNotif};
use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator};
use anyhow::Context;
use codec::SpawnBesidePane;
use config::keyassignment::{PaneDirection, SpawnCommand, SpawnTabDomain};
use config::{
    DimensionContext, HorizontalWindowContentAlignment, VerticalWindowContentAlignment,
    WindowContentAlignment, WindowPadding,
};
use mux::Mux;
use mux::domain::{Domain, DomainState};
use mux::pane::{CachePolicy, CloseReason, Pane, PaneId};
use mux::tab::{PositionedPane, TabId};
use mux::window::WindowId as MuxWindowId;
use portable_pty::CommandBuilder;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wezterm_client::domain::ClientDomain;
use wezterm_client::pane::ClientPane;
use wezterm_term::{KeyCode, KeyModifiers, TerminalSize};
use window::{WindowOps, color::LinearRgba};

mod blur;
mod frame;
mod session;
use session::{Program, Slot, State};

const SHELL_SIZE: (f32, f32) = (0.75, 0.75);
/// Fewest columns and rows a size step leaves.
const MIN_CELLS: (f32, f32) = (20., 5.);

#[derive(Default)]
pub(super) struct TerminalOverlay {
    session: Rc<RefCell<State>>,
    blur: RefCell<Option<blur::BackdropBlur>>,
}

/// Eases the drawn frame and content from where a size step found them.
#[derive(Clone, Copy, Debug)]
struct Motion {
    size: (f32, f32),
    origin: (f32, f32),
    start: Instant,
    duration: Duration,
    eased: f32,
}

impl Motion {
    /// The sidebar's ease-out cubic; false once settled.
    fn advance(&mut self, now: Instant) -> bool {
        let t =
            now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32();
        self.eased = 1. - (1. - t.min(1.)).powi(3);
        t < 1.
    }

    fn lerp(&self, from: (f32, f32), to: (f32, f32)) -> (f32, f32) {
        (
            from.0 + (to.0 - from.0) * self.eased,
            from.1 + (to.1 - from.1) * self.eased,
        )
    }
}

/// The frame and content as painted this frame, against the layout they ease toward.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    layout: Layout,
    size: (f32, f32),
    painted: Layout,
    origin: (f32, f32),
    moving: bool,
}

fn axis(size: &mut (Option<f32>, Option<f32>), horizontal: bool) -> &mut Option<f32> {
    if horizontal { &mut size.0 } else { &mut size.1 }
}

/// `step` by `cells`, then by pixels until `grid` follows, since a percent padding grows with the frame.
fn step_cells(
    current: f32,
    cells: f32,
    cell: f32,
    (minimum, maximum): (f32, f32),
    grid: impl Fn(f32) -> f32,
) -> f32 {
    let mut target = step(current, cells * cell, minimum, maximum);
    let want = grid(current) + cells;
    while cells > 0. && grid(target) < want && target < maximum {
        target = (target + 1.).min(maximum);
    }
    while cells < 0. && grid(target) > want && target > minimum {
        target = (target - 1.).max(minimum);
    }
    target
}

/// `current` moved by `delta` within `minimum..=maximum`, never back past where it started.
fn step(current: f32, delta: f32, minimum: f32, maximum: f32) -> f32 {
    if delta > 0. {
        (current + delta).min(maximum).max(current)
    } else {
        (current + delta).max(minimum).min(current)
    }
}

enum Target {
    Domain(Arc<dyn Domain>),
    /// A client without `local_pane_layout` mirrors its server's tabs, so the server spawns
    /// beside its pane in a hidden workspace.
    Beside(Arc<dyn Domain>, PaneId),
}

struct Launch {
    target: Target,
    command: Option<CommandBuilder>,
    cwd: Option<String>,
}

fn mirrored_tab(pane: &Arc<dyn Pane>) -> Option<TabId> {
    Mux::get()
        .resolve_pane_id(pane.pane_id())
        .map(|(_, _, tab)| tab)
}

/// Keyed by the server window, which outlives this GUI, so a restart finds what it left.
fn quick_workspace(domain: &Arc<dyn Domain>, window: MuxWindowId, slot: Slot) -> String {
    let window = domain
        .downcast_ref::<ClientDomain>()
        .and_then(|client| client.local_to_remote_window_id(window))
        .map_or_else(|| format!("local{window}"), |id| id.to_string());
    let kind = match slot {
        Slot::Shell(_) => "shell",
        Slot::Program(_) => "program",
    };
    format!("{}{window}:{kind}", mux::QUICK_WORKSPACE_PREFIX)
}

fn quick_panes(workspace: &str) -> Vec<Arc<dyn Pane>> {
    let mux = Mux::get();
    mux.iter_windows_in_workspace(workspace)
        .into_iter()
        .filter_map(|id| mux.get_window(id))
        .flat_map(|window| {
            window
                .iter_tabs()
                .filter_map(|tab| tab.get_active_pane())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn remove(pane: &Arc<dyn Pane>) {
    match Mux::try_get() {
        Some(mux) => mux.remove_pane(pane.pane_id()),
        None => pane.kill(),
    }
}

fn alive(pane: &Arc<dyn Pane>) -> bool {
    !pane.is_dead() && Mux::get().get_pane(pane.pane_id()).is_some()
}

fn term_config(
    config: &config::ConfigHandle,
    colors: Option<&config::Palette>,
) -> Arc<config::TermConfig> {
    let term = config::TermConfig::with_config(config.clone());
    if let Some(colors) = colors {
        term.set_client_palette(config.resolved_palette.overlay_with(colors).into());
    }
    Arc::new(term)
}

fn directory(url: &url::Url) -> Option<String> {
    let mut local = url::Url::parse("file:///").ok()?;
    local.set_path(url.path());
    local
        .to_file_path()
        .ok()?
        .into_os_string()
        .into_string()
        .ok()
}

fn border_width(dpi: u32) -> f32 {
    (dpi as f32 / 96.).max(1.)
}

/// Between the frame and the grid, read as the main window reads its own settings.
#[derive(Clone, Copy, Debug)]
struct Edges {
    border: f32,
    padding: WindowPadding,
    alignment: WindowContentAlignment,
}

#[derive(Clone, Copy, Debug)]
struct Layout {
    frame: Bounds,
    /// Inside the border and padding, which always hold the content.
    inner: Bounds,
    content: Bounds,
    size: TerminalSize,
}

impl Layout {
    fn new(
        bounds: Bounds,
        (width, height): (f32, f32),
        edges: &Edges,
        (cell_width, cell_height): (f32, f32),
        dpi: u32,
    ) -> Self {
        // Whole device pixels keep glyphs and frame edges crisp.
        let frame = Bounds {
            x: (bounds.x + bounds.width * (1. - width) / 2.).round(),
            y: (bounds.y + bounds.height * (1. - height) / 2.).round(),
            width: (bounds.width * width).round(),
            height: (bounds.height * height).round(),
        };
        let border = edges.border.min(frame.width / 4.).min(frame.height / 4.);
        let (edge_width, edge_height) = (frame.width - 2. * border, frame.height - 2. * border);
        let side = |dimension: config::Dimension, pixel_max: f32, pixel_cell: f32| {
            let context = DimensionContext {
                dpi: dpi as f32,
                pixel_max,
                pixel_cell,
            };
            dimension
                .evaluate_as_pixels(context)
                .min(pixel_max / 4.)
                .max(0.)
                .floor()
        };
        let padding = &edges.padding;
        let left = border + side(padding.left, edge_width, cell_width);
        let right = border + side(padding.right, edge_width, cell_width);
        let top = border + side(padding.top, edge_height, cell_height);
        let bottom = border + side(padding.bottom, edge_height, cell_height);
        let inner = Bounds {
            x: frame.x + left,
            y: frame.y + top,
            width: (frame.width - left - right).max(0.),
            height: (frame.height - top - bottom).max(0.),
        };
        let cols = (inner.width / cell_width).floor().max(1.) as usize;
        let rows = (inner.height / cell_height).floor().max(1.) as usize;
        let width = cols as f32 * cell_width;
        let height = rows as f32 * cell_height;
        let x = match edges.alignment.horizontal {
            HorizontalWindowContentAlignment::Left => inner.x,
            HorizontalWindowContentAlignment::Center => inner.x + (inner.width - width) / 2.,
            HorizontalWindowContentAlignment::Right => inner.x + inner.width - width,
        };
        let y = match edges.alignment.vertical {
            VerticalWindowContentAlignment::Top => inner.y,
            VerticalWindowContentAlignment::Center => inner.y + (inner.height - height) / 2.,
            VerticalWindowContentAlignment::Bottom => inner.y + inner.height - height,
        };
        Self {
            frame,
            inner,
            content: Bounds {
                x: x.floor(),
                y: y.floor(),
                width,
                height,
            },
            size: TerminalSize {
                cols,
                rows,
                pixel_width: width as usize,
                pixel_height: height as usize,
                dpi,
            },
        }
    }
}

impl TermWindow {
    pub(super) fn terminal_overlays_can_close(&self) -> bool {
        self.ui_host
            .iter()
            .chain(self.vtabs_suspended.values())
            .all(|ui| {
                let state = ui.terminal.session.borrow();
                let idle =
                    state.pane.iter().chain(state.program_panes()).all(|pane| {
                        pane.can_close_without_prompting(mux::pane::CloseReason::Window)
                    });
                idle
            })
    }

    pub(super) fn terminal_overlay_config(&self) -> Option<QuickTerminalConfig> {
        let ui = self.ui_host.as_ref()?;
        let config = ui.provider.quick_terminal_config();
        let state = ui.terminal.session.borrow();
        Some(match state.program.as_ref().filter(|_| state.visible) {
            Some(program) => config.with(&program.spec),
            None => config,
        })
    }

    fn terminal_overlay_edges(&self) -> Edges {
        let config = self.terminal_overlay_config();
        let bordered = config.map_or(true, |config| config.border != TerminalBorder::None);
        Edges {
            border: if bordered {
                border_width(self.dimensions.dpi as u32)
            } else {
                0.
            },
            padding: config
                .map(|config| config.padding)
                .unwrap_or_default()
                .resolve(&self.config.window_padding),
            alignment: self.config.window_content_alignment,
        }
    }

    fn terminal_overlay_layout_of(&self, size: (f32, f32)) -> Layout {
        Layout::new(
            self.vtabs_geometry().ui_bounds(true),
            size,
            &self.terminal_overlay_edges(),
            (
                self.render_metrics.cell_size.width as f32,
                self.render_metrics.cell_size.height as f32,
            ),
            self.dimensions.dpi as u32,
        )
    }

    fn terminal_overlay_target(&self) -> ((f32, f32), Option<Motion>) {
        self.ui_host.as_ref().map_or((SHELL_SIZE, None), |ui| {
            let state = ui.terminal.session.borrow();
            (
                state.size(ui.provider.quick_terminal_config().size),
                state.motion,
            )
        })
    }

    fn terminal_overlay_layout(&self) -> Layout {
        self.terminal_overlay_layout_of(self.terminal_overlay_target().0)
    }

    fn terminal_overlay_drawn(&self) -> Drawn {
        let (size, motion) = self.terminal_overlay_target();
        let layout = self.terminal_overlay_layout_of(size);
        let origin = (layout.content.x, layout.content.y);
        let Some(motion) = motion else {
            return Drawn {
                layout,
                size,
                painted: layout,
                origin,
                moving: false,
            };
        };
        let drawn = motion.lerp(motion.size, size);
        let (x, y) = motion.lerp(motion.origin, origin);
        Drawn {
            layout,
            size: drawn,
            painted: self.terminal_overlay_layout_of(drawn),
            origin: (x.round(), y.round()),
            moving: true,
        }
    }

    /// The frame as painted, mid step included.
    pub(super) fn terminal_overlay_bounds(&self) -> Bounds {
        self.terminal_overlay_drawn().painted.frame
    }

    pub(super) fn terminal_overlay_visible(&self) -> bool {
        self.ui_host
            .as_ref()
            .is_some_and(|ui| ui.terminal.session.borrow().visible)
    }

    fn terminal_overlay_program(&self) -> bool {
        self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.session.borrow();
            state.visible && state.program.is_some()
        })
    }

    pub(super) fn terminal_overlay_owns(&self, pane_id: PaneId) -> bool {
        self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.session.borrow();
            let owned = state
                .pane
                .iter()
                .chain(state.program_panes())
                .any(|pane| pane.pane_id() == pane_id);
            owned
        })
    }

    pub(super) fn terminal_overlay_pane(&self) -> Option<Arc<dyn Pane>> {
        self.ui_host.as_ref()?.terminal.session.borrow().shown()
    }

    pub(super) fn terminal_overlay_content(&self, pane_id: PaneId) -> Option<Bounds> {
        let pane = self.terminal_overlay_pane()?;
        let overlay = self
            .pane_state(pane.pane_id())
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.pane_id());
        (pane.pane_id() == pane_id || overlay == Some(pane_id))
            .then(|| self.terminal_overlay_layout().content)
    }

    pub(super) fn pane_padding_left_top(&self, pane_id: PaneId) -> (f32, f32) {
        if let Some(bounds) = self.terminal_overlay_content(pane_id) {
            let border = self.get_os_border();
            (
                bounds.x - border.left.get() as f32,
                bounds.y - border.top.get() as f32,
            )
        } else {
            self.padding_left_top()
        }
    }

    pub(super) fn positioned_terminal_overlay(&self) -> Option<PositionedPane> {
        self.terminal_overlay_pane()?;
        let pane = self.get_active_pane_or_overlay()?;
        let size = self.terminal_overlay_layout().size;
        Some(PositionedPane {
            pane,
            index: 0,
            is_active: true,
            is_zoomed: false,
            left: 0,
            top: 0,
            width: size.cols,
            height: size.rows,
            pixel_width: size.pixel_width,
            pixel_height: size.pixel_height,
        })
    }

    pub(super) fn hide_terminal_overlay(&mut self) {
        if !self.terminal_overlay_visible() {
            return;
        }
        if let Some(ui) = &self.ui_host {
            let panes = {
                let mut state = ui.terminal.session.borrow_mut();
                for pane in state.pane.iter().chain(state.program_panes()) {
                    pane.focus_changed(false);
                }
                state.visible = false;
                state.motion = None;
                let program = state.put_away(alive);
                let shell = if ui.provider.quick_terminal_config().preserve_shell {
                    None
                } else {
                    state.discard_shell()
                };
                [program, shell]
            };
            for pane in panes.iter().flatten() {
                remove(pane);
            }
            ui.terminal.blur.borrow_mut().take();
        }
        self.current_mouse_capture = None;
        self.current_mouse_buttons.clear();
        self.dragging = None;
        if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.focus_changed(self.focused.is_some());
        }
        if let Some(window) = &self.window {
            window.invalidate();
        }
    }

    /// Hides while nothing runs in the pane, judged as a close is; otherwise Escape reaches it.
    pub(super) fn terminal_overlay_escape(&mut self) -> bool {
        if self.terminal_overlay_pane().is_none() {
            self.hide_terminal_overlay();
            return true;
        }
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return false;
        };
        let Some(sent) = pane
            .downcast_ref::<ClientPane>()
            .map(|client| client.key_down_if_busy(KeyCode::Escape, KeyModifiers::NONE))
        else {
            let idle = pane.can_close_without_prompting(CloseReason::Pane);
            if idle {
                self.hide_terminal_overlay();
            }
            return idle;
        };
        let Some(window) = self.window.clone() else {
            return false;
        };
        promise::spawn::spawn(async move {
            let sent = sent.await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| match sent {
                Ok(true) => {}
                Ok(false) => {
                    let shown = tw.terminal_overlay_pane();
                    if shown.is_some_and(|shown| shown.pane_id() == pane.pane_id()) {
                        tw.hide_terminal_overlay();
                    }
                }
                // A stock server cannot judge; the key still reaches the pane.
                Err(_) => {
                    pane.key_down(KeyCode::Escape, KeyModifiers::NONE).ok();
                }
            })));
        })
        .detach();
        true
    }

    pub(super) fn toggle_terminal_overlay(&mut self) {
        if self.terminal_overlay_visible() && !self.terminal_overlay_program() {
            self.hide_terminal_overlay();
            return;
        }
        self.show_terminal_overlay(None);
    }

    pub(super) fn toggle_terminal_program(&mut self, program: TerminalProgram) {
        let shown = self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.session.borrow();
            state.visible
                && state
                    .program
                    .as_ref()
                    .is_some_and(|current| current.spec == program)
        });
        if shown {
            self.hide_terminal_overlay();
            return;
        }
        let reusable = self
            .ui_host
            .as_ref()
            .is_some_and(|ui| ui.terminal.session.borrow().reusable(&program, alive));
        let launch = if reusable {
            None
        } else {
            match self.terminal_launch(&program.spawn) {
                Ok(launch) => Some(launch),
                Err(error) => {
                    let error = format!("Quick terminal: {error:#}");
                    log::warn!("{error}");
                    self.vtabs_message_for(self.mux_window_id, Message::Error(error));
                    return;
                }
            }
        };
        self.show_terminal_overlay(Some((program, launch)));
    }

    /// Resolved like a split of the active tab's pane; `Beside` falls back to any pane of the domain.
    fn terminal_launch(&self, spawn: &SpawnCommand) -> anyhow::Result<Launch> {
        let mux = Mux::get();
        let current = mux
            .get_active_tab_for_window(self.mux_window_id)
            .and_then(|tab| tab.get_active_pane());
        let mut spawn = spawn.clone();
        if let Some(ui) = &self.ui_host {
            ui.provider.prepare_command(&mut spawn);
        }
        let domain = mux
            .resolve_spawn_tab_domain(current.as_ref().map(|pane| pane.pane_id()), &spawn.domain)?;
        let current = current.filter(|pane| pane.domain_id() == domain.domain_id());
        let explicit = spawn.cwd.map(|cwd| cwd.to_string_lossy().into_owned());
        let (target, cwd) = if domain
            .downcast_ref::<ClientDomain>()
            .is_some_and(|client| !client.local_pane_layout())
        {
            let pane = current
                .or_else(|| {
                    mux.iter_panes()
                        .into_iter()
                        .find(|pane| pane.domain_id() == domain.domain_id())
                })
                .with_context(|| format!("no pane open in {}", domain.domain_name()))?;
            let pane = pane
                .downcast_ref::<ClientPane>()
                .context("not a client pane")?
                .remote_pane_id();
            (Target::Beside(domain, pane), explicit)
        } else {
            let cwd = explicit.or_else(|| {
                current
                    .and_then(|pane| pane.get_current_working_dir(CachePolicy::FetchImmediate))
                    .and_then(|url| directory(&url))
            });
            (Target::Domain(domain), cwd)
        };
        let (args, env) = (spawn.args, spawn.set_environment_variables);
        let command = (args.is_some() || !env.is_empty()).then(|| {
            let mut command = args
                .map(|args| CommandBuilder::from_argv(args.into_iter().map(Into::into).collect()))
                .unwrap_or_else(CommandBuilder::new_default_prog);
            for (key, value) in env {
                command.env(key, value);
            }
            command
        });
        Ok(Launch {
            target,
            command,
            cwd,
        })
    }

    /// A program without a launch reopens its kept command.
    fn show_terminal_overlay(&mut self, program: Option<(TerminalProgram, Option<Launch>)>) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(ui) = self.ui_host.as_ref() else {
            return;
        };
        let preserve_shell = ui.provider.quick_terminal_config().preserve_shell;
        let state_ref = ui.terminal.session.clone();
        if !preserve_shell {
            let discarded = state_ref.borrow_mut().discard_shell();
            if let Some(pane) = discarded {
                remove(&pane);
            }
        }
        if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.focus_changed(false);
        }
        self.current_mouse_capture = None;
        self.current_mouse_buttons.clear();
        self.dragging = None;
        let previous = state_ref.borrow_mut().put_away(alive);
        let ended = state_ref.borrow_mut().prune(alive);
        for pane in previous.iter().chain(&ended) {
            remove(pane);
        }
        let mut state = state_ref.borrow_mut();
        state.visible = true;
        state.motion = None;
        window.invalidate();
        if let Some((spec, launch)) = program {
            if let Some(kept) = state.take_kept(&spec) {
                if let Some(pane) = &kept.pane {
                    pane.set_config(term_config(&self.config, kept.spec.colors.as_ref()));
                    pane.focus_changed(true);
                }
                state.program = Some(kept);
                return;
            }
            let Some(launch) = launch else {
                state.visible = false;
                return;
            };
            state.programs += 1;
            let id = state.programs;
            state.program = Some(Program::new(id, spec));
            drop(state);
            self.spawn_terminal_pane(
                &state_ref,
                Slot::Program(id),
                Ok(launch.target),
                launch.command,
                launch.cwd,
            );
            return;
        }
        let dead = state
            .pane
            .as_ref()
            .is_some_and(|pane| pane.is_dead() || Mux::get().get_pane(pane.pane_id()).is_none());
        if dead {
            if let Some(pane) = state.pane.take() {
                Mux::get().remove_pane(pane.pane_id());
            }
        }
        if let Some(pane) = &state.pane {
            pane.focus_changed(true);
            return;
        }
        if state.pending.is_some() {
            return;
        }
        drop(state);
        let local = SpawnCommand {
            domain: SpawnTabDomain::DomainName("local".into()),
            ..Default::default()
        };
        let launch = self
            .terminal_launch(&SpawnCommand::default())
            .or_else(|_| self.terminal_launch(&local));
        let launch = match launch {
            Ok(launch) => launch,
            Err(error) => {
                log::warn!("Quick terminal: {error:#}");
                return;
            }
        };
        if let Target::Beside(domain, _) = &launch.target {
            let workspace = quick_workspace(domain, self.mux_window_id, Slot::Shell(0));
            let panes = quick_panes(&workspace);
            if !preserve_shell {
                panes.iter().for_each(remove);
            } else if let Some(pane) = panes.into_iter().next() {
                pane.set_config(Arc::new(config::TermConfig::with_config(
                    self.config.clone(),
                )));
                pane.focus_changed(true);
                let mut state = state_ref.borrow_mut();
                state.tab = mirrored_tab(&pane);
                state.pane = Some(pane);
                return;
            }
        }
        let slot = state_ref.borrow_mut().launch_shell();
        self.spawn_terminal_pane(
            &state_ref,
            slot,
            Ok(launch.target),
            launch.command,
            launch.cwd,
        );
    }

    /// A pane arriving after its slot stopped wanting it is removed.
    fn spawn_terminal_pane(
        &self,
        state: &Rc<RefCell<State>>,
        slot: Slot,
        target: anyhow::Result<Target>,
        command: Option<CommandBuilder>,
        cwd: Option<String>,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let state = Rc::downgrade(state);
        let size = self.terminal_overlay_layout().size;
        let config = self.config.clone();
        let window_id = self.mux_window_id;
        promise::spawn::spawn(async move {
            let result = async {
                match target? {
                    Target::Domain(domain) => {
                        if domain.state() == DomainState::Detached {
                            domain.attach(Some(window_id)).await?;
                        }
                        domain.spawn_pane(size, command, cwd).await
                    }
                    Target::Beside(domain, pane_id) => {
                        let client = domain
                            .downcast_ref::<ClientDomain>()
                            .ok_or_else(|| anyhow::anyhow!("not a client domain"))?;
                        let workspace = quick_workspace(&domain, window_id, slot);
                        // Programs a quit GUI couldn't end would otherwise outlive it.
                        if matches!(slot, Slot::Program(_))
                            && state.upgrade().is_some_and(|state| {
                                state.borrow_mut().swept.insert(workspace.clone())
                            })
                        {
                            quick_panes(&workspace).iter().for_each(remove);
                        }
                        let spawned =
                            ClientDomain::get_client_inner_for_domain(domain.domain_id())?
                                .client
                                .spawn_beside_pane(SpawnBesidePane {
                                    pane_id,
                                    command,
                                    command_dir: cwd,
                                    size,
                                    workspace,
                                })
                                .await?;
                        // The pane arrives with this client's mirror of the server's hidden tab.
                        client.resync().await?;
                        client
                            .remote_to_local_pane_id(spawned.pane_id)
                            .and_then(|id| Mux::get().get_pane(id))
                            .ok_or_else(|| anyhow::anyhow!("pane {} not mirrored", spawned.pane_id))
                    }
                }
            }
            .await;
            let Some(state) = state.upgrade().filter(|state| state.borrow().wants(slot)) else {
                if let Ok(pane) = result {
                    remove(&pane);
                }
                return;
            };
            let mut state = state.borrow_mut();
            match result {
                Ok(pane) => {
                    pane.set_config(term_config(&config, state.colors(slot).as_ref()));
                    state.settle(slot, Some(pane.clone()));
                    pane.focus_changed(
                        state
                            .shown()
                            .is_some_and(|shown| shown.pane_id() == pane.pane_id()),
                    );
                }
                Err(error) => {
                    state.settle(slot, None);
                    window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        tw.vtabs_message_for(
                            window_id,
                            Message::Error(format!("Quick terminal: {error:#}")),
                        );
                    })));
                }
            }
            window.invalidate();
        })
        .detach();
    }

    pub(super) fn resize_terminal_overlay(&mut self) {
        if let Some(ui) = &self.ui_host {
            let config = ui.provider.quick_terminal_config();
            let discarded = {
                let mut state = ui.terminal.session.borrow_mut();
                if state.configure(config) {
                    self.quad_generation += 1;
                }
                let now = Instant::now();
                if state
                    .motion
                    .as_mut()
                    .is_some_and(|motion| !motion.advance(now))
                {
                    state.motion = None;
                }
                let shell = if !config.preserve_shell && (!state.visible || state.program.is_some())
                {
                    state.discard_shell()
                } else {
                    None
                };
                let mut discarded = state.prune(alive);
                discarded.extend(shell);
                let reloaded = state.applied.as_ref().map_or(true, |applied| {
                    !std::ptr::eq::<config::Config>(&**applied, &*self.config)
                });
                if reloaded {
                    state.applied = Some(self.config.clone());
                    if let Some(pane) = &state.pane {
                        pane.set_config(term_config(&self.config, None));
                    }
                    for program in state.program.iter().chain(&state.kept) {
                        if let Some(pane) = &program.pane {
                            pane.set_config(term_config(
                                &self.config,
                                program.spec.colors.as_ref(),
                            ));
                        }
                    }
                }
                discarded
            };
            for pane in &discarded {
                remove(pane);
            }
        }
        let replacement = self
            .ui_host
            .as_ref()
            .and_then(|ui| ui.terminal.session.borrow_mut().follow());
        if let Some(pane) = replacement {
            pane.set_config(Arc::new(config::TermConfig::with_config(
                self.config.clone(),
            )));
            let shown = self.terminal_overlay_pane();
            pane.focus_changed(shown.is_some_and(|shown| shown.pane_id() == pane.pane_id()));
        }
        let Some(pane) = self.terminal_overlay_pane() else {
            return;
        };
        if pane.is_dead() || Mux::get().get_pane(pane.pane_id()).is_none() {
            self.hide_terminal_overlay();
            return;
        }
        let size = self.terminal_overlay_layout().size;
        let dims = pane.get_dimensions();
        if dims.cols != size.cols || dims.viewport_rows != size.rows || dims.dpi != size.dpi {
            if let Err(error) = pane.resize(size) {
                log::warn!("Quick terminal resize: {error:#}");
            }
            if let Some(overlay) = &self.pane_state(pane.pane_id()).overlay {
                overlay.pane.resize(size).ok();
            }
        }
    }

    /// `AdjustPaneSize` in cells; Right and Down grow, and the frame stays centered.
    pub(super) fn adjust_terminal_overlay(&mut self, direction: PaneDirection, amount: usize) {
        let (horizontal, sign) = match direction {
            PaneDirection::Right => (true, 1.),
            PaneDirection::Left => (true, -1.),
            PaneDirection::Down => (false, 1.),
            PaneDirection::Up => (false, -1.),
            PaneDirection::Next | PaneDirection::Prev => return,
        };
        let Some(ui) = &self.ui_host else {
            return;
        };
        let bounds = self.vtabs_geometry().ui_bounds(true);
        let drawn = self.terminal_overlay_drawn();
        let (frame, inner) = (drawn.layout.frame, drawn.layout.inner);
        let metrics = &self.render_metrics.cell_size;
        let (extent, current, padding, cell, minimum_cells) = if horizontal {
            let padding = frame.width - inner.width;
            (
                bounds.width,
                frame.width,
                padding,
                metrics.width as f32,
                MIN_CELLS.0,
            )
        } else {
            let padding = frame.height - inner.height;
            (
                bounds.height,
                frame.height,
                padding,
                metrics.height as f32,
                MIN_CELLS.1,
            )
        };
        if extent <= 0. {
            return;
        }
        let size = self.terminal_overlay_target().0;
        let grid = |frame: f32| {
            let size = if horizontal {
                (frame / extent, size.1)
            } else {
                (size.0, frame / extent)
            };
            let layout = self.terminal_overlay_layout_of(size);
            let cells = if horizontal {
                layout.size.cols
            } else {
                layout.size.rows
            };
            cells as f32
        };
        let cells = sign * amount as f32;
        let target = step_cells(
            current,
            cells,
            cell,
            (minimum_cells * cell + padding, extent),
            grid,
        );
        if target == current {
            return;
        }
        let duration = ui.provider.quick_terminal_config().transition;
        let mut state = ui.terminal.session.borrow_mut();
        state.adjust(horizontal, target / extent);
        state.motion = (!duration.is_zero()).then(|| Motion {
            size: drawn.size,
            origin: drawn.origin,
            start: Instant::now(),
            duration,
            eased: 0.,
        });
        drop(state);
        if let Some(window) = &self.window {
            window.invalidate();
        }
    }

    pub(crate) fn inspect_terminal_overlay(&self) -> serde_json::Value {
        let layout = self.terminal_overlay_layout();
        let (pane, program, kept) = self
            .ui_host
            .as_ref()
            .map_or((None, None, Vec::new()), |ui| {
                let state = ui.terminal.session.borrow();
                (
                    state.pane.as_ref().map(|pane| pane.pane_id()),
                    state
                        .program
                        .as_ref()
                        .and_then(|program| program.pane.as_ref())
                        .map(|pane| pane.pane_id()),
                    state
                        .kept
                        .iter()
                        .filter_map(|program| program.pane.as_ref())
                        .map(|pane| pane.pane_id())
                        .collect(),
                )
            });
        serde_json::json!({
            "visible": self.terminal_overlay_visible(),
            "pane": pane,
            "program": program,
            "kept": kept,
            "bounds": {"x": layout.frame.x, "y": layout.frame.y, "width": layout.frame.width, "height": layout.frame.height},
            "content": {"x": layout.content.x, "y": layout.content.y, "width": layout.content.width, "height": layout.content.height},
            "cols": layout.size.cols, "rows": layout.size.rows,
        })
    }

    pub(super) fn paint_terminal_overlay(&mut self) -> anyhow::Result<()> {
        if !self.terminal_overlay_visible() {
            return Ok(());
        }
        let drawn = self.terminal_overlay_drawn();
        let (layout, painted) = (drawn.layout, drawn.painted);
        let Some(config) = self.terminal_overlay_config() else {
            return Ok(());
        };
        if drawn.moving {
            let interval = Duration::from_secs(1) / self.config.max_fps.clamp(1, 1000) as u32;
            self.update_next_frame_time(Some(Instant::now() + interval));
        }
        let layer = self.render_state.as_ref().unwrap().layer_for_zindex(6)?;
        let mut layers = layer.quad_allocator();
        self.filled_rectangle(
            &mut layers,
            0,
            euclid::rect(
                0.,
                0.,
                self.dimensions.pixel_width as f32,
                self.dimensions.pixel_height as f32,
            ),
            LinearRgba::with_components(0., 0., 0., 0.2),
        )?;
        let background = config
            .background_color
            .unwrap_or_else(|| {
                self.terminal_overlay_pane()
                    .map(|pane| pane.palette().background)
                    .unwrap_or(self.palette().background)
                    .to_linear()
            })
            .mul_alpha(config.opacity);
        let scale = border_width(self.dimensions.dpi as u32);
        let border_color = match config.border {
            TerminalBorder::None => None,
            TerminalBorder::Palette => {
                let foreground = self.terminal_overlay_pane().map_or_else(
                    || self.palette().foreground,
                    |pane| pane.palette().foreground,
                );
                Some(foreground.to_linear().mul_alpha(0.22))
            }
            TerminalBorder::Color(color) => Some(color),
        };
        let border = if let Some(color) = border_color {
            self.paint_terminal_border(&mut layers, painted.frame, scale, color)?;
            scale
        } else {
            0.
        };
        self.paint_vtabs_rounded_fill(
            &mut layers,
            Bounds {
                x: painted.frame.x + border,
                y: painted.frame.y + border,
                width: (painted.frame.width - 2. * border).max(0.),
                height: (painted.frame.height - 2. * border).max(0.),
            },
            12. * scale - border,
            background,
        )?;
        if let Some(pos) = self.positioned_terminal_overlay() {
            self.update_text_cursor(&pos);
            if self.focused.is_some() {
                pos.pane.advise_focus();
            }
            let mut quads = HeapQuadAllocator::default();
            self.paint_pane(&pos, &mut TripleLayerQuadAllocator::Heap(&mut quads))?;
            let x = self.dimensions.pixel_width as f32 / 2.;
            let y = self.dimensions.pixel_height as f32 / 2.;
            let content = layout.content;
            let offset = (drawn.origin.0 - content.x, drawn.origin.1 - content.y);
            let inner = painted.inner;
            quads.apply_transformed(
                &mut layers,
                offset,
                1.,
                (
                    drawn.origin.0.max(inner.x) - x,
                    drawn.origin.1.max(inner.y) - y,
                    (drawn.origin.0 + content.width).min(inner.x + inner.width) - x,
                    (drawn.origin.1 + content.height).min(inner.y + inner.height) - y,
                ),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/vtabs/terminal_overlay.rs"]
mod vtabs_tests;

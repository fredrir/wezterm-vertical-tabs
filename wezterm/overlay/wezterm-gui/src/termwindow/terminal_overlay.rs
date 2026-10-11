//! A PTY surface owned by the existing GUI window, outside its tab projection.
use super::ui_host::{Bounds, QuickTerminalConfig, TerminalProgram};
use super::{TermWindow, TermWindowNotif};
use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator};
use codec::SpawnBesidePane;
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use mux::domain::{Domain, DomainState};
use mux::pane::{CachePolicy, CloseReason, Pane, PaneId};
use mux::tab::{PositionedPane, TabId};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use portable_pty::CommandBuilder;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wezterm_client::domain::ClientDomain;
use wezterm_client::pane::ClientPane;
use wezterm_term::{KeyCode, KeyModifiers, TerminalSize};
use window::{color::LinearRgba, WindowOps};

mod blur;
mod frame;

const SHELL_SIZE: (f32, f32) = (0.75, 0.75);

#[derive(Default)]
pub(super) struct TerminalOverlay(Rc<RefCell<State>>, RefCell<Option<blur::BackdropBlur>>);

#[derive(Default)]
struct State {
    visible: bool,
    pending: Option<u64>,
    launches: u64,
    config: Option<QuickTerminalConfig>,
    pane: Option<Arc<dyn Pane>>,
    /// The shell's mirrored tab, where `attach_mux` swaps in its replacement.
    tab: Option<TabId>,
    /// Shown instead of the shell; exists only while visible.
    program: Option<Program>,
    programs: u64,
}

struct Program {
    id: u64,
    spec: TerminalProgram,
    pane: Option<Arc<dyn Pane>>,
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

#[derive(Clone, Copy)]
enum Slot {
    Shell(u64),
    Program(u64),
}

impl State {
    /// The shell's tab shows a different pane once `attach_mux` replaced it.
    fn follow(&mut self) -> Option<Arc<dyn Pane>> {
        let current = self.pane.as_ref()?;
        let replacement = Mux::get().get_tab(self.tab?)?.get_active_pane()?;
        (replacement.pane_id() != current.pane_id()).then(|| {
            self.pane = Some(replacement.clone());
            replacement
        })
    }

    fn shown(&self) -> Option<Arc<dyn Pane>> {
        if !self.visible {
            return None;
        }
        match &self.program {
            Some(program) => program.pane.clone(),
            None => self.pane.clone(),
        }
    }

    fn size(&self, shell: (f32, f32)) -> (f32, f32) {
        self.program.as_ref().map_or(shell, |program| {
            (
                program.spec.width.unwrap_or(shell.0),
                program.spec.height.unwrap_or(shell.1),
            )
        })
    }

    fn wants(&self, slot: Slot) -> bool {
        match slot {
            Slot::Shell(id) => self.pending == Some(id),
            Slot::Program(id) => self
                .program
                .as_ref()
                .is_some_and(|program| program.id == id),
        }
    }

    fn discard_shell(&mut self) -> Option<Arc<dyn Pane>> {
        self.pending = None;
        self.tab = None;
        self.pane.take()
    }

    fn launch_shell(&mut self) -> Slot {
        self.launches += 1;
        self.pending = Some(self.launches);
        Slot::Shell(self.launches)
    }

    fn settle(&mut self, slot: Slot, pane: Option<Arc<dyn Pane>>) {
        match (slot, pane) {
            (Slot::Shell(_), pane) => {
                self.pending = None;
                self.visible &= pane.is_some() || self.program.is_some();
                self.tab = pane.as_ref().and_then(mirrored_tab);
                self.pane = pane;
            }
            (Slot::Program(_), Some(pane)) => {
                if let Some(program) = &mut self.program {
                    program.pane = Some(pane);
                }
            }
            (Slot::Program(_), None) => {
                self.program = None;
                self.visible = false;
            }
        }
    }
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

impl Drop for State {
    fn drop(&mut self) {
        let program = self.program.take().and_then(|program| program.pane);
        for pane in self.pane.take().into_iter().chain(program) {
            remove(&pane);
        }
    }
}

/// Client domains spawn panes outside tabs only with `local_pane_layout`.
fn local_pane_layout(config: &config::Config, name: &str) -> bool {
    config
        .unix_domains
        .iter()
        .any(|unix| unix.name == name && unix.local_pane_layout)
        || config
            .tls_clients
            .iter()
            .any(|tls| tls.name == name && tls.local_pane_layout)
        || config
            .ssh_domains()
            .iter()
            .any(|ssh| ssh.name == name && ssh.local_pane_layout)
}

/// `to_file_path` refuses the host a shell's OSC 7 includes, so only the path is decoded.
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

#[derive(Clone, Copy, Debug)]
struct Layout {
    frame: Bounds,
    content: Bounds,
    size: TerminalSize,
}

impl Layout {
    fn new(
        bounds: Bounds,
        (width, height): (f32, f32),
        cell_width: f32,
        cell_height: f32,
        dpi: u32,
    ) -> Self {
        let frame = Bounds {
            x: bounds.x + bounds.width * (1. - width) / 2.,
            y: bounds.y + bounds.height * (1. - height) / 2.,
            width: bounds.width * width,
            height: bounds.height * height,
        };
        let padding = (12. * dpi as f32 / 96.)
            .min(frame.width / 4.)
            .min(frame.height / 4.);
        let cols = ((frame.width - 2. * padding) / cell_width).floor().max(1.) as usize;
        let rows = ((frame.height - 2. * padding) / cell_height)
            .floor()
            .max(1.) as usize;
        let width = cols as f32 * cell_width;
        let height = rows as f32 * cell_height;
        Self {
            frame,
            content: Bounds {
                x: frame.x + (frame.width - width) / 2.,
                y: frame.y + (frame.height - height) / 2.,
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
                let state = ui.terminal.0.borrow();
                let program = state
                    .program
                    .as_ref()
                    .and_then(|program| program.pane.as_ref());
                state
                    .pane
                    .iter()
                    .chain(program)
                    .all(|pane| pane.can_close_without_prompting(mux::pane::CloseReason::Window))
            })
    }

    fn terminal_overlay_layout(&self) -> Layout {
        Layout::new(
            self.vtabs_geometry().ui_bounds(true),
            self.ui_host.as_ref().map_or(SHELL_SIZE, |ui| {
                ui.terminal
                    .0
                    .borrow()
                    .size(ui.provider.quick_terminal_config().size)
            }),
            self.render_metrics.cell_size.width as f32,
            self.render_metrics.cell_size.height as f32,
            self.dimensions.dpi as u32,
        )
    }

    pub(super) fn terminal_overlay_bounds(&self) -> Bounds {
        self.terminal_overlay_layout().frame
    }

    pub(super) fn terminal_overlay_visible(&self) -> bool {
        self.ui_host
            .as_ref()
            .is_some_and(|ui| ui.terminal.0.borrow().visible)
    }

    fn terminal_overlay_program(&self) -> bool {
        self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.0.borrow();
            state.visible && state.program.is_some()
        })
    }

    pub(super) fn terminal_overlay_owns(&self, pane_id: PaneId) -> bool {
        self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.0.borrow();
            let program = state
                .program
                .as_ref()
                .and_then(|program| program.pane.as_ref());
            state
                .pane
                .iter()
                .chain(program)
                .any(|pane| pane.pane_id() == pane_id)
        })
    }

    pub(super) fn terminal_overlay_pane(&self) -> Option<Arc<dyn Pane>> {
        self.ui_host.as_ref()?.terminal.0.borrow().shown()
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
                let mut state = ui.terminal.0.borrow_mut();
                state.visible = false;
                if let Some(pane) = &state.pane {
                    pane.focus_changed(false);
                }
                let program = state.program.take().and_then(|program| program.pane);
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
            ui.terminal.1.borrow_mut().take();
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
            let state = ui.terminal.0.borrow();
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
        let Some(launch) = self.terminal_launch(&program.spawn) else {
            return;
        };
        self.show_terminal_overlay(Some((program, launch)));
    }

    /// Resolved like a split of the active tab's pane; `None` without a pane to spawn beside.
    fn terminal_launch(&self, spawn: &SpawnCommand) -> Option<Launch> {
        let mux = Mux::get();
        let current = mux
            .get_active_tab_for_window(self.mux_window_id)
            .and_then(|tab| tab.get_active_pane());
        let mut spawn = spawn.clone();
        if let Some(ui) = &self.ui_host {
            ui.provider.prepare_command(false, &mut spawn);
        }
        let domain = mux
            .resolve_spawn_tab_domain(current.as_ref().map(|pane| pane.pane_id()), &spawn.domain)
            .map_err(|error| log::warn!("Quick terminal: {error:#}"))
            .ok()?;
        let current = current.filter(|pane| pane.domain_id() == domain.domain_id());
        let explicit = spawn.cwd.map(|cwd| cwd.to_string_lossy().into_owned());
        let (target, cwd) = if domain.downcast_ref::<ClientDomain>().is_some()
            && !local_pane_layout(&config::configuration(), domain.domain_name())
        {
            let pane = current?.downcast_ref::<ClientPane>()?.remote_pane_id();
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
        Some(Launch {
            target,
            command,
            cwd,
        })
    }

    fn show_terminal_overlay(&mut self, program: Option<(TerminalProgram, Launch)>) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(ui) = self.ui_host.as_ref() else {
            return;
        };
        let preserve_shell = ui.provider.quick_terminal_config().preserve_shell;
        let state_ref = ui.terminal.0.clone();
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
        let previous = state_ref
            .borrow_mut()
            .program
            .take()
            .and_then(|program| program.pane);
        if let Some(pane) = previous {
            remove(&pane);
        }
        let mut state = state_ref.borrow_mut();
        state.visible = true;
        window.invalidate();
        if let Some((spec, launch)) = program {
            state.programs += 1;
            let id = state.programs;
            state.program = Some(Program {
                id,
                spec,
                pane: None,
            });
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
        let Some(launch) = self
            .terminal_launch(&SpawnCommand::default())
            .or_else(|| self.terminal_launch(&local))
        else {
            return;
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
                        if let Slot::Program(_) = slot {
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
                    pane.set_config(Arc::new(config::TermConfig::with_config(config)));
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
                            serde_json::json!({"error": format!("Quick terminal: {error:#}")}),
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
                let mut state = ui.terminal.0.borrow_mut();
                if state.config != Some(config) {
                    state.config = Some(config);
                    self.quad_generation += 1;
                }
                if !config.preserve_shell && (!state.visible || state.program.is_some()) {
                    state.discard_shell()
                } else {
                    None
                }
            };
            if let Some(pane) = discarded {
                remove(&pane);
            }
        }
        let replacement = self
            .ui_host
            .as_ref()
            .and_then(|ui| ui.terminal.0.borrow_mut().follow());
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

    pub(crate) fn inspect_terminal_overlay(&self) -> serde_json::Value {
        let layout = self.terminal_overlay_layout();
        let (pane, program) = self.ui_host.as_ref().map_or((None, None), |ui| {
            let state = ui.terminal.0.borrow();
            (
                state.pane.as_ref().map(|pane| pane.pane_id()),
                state
                    .program
                    .as_ref()
                    .and_then(|program| program.pane.as_ref())
                    .map(|pane| pane.pane_id()),
            )
        });
        serde_json::json!({
            "visible": self.terminal_overlay_visible(),
            "pane": pane,
            "program": program,
            "bounds": {"x": layout.frame.x, "y": layout.frame.y, "width": layout.frame.width, "height": layout.frame.height},
            "content": {"x": layout.content.x, "y": layout.content.y, "width": layout.content.width, "height": layout.content.height},
            "cols": layout.size.cols, "rows": layout.size.rows,
        })
    }

    pub(super) fn paint_terminal_overlay(&mut self) -> anyhow::Result<()> {
        if !self.terminal_overlay_visible() {
            return Ok(());
        }
        let layout = self.terminal_overlay_layout();
        let config = self
            .ui_host
            .as_ref()
            .unwrap()
            .provider
            .quick_terminal_config();
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
        let border = (self.dimensions.dpi as f32 / 96.).max(1.);
        let border_color = config
            .border_color
            .unwrap_or_else(|| self.palette().foreground.to_linear().mul_alpha(0.22));
        self.paint_terminal_border(&mut layers, layout.frame, border, border_color)?;
        self.paint_vtabs_rounded_fill(
            &mut layers,
            Bounds {
                x: layout.frame.x + border,
                y: layout.frame.y + border,
                width: (layout.frame.width - 2. * border).max(0.),
                height: (layout.frame.height - 2. * border).max(0.),
            },
            11. * border,
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
            quads.apply_transformed(
                &mut layers,
                (0., 0.),
                1.,
                (
                    content.x - x,
                    content.y - y,
                    content.x + content.width - x,
                    content.y + content.height - y,
                ),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/vtabs/terminal_overlay.rs"]
mod vtabs_tests;

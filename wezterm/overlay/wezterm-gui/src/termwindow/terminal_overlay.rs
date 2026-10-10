//! A PTY surface owned by the existing GUI window, outside its tab projection: a persistent
//! shell, or a program that ends when hidden.
use super::ui_host::{Bounds, TerminalProgram};
use super::{TermWindow, TermWindowNotif};
use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator};
use mux::domain::{Domain, DomainState};
use mux::pane::{CachePolicy, Pane, PaneId};
use mux::tab::PositionedPane;
use mux::Mux;
use portable_pty::CommandBuilder;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wezterm_term::TerminalSize;
use window::{color::LinearRgba, WindowOps};

const SHELL_SIZE: (f32, f32) = (0.75, 0.75);

#[derive(Default)]
pub(super) struct TerminalOverlay(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    visible: bool,
    pending: bool,
    pane: Option<Arc<dyn Pane>>,
    /// Shown instead of the shell; exists only while visible.
    program: Option<Program>,
    programs: u64,
}

struct Program {
    id: u64,
    spec: TerminalProgram,
    pane: Option<Arc<dyn Pane>>,
}

struct Launch {
    domain: Arc<dyn Domain>,
    command: Option<CommandBuilder>,
    cwd: Option<String>,
}

#[derive(Clone, Copy)]
enum Slot {
    Shell,
    Program(u64),
}

impl State {
    fn shown(&self) -> Option<Arc<dyn Pane>> {
        if !self.visible {
            return None;
        }
        match &self.program {
            Some(program) => program.pane.clone(),
            None => self.pane.clone(),
        }
    }

    fn size(&self) -> (f32, f32) {
        self.program.as_ref().map_or(SHELL_SIZE, |program| {
            (
                program.spec.width.unwrap_or(SHELL_SIZE.0),
                program.spec.height.unwrap_or(SHELL_SIZE.1),
            )
        })
    }

    fn wants(&self, slot: Slot) -> bool {
        match slot {
            Slot::Shell => self.pending,
            Slot::Program(id) => self.program.as_ref().is_some_and(|program| program.id == id),
        }
    }

    fn settle(&mut self, slot: Slot, pane: Option<Arc<dyn Pane>>) {
        match (slot, pane) {
            (Slot::Shell, pane) => {
                self.pending = false;
                self.visible &= pane.is_some() || self.program.is_some();
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

#[derive(Debug, PartialEq)]
enum Host {
    Domain,
    Local,
}

/// Client domains spawn panes outside tabs only with `local_pane_layout`; a unix socket
/// without a proxy shares this machine, so its programs run locally.
fn client_host(config: &config::Config, name: &str) -> Option<Host> {
    let unix = config.unix_domains.iter().find(|unix| unix.name == name);
    if unix.is_some_and(|unix| unix.local_pane_layout)
        || config
            .tls_clients
            .iter()
            .any(|tls| tls.name == name && tls.local_pane_layout)
        || config
            .ssh_domains()
            .iter()
            .any(|ssh| ssh.name == name && ssh.local_pane_layout)
    {
        return Some(Host::Domain);
    }
    unix.filter(|unix| unix.proxy_command.is_none())
        .map(|_| Host::Local)
}

fn program_domain(domain: Arc<dyn Domain>) -> Option<Arc<dyn Domain>> {
    if domain
        .downcast_ref::<wezterm_client::domain::ClientDomain>()
        .is_none()
    {
        return Some(domain);
    }
    match client_host(&config::configuration(), domain.domain_name())? {
        Host::Domain => Some(domain),
        Host::Local => Mux::get().get_domain_by_name("local"),
    }
}

/// Remote panes report their host, which a decoded path doesn't need.
fn directory(url: &url::Url) -> Option<String> {
    let mut url = url.clone();
    url.set_host(None).ok()?;
    url.to_file_path().ok()?.into_os_string().into_string().ok()
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
                let program = state.program.as_ref().and_then(|program| program.pane.as_ref());
                state.pane.iter().chain(program).all(|pane| {
                    pane.can_close_without_prompting(mux::pane::CloseReason::Window)
                })
            })
    }

    fn terminal_overlay_layout(&self) -> Layout {
        Layout::new(
            self.vtabs_geometry().ui_bounds(true),
            self.ui_host
                .as_ref()
                .map_or(SHELL_SIZE, |ui| ui.terminal.0.borrow().size()),
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

    pub(super) fn terminal_overlay_program(&self) -> bool {
        self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.0.borrow();
            state.visible && state.program.is_some()
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
            let program = {
                let mut state = ui.terminal.0.borrow_mut();
                state.visible = false;
                if let Some(pane) = &state.pane {
                    pane.focus_changed(false);
                }
                state.program.take().and_then(|program| program.pane)
            };
            if let Some(pane) = program {
                remove(&pane);
            }
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
        let Some(launch) = self.terminal_program_launch(&program) else {
            return;
        };
        self.show_terminal_overlay(Some((program, launch)));
    }

    /// Resolved like a split of the active tab's pane; `None` where its domain can't host one.
    fn terminal_program_launch(&self, program: &TerminalProgram) -> Option<Launch> {
        let mux = Mux::get();
        let current = mux
            .get_active_tab_for_window(self.mux_window_id)
            .and_then(|tab| tab.get_active_pane());
        let mut spawn = program.spawn.clone();
        if let Some(ui) = &self.ui_host {
            ui.provider.prepare_command(false, &mut spawn);
        }
        let domain = mux
            .resolve_spawn_tab_domain(current.as_ref().map(|pane| pane.pane_id()), &spawn.domain)
            .map_err(|error| log::warn!("Quick terminal: {error:#}"))
            .ok()?;
        let cwd = match spawn.cwd {
            Some(cwd) => Some(cwd.to_string_lossy().into_owned()),
            None => current
                .filter(|pane| pane.domain_id() == domain.domain_id())
                .and_then(|pane| pane.get_current_working_dir(CachePolicy::FetchImmediate))
                .and_then(|url| directory(&url)),
        };
        let domain = program_domain(domain)?;
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
            domain,
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
        let state_ref = ui.terminal.0.clone();
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
                Ok(launch.domain),
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
        if state.pending {
            return;
        }
        state.pending = true;
        drop(state);
        let mut spawn = config::keyassignment::SpawnCommand::default();
        if let Some(ui) = &self.ui_host {
            ui.provider.prepare_command(false, &mut spawn);
        }
        let mut command = CommandBuilder::new_default_prog();
        for (key, value) in spawn.set_environment_variables {
            command.env(key, value);
        }
        let domain = Mux::get()
            .get_domain_by_name("local")
            .ok_or_else(|| anyhow::anyhow!("Local terminal domain is unavailable"));
        self.spawn_terminal_pane(&state_ref, Slot::Shell, domain, Some(command), None);
    }

    /// A pane arriving after its slot stopped wanting it is removed.
    fn spawn_terminal_pane(
        &self,
        state: &Rc<RefCell<State>>,
        slot: Slot,
        domain: anyhow::Result<Arc<dyn Domain>>,
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
                let domain = domain?;
                if domain.state() == DomainState::Detached {
                    domain.attach(Some(window_id)).await?;
                }
                domain.spawn_pane(size, command, cwd).await
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
        let background = self
            .terminal_overlay_pane()
            .map(|pane| pane.palette().background)
            .unwrap_or(self.palette().background)
            .to_linear();
        let border = (self.dimensions.dpi as f32 / 96.).max(1.);
        let border_color = self.palette().foreground.to_linear().mul_alpha(0.22);
        self.paint_vtabs_rounded_fill(&mut layers, layout.frame, 12. * border, border_color)?;
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

//! A PTY surface owned by the existing GUI window, outside its tab projection.
use super::ui_host::{Bounds, Message, QuickTerminalConfig, TerminalBorder, TerminalProgram};
use super::{TermWindow, TermWindowNotif};
use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator};
use anyhow::Context;
use config::keyassignment::{PaneDirection, SpawnCommand, SpawnTabDomain};
use config::{
    DimensionContext, HorizontalWindowContentAlignment, VerticalWindowContentAlignment,
    WindowContentAlignment, WindowPadding,
};
use mux::domain::{Domain, DomainState};
use mux::pane::{CachePolicy, CloseReason, Pane, PaneId};
use mux::tab::{PositionedPane, TabId};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use portable_pty::CommandBuilder;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wezterm_client::domain::ClientDomain;
use wezterm_client::pane::ClientPane;
use wezterm_term::color::ColorPalette;
use wezterm_term::{KeyCode, KeyModifiers, TerminalSize};
use window::{color::LinearRgba, WindowOps};

mod blur;
mod frame;
mod launch;
mod layout;
mod session;
use session::{Hidden, Open, Session, Slot};

/// The quick terminal's render layer, above the sidebar UI's.
const ZINDEX: i8 = 6;

#[derive(Default)]
pub(super) struct TerminalOverlay {
    session: Rc<RefCell<Session>>,
    space: Option<String>,
    inactive: HashMap<String, Rc<RefCell<Session>>>,
    blur: RefCell<Option<blur::BackdropBlur>>,
}

impl TerminalOverlay {
    fn sessions(&self) -> impl Iterator<Item = &Rc<RefCell<Session>>> {
        std::iter::once(&self.session).chain(self.inactive.values())
    }

    fn select_space(
        &mut self,
        space: &str,
        preserve_shell: bool,
        alive: impl Fn(&Arc<dyn Pane>) -> bool,
    ) -> Option<Hidden> {
        if self.space.as_deref() == Some(space) {
            return None;
        }
        let hidden = self.session.borrow_mut().hide(preserve_shell, alive);
        let next = self.inactive.remove(space).unwrap_or_default();
        let previous = std::mem::replace(&mut self.session, next);
        if let Some(previous_space) = self.space.replace(space.to_owned()) {
            self.inactive.insert(previous_space, previous);
        }
        self.blur.borrow_mut().take();
        Some(hidden)
    }

    fn retain_spaces(&mut self, exists: impl Fn(&str) -> bool) {
        self.inactive.retain(|space, _| exists(space));
    }
}

fn mirrored_tab(pane: &Arc<dyn Pane>) -> Option<TabId> {
    Mux::get()
        .resolve_pane_id(pane.pane_id())
        .map(|(_, _, tab)| tab)
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

fn border_width(dpi: u32) -> f32 {
    (dpi as f32 / 96.).max(1.)
}

fn corner_radius(dpi: u32) -> f32 {
    12. * border_width(dpi)
}

impl TermWindow {
    pub(super) fn sync_terminal_overlay_space(&mut self) {
        let Some(ui) = &mut self.ui_host else {
            return;
        };
        let hidden = ui.terminal.select_space(
            ui.provider.selected_space(),
            ui.provider.quick_terminal_config().preserve_shell,
            alive,
        );
        let provider = &ui.provider;
        ui.terminal
            .retain_spaces(|space| provider.space_exists(space));
        if let Some(hidden) = hidden {
            self.finish_hiding_terminal_overlay(hidden);
        }
    }

    pub(super) fn terminal_overlays_can_close(&self) -> bool {
        self.ui_host
            .iter()
            .chain(self.vtabs_suspended.values())
            .all(|ui| {
                ui.terminal.sessions().all(|session| {
                    let state = session.borrow();
                    let idle = state.panes().all(|pane| {
                        pane.can_close_without_prompting(mux::pane::CloseReason::Window)
                    });
                    idle
                })
            })
    }

    pub(super) fn terminal_overlay_config(&self) -> Option<QuickTerminalConfig> {
        let ui = self.ui_host.as_ref()?;
        let config = ui.provider.quick_terminal_config();
        let state = ui.terminal.session.borrow();
        Some(state.effective_config(config))
    }

    pub(super) fn terminal_overlay_visible(&self) -> bool {
        self.ui_host
            .as_ref()
            .is_some_and(|ui| ui.terminal.session.borrow().visible())
    }

    fn terminal_overlay_program(&self) -> bool {
        self.ui_host.as_ref().is_some_and(|ui| {
            let state = ui.terminal.session.borrow();
            state.showing_program()
        })
    }

    pub(super) fn terminal_overlay_owns(&self, pane_id: PaneId) -> bool {
        self.ui_host.as_ref().is_some_and(|ui| {
            ui.terminal.sessions().any(|session| {
                let state = session.borrow();
                let owned = state.panes().any(|pane| pane.pane_id() == pane_id);
                owned
            })
        })
    }

    pub(super) fn terminal_overlay_pane(&self) -> Option<Arc<dyn Pane>> {
        self.ui_host.as_ref()?.terminal.session.borrow().shown()
    }

    pub(super) fn terminal_overlay_shows(&self, pane_id: PaneId) -> bool {
        self.terminal_overlay_pane()
            .is_some_and(|pane| pane.pane_id() == pane_id)
    }

    /// The shown pane, or the overlay (such as copy mode) above it.
    pub(super) fn terminal_overlay_active_pane(&self) -> Option<Arc<dyn Pane>> {
        let pane = self.terminal_overlay_pane()?;
        let state = self.pane_state(pane.pane_id());
        let overlay = state.overlay.as_ref().map(|overlay| overlay.pane.clone());
        Some(overlay.unwrap_or(pane))
    }

    /// The pane's palette, with the quick terminal's background while it shows the pane.
    pub(super) fn terminal_overlay_palette(&self, pane: &Arc<dyn Pane>) -> ColorPalette {
        let mut palette = pane.palette();
        if self.terminal_overlay_content(pane.pane_id()).is_some() {
            let config = self.terminal_overlay_config();
            if let Some(color) = config.and_then(|config| config.background_color) {
                palette.background = color.to_srgb();
            }
        }
        palette
    }

    /// Painted beneath the quick terminal, which takes their focus while it shows.
    pub(super) fn panes_beneath_terminal_overlay(&self) -> Vec<PositionedPane> {
        let mut panes = self.get_background_panes();
        if self.terminal_overlay_visible() {
            for pane in &mut panes {
                pane.is_active = false;
            }
        }
        panes
    }

    pub(super) fn terminal_overlay_focus_changed(&mut self, focused: bool) {
        if !focused
            && self
                .terminal_overlay_config()
                .is_some_and(|config| config.auto_hide)
        {
            self.hide_terminal_overlay();
        }
    }

    pub(super) fn hide_terminal_overlay(&mut self) {
        if !self.terminal_overlay_visible() {
            return;
        }
        if let Some(ui) = &self.ui_host {
            let hidden = ui
                .terminal
                .session
                .borrow_mut()
                .hide(ui.provider.quick_terminal_config().preserve_shell, alive);
            ui.terminal.blur.borrow_mut().take();
            self.finish_hiding_terminal_overlay(hidden);
        }
    }

    fn finish_hiding_terminal_overlay(&mut self, hidden: Hidden) {
        for pane in &hidden.unfocus {
            pane.focus_changed(false);
        }
        for pane in &hidden.discarded {
            remove(pane);
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

    pub(super) fn resize_terminal_overlay(&mut self) {
        if let Some(ui) = &self.ui_host {
            let config = ui.provider.quick_terminal_config();
            for session in ui.terminal.sessions() {
                let mut state = session.borrow_mut();
                let refresh = state.refresh(config, &self.config, Instant::now(), alive);
                if refresh.changed {
                    self.quad_generation += 1;
                }
                for update in refresh.configurations {
                    update
                        .pane
                        .set_config(term_config(&self.config, update.colors.as_ref()));
                }
                for pane in &refresh.discarded {
                    remove(pane);
                }
                if let Some(pane) = state.follow() {
                    pane.set_config(term_config(&self.config, None));
                    pane.focus_changed(
                        state
                            .shown()
                            .is_some_and(|shown| shown.pane_id() == pane.pane_id()),
                    );
                }
            }
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
        let (pane, program, kept) = self
            .ui_host
            .as_ref()
            .map_or((None, None, Vec::new()), |ui| {
                ui.terminal.session.borrow().pane_ids()
            });
        let rect = |b: Bounds| {
            serde_json::json!({
                "x": b.x, "y": b.y, "width": b.width, "height": b.height,
            })
        };
        serde_json::json!({
            "visible": self.terminal_overlay_visible(),
            "pane": pane,
            "program": program,
            "kept": kept,
            "bounds": rect(layout.frame),
            "content": rect(layout.content),
            "cols": layout.size.cols, "rows": layout.size.rows,
        })
    }
}

#[cfg(test)]
#[path = "../../tests/vtabs/terminal_overlay.rs"]
mod vtabs_tests;

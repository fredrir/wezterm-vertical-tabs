//! A persistent PTY surface owned by the existing GUI window, outside its tab projection.
use super::ui_host::Bounds;
use super::{TermWindow, TermWindowNotif};
use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator};
use mux::pane::{Pane, PaneId};
use mux::tab::PositionedPane;
use mux::Mux;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wezterm_term::TerminalSize;
use window::{color::LinearRgba, WindowOps};

#[derive(Default)]
pub(super) struct TerminalOverlay(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    visible: bool,
    pending: bool,
    pane: Option<Arc<dyn Pane>>,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(pane) = self.pane.take() {
            pane.kill();
            if let Some(mux) = Mux::try_get() {
                mux.remove_pane(pane.pane_id());
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Layout {
    frame: Bounds,
    content: Bounds,
    size: TerminalSize,
}

impl Layout {
    fn new(bounds: Bounds, cell_width: f32, cell_height: f32, dpi: u32) -> Self {
        let frame = Bounds {
            x: bounds.x + bounds.width / 8.,
            y: bounds.y + bounds.height / 8.,
            width: bounds.width * 0.75,
            height: bounds.height * 0.75,
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
                ui.terminal.0.borrow().pane.as_ref().map_or(true, |pane| {
                    pane.can_close_without_prompting(mux::pane::CloseReason::Window)
                })
            })
    }

    fn terminal_overlay_layout(&self) -> Layout {
        Layout::new(
            self.vtabs_geometry().ui_bounds(true),
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

    pub(super) fn terminal_overlay_pane(&self) -> Option<Arc<dyn Pane>> {
        let ui = self.ui_host.as_ref()?;
        let state = ui.terminal.0.borrow();
        state.visible.then(|| state.pane.clone()).flatten()
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
            let mut state = ui.terminal.0.borrow_mut();
            state.visible = false;
            if let Some(pane) = &state.pane {
                pane.focus_changed(false);
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
        if self.terminal_overlay_visible() {
            self.hide_terminal_overlay();
            return;
        }
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
        let mut state = state_ref.borrow_mut();
        if state
            .pane
            .as_ref()
            .is_some_and(|pane| pane.is_dead() || Mux::get().get_pane(pane.pane_id()).is_none())
        {
            if let Some(pane) = state.pane.take() {
                Mux::get().remove_pane(pane.pane_id());
            }
        }
        state.visible = true;
        window.invalidate();
        if let Some(pane) = &state.pane {
            pane.focus_changed(true);
            return;
        }
        if state.pending {
            return;
        }
        state.pending = true;
        drop(state);
        let state = Rc::downgrade(&state_ref);
        let size = self.terminal_overlay_layout().size;
        let config = self.config.clone();
        let mut spawn = config::keyassignment::SpawnCommand::default();
        if let Some(ui) = &self.ui_host {
            ui.provider.prepare_command(false, &mut spawn);
        }
        let mut command = portable_pty::CommandBuilder::new_default_prog();
        for (key, value) in spawn.set_environment_variables {
            command.env(key, value);
        }
        let window_id = self.mux_window_id;
        promise::spawn::spawn(async move {
            let result = async {
                let domain = Mux::get()
                    .get_domain_by_name("local")
                    .ok_or_else(|| anyhow::anyhow!("Local terminal domain is unavailable"))?;
                domain.spawn_pane(size, Some(command), None).await
            }
            .await;
            let Some(state) = state.upgrade() else {
                if let Ok(pane) = result {
                    Mux::get().remove_pane(pane.pane_id());
                }
                return;
            };
            let mut state = state.borrow_mut();
            state.pending = false;
            match result {
                Ok(pane) => {
                    pane.set_config(Arc::new(config::TermConfig::with_config(config)));
                    pane.focus_changed(state.visible);
                    state.pane = Some(pane);
                }
                Err(error) => {
                    state.visible = false;
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
        let pane = self
            .ui_host
            .as_ref()
            .and_then(|ui| ui.terminal.0.borrow().pane.clone());
        serde_json::json!({
            "visible": self.terminal_overlay_visible(),
            "pane": pane.as_ref().map(|pane| pane.pane_id()),
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

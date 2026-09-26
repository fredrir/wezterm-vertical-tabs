use crate::SidebarUi;
use crate::components::palette::palette;
use crate::components::tooltip::tooltip;
use crate::components::{dialog, list, menu};
use crate::element::ElementId;
use crate::overlays::{Form, FormKind, Overlay, action_exists};
use crate::runtime::canvas::Canvas;
use crate::runtime::motion;
use crate::views::sidebar::SidebarProps;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{Block, Widget};
use std::time::Duration;
use vtabs_core::Model;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceTransform {
    /// Fraction of the surface width. Applied by the compositor, never to pane sizes.
    pub translate_x: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct FrameUpdate {
    pub revision: u64,
    pub resized: bool,
    pub changed_cells: Vec<(u16, u16)>,
    pub dirty_rows: Vec<u16>,
    pub cursor: Option<Position>,
    /// Rows the caret moves down to follow text the host centers in a two-row surface.
    pub cursor_shift: f32,
    pub ime_rect: Option<Rect>,
    pub transform: SurfaceTransform,
}

pub(crate) struct Frame {
    pub buffer: Buffer,
    pub staging: Buffer,
    pub model_revision: Option<u64>,
    pub number: u64,
    pub dirty: bool,
    pub last: Duration,
    pub now: Duration,
    pub last_rail: Option<vtabs_core::RailMode>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            buffer: Buffer::default(),
            staging: Buffer::default(),
            model_revision: None,
            number: 0,
            dirty: true,
            last: Duration::ZERO,
            now: Duration::ZERO,
            last_rail: None,
        }
    }
}

impl SidebarUi {
    /// Returns None when a terminal repaint can reuse the previously committed UI.
    /// Resize publishes one fully composed frame; the previous buffer survives until swap.
    pub fn render(&mut self, model: &Model, area: Rect, now: Duration) -> Option<FrameUpdate> {
        self.frame.now = now;
        let resized = self.frame.buffer.area != area;
        if resized {
            self.sidebar.reveal_selection = true;
            self.frame.staging.resize(area);
            self.cancel_effects();
            self.pointer.press = None;
            self.frame.dirty = true;
        }
        if self.frame.model_revision != Some(model.revision) {
            if self
                .overlays
                .pending_form
                .is_some_and(|revision| revision != model.revision)
            {
                self.dismiss();
            }
            self.frame.model_revision = Some(model.revision);
            self.theme.apply(model);
            self.settings.config_owned.clone_from(&model.config_owned);
            if self
                .frame
                .last_rail
                .is_some_and(|rail| rail != model.settings.rail)
                && motion::enabled(&model.settings)
                && self.host.live()
                && !self.is_modal()
                && !area.is_empty()
            {
                let direction = if model.settings.side == vtabs_core::Side::Right {
                    0.15
                } else {
                    -0.15
                };
                self.transition_surface(
                    direction,
                    0.0,
                    now,
                    Duration::from_millis(u64::from(model.settings.animation_ms)),
                );
            }
            self.frame.last_rail = Some(model.settings.rail);
            self.sidebar.space_activity.clear();
            self.sidebar.space_activity.extend(
                model
                    .tabs
                    .values()
                    .filter(|tab| tab.unread || tab.bell)
                    .map(|tab| tab.space_id.clone()),
            );
            if self.sidebar.last_space.as_ref() != Some(&model.selected_space) {
                self.sidebar.reveal_selection = true;
                self.sidebar.scroll = 0;
                self.sidebar.settings_place = None;
                if let Some(index) = model
                    .spaces
                    .iter()
                    .position(|space| space.id == model.selected_space)
                {
                    let rows = usize::from(self.sidebar.spaces.height).max(1);
                    self.sidebar.space_scroll =
                        list::reveal_rows(self.sidebar.space_scroll, index, rows);
                }
                self.sidebar.last_space = Some(model.selected_space.clone());
            }
            if self.sidebar.last_tab != model.selected_tab {
                // Composition reveals the row once the frame's row capacity is known.
                self.sidebar.reveal_selection = true;
                self.sidebar.last_tab = model.selected_tab;
            }
            self.frame.dirty = true;
            if !motion::enabled(&model.settings) {
                self.cancel_effects();
            }
            self.prune_targets(model);
        }
        if !self.host.visible {
            return None;
        }
        if self.caret.tick(now) | self.tooltip.tick(now, self.pointer.hovered.is_some()) {
            self.frame.dirty = true;
        }
        self.advance_press(model, now);
        self.advance_drop(model, now);
        if !self.frame.dirty && self.effects.cell.is_none() {
            return self
                .effects
                .surface
                .is_some()
                .then(|| self.finish_frame(false, Vec::new(), Vec::new(), now));
        }
        self.frame.staging.resize(area);
        self.frame.staging.reset();
        self.paint.reset();
        Block::default()
            .style(self.theme.base())
            .render(area, &mut self.frame.staging);
        if area.width > 0 && area.height > 0 {
            self.compose(model, area);
        }
        let elapsed = now
            .saturating_sub(self.frame.last)
            .min(Duration::from_millis(1000));
        if let Some(effect) = self.effects.cell.as_mut() {
            let effect_area = self.effects.cell_area.unwrap_or(area).intersection(area);
            effect.process(
                tachyonfx::Duration::from_millis(
                    elapsed.as_millis().min(u128::from(u32::MAX)) as u32
                ),
                &mut self.frame.staging,
                effect_area,
            );
            if effect.done() {
                self.effects.cell = None;
                self.effects.cell_area = None;
            }
        }
        let changed_cells = if resized {
            (area.y..area.bottom())
                .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
                .collect()
        } else {
            self.frame
                .buffer
                .diff_iter(&self.frame.staging)
                .map(|(x, y, _)| (x, y))
                .collect::<Vec<_>>()
        };
        let mut dirty_rows = Vec::new();
        for &(_, y) in &changed_cells {
            if dirty_rows.last() != Some(&y) {
                dirty_rows.push(y);
            }
        }
        std::mem::swap(&mut self.frame.buffer, &mut self.frame.staging);
        self.frame.dirty = false;
        Some(self.finish_frame(resized, changed_cells, dirty_rows, now))
    }

    fn finish_frame(
        &mut self,
        resized: bool,
        changed_cells: Vec<(u16, u16)>,
        dirty_rows: Vec<u16>,
        now: Duration,
    ) -> FrameUpdate {
        let mut transform = SurfaceTransform {
            translate_x: 0.0,
            opacity: 1.0,
        };
        if let Some(surface) = self.effects.surface {
            let t = motion::progress(now.saturating_sub(surface.start), surface.duration);
            transform.translate_x = motion::lerp(surface.from, surface.to, motion::ease_out(t));
            if t >= 1.0 {
                self.effects.surface = None;
            }
        }
        self.frame.last = now;
        self.frame.number = self.frame.number.wrapping_add(1);
        let field = self
            .active_editor()
            .and_then(|slot| Some((self.editor(slot)?, self.paint.field(&slot.element())?)));
        let ime_rect = field
            .filter(|(_, field)| field.rect.width > 0)
            .map(|(editor, field)| {
                let column = editor
                    .cursor_columns()
                    .saturating_sub(editor.scroll_columns)
                    .min(usize::from(field.rect.width - 1));
                Rect::new(field.rect.x + column as u16, field.rect.y, 1, 1)
            });
        let cursor_shift = field.map_or(0.0, |(_, field)| field.shift);
        FrameUpdate {
            revision: self.frame.number,
            resized,
            changed_cells,
            dirty_rows,
            cursor: field.and_then(|(_, field)| field.caret),
            cursor_shift,
            ime_rect,
            transform,
        }
    }

    fn prune_targets(&mut self, model: &Model) {
        let valid = |id: &ElementId| match id {
            ElementId::Tab(id) | ElementId::CloseTab(id) => model.tabs.contains_key(id),
            ElementId::Pane(id, pane) | ElementId::ClosePane(id, pane) => model
                .tabs
                .get(id)
                .is_some_and(|tab| tab.panes.iter().any(|entry| entry.id == *pane)),
            ElementId::Space(id) => model.spaces.iter().any(|space| &space.id == id),
            _ => true,
        };
        if self
            .sidebar
            .rename
            .as_ref()
            .is_some_and(|rename| !model.tabs.contains_key(&rename.id))
        {
            self.sidebar.rename = None;
            self.caret.stop();
            if self.focused == Some(ElementId::Editor) {
                self.focused = None;
            }
        }
        if self
            .pointer
            .press
            .as_ref()
            .is_some_and(|press| !valid(&press.id))
        {
            self.pointer.press = None;
        }
        if self.focused.as_ref().is_some_and(|id| !valid(id)) {
            self.focused = None;
        }
        if self.pointer.hovered.as_ref().is_some_and(|id| !valid(id)) {
            self.pointer.hovered = None;
            self.tooltip.shown = false;
        }
        if self.pointer.drag.as_ref().is_some_and(|id| !valid(id)) {
            self.pointer.drag = None;
        }
        let prune = |overlay: &mut Overlay| {
            if let Overlay::Menu(menu) = overlay {
                menu.items.retain(|item| action_exists(model, &item.action));
                if let Some(search) = &mut menu.search {
                    search
                        .all_items
                        .retain(|item| action_exists(model, &item.action));
                }
                menu.selected = menu.selected.min(menu.items.len().saturating_sub(1));
            }
        };
        if let Some(overlay) = &mut self.overlays.current {
            prune(overlay);
        }
        for overlay in &mut self.overlays.stack {
            prune(overlay);
        }
        if self
            .overlays
            .current
            .as_ref()
            .is_some_and(|overlay| match overlay {
                Overlay::Form(Form {
                    kind: FormKind::RenameTab(id),
                    ..
                }) => !model.tabs.contains_key(id),
                Overlay::Form(Form {
                    kind:
                        FormKind::RenameSpace(id)
                        | FormKind::SpaceIcon(id)
                        | FormKind::SpaceAccent(id)
                        | FormKind::SpaceRules(id),
                    ..
                }) => !model.spaces.iter().any(|space| &space.id == id),
                _ => false,
            })
        {
            self.dismiss();
        }
    }

    fn compose(&mut self, model: &Model, area: Rect) {
        let width = self
            .host
            .sidebar_columns
            .unwrap_or(area.width)
            .min(area.width);
        let right = model.settings.side == vtabs_core::Side::Right;
        // A successful form can close during this render before the host contracts
        // its viewport. Keep rail targets within the configured width in that frame.
        let sidebar = if area.width > width {
            let x = if right { area.right() - width } else { area.x };
            Rect::new(x, area.y, width, area.height)
        } else {
            area
        };
        self.sidebar.rect = sidebar;
        let anchor = self.anchor_position();
        let editing = self.focused == Some(ElementId::Editor);
        let mut cx = Canvas {
            buf: &mut self.frame.staging,
            paint: &mut self.paint,
            theme: &self.theme,
            pointer: &self.pointer,
            focused: self.focused.as_ref(),
            caret: self.caret.visible && self.host.focused,
        };
        let props = SidebarProps {
            header_inset: self.host.header_inset,
            settings_open: self.settings.open,
            settings_listed: self.settings.listed,
        };
        self.sidebar.render(model, sidebar, props, &mut cx);
        if self.settings.open {
            let page = if area.width > sidebar.width {
                let x = if right { area.x } else { sidebar.right() };
                Rect::new(x, area.y, area.width - sidebar.width, area.height)
            } else {
                area
            };
            if page == area {
                cx.paint.clear_targets();
            }
            let overlay_open = self.overlays.current.is_some();
            self.settings.render(model, page, overlay_open, &mut cx);
        }
        if let Some(overlay) = &mut self.overlays.current {
            // Modal hit regions replace underlying targets; background clicks dismiss.
            cx.paint.clear_targets();
            self.overlays.rect = match overlay {
                Overlay::Menu(menu) if menu.search.is_some() => palette(&mut cx, area, menu),
                Overlay::Menu(menu) if menu.message.is_some() && dialog::fits(area) => {
                    dialog::confirm(&mut cx, area, menu)
                }
                Overlay::Menu(menu) => menu::menu(&mut cx, area, anchor, menu),
                Overlay::Form(form) => dialog::form(&mut cx, area, form, editing),
            };
        } else if self.tooltip.shown {
            let target = self
                .pointer
                .hovered
                .as_ref()
                .and_then(|id| cx.paint.hit(id))
                .cloned();
            match target {
                Some(target) => tooltip(&mut cx, area, sidebar, &target),
                None => self.tooltip.hide(),
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/runtime/frame.rs"]
mod tests;

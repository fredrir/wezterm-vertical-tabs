use crate::SidebarUi;
use crate::components::list;
use crate::element::ElementId;
use crate::events::{DropMotion, DropTarget, EditorSlot};
use crate::input::{Modifiers, MouseButton};
use crate::intent::UiIntent;
use crate::overlays::Overlay;
use crate::runtime::motion;
use crate::views::tab_name;
use ratatui::layout::Position;
use std::time::Duration;
use vtabs_core::{Model, TabId};

const DOUBLE_CLICK: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub(crate) struct Press {
    pub id: ElementId,
    pub down: Option<Duration>,
    pub up: Option<Option<Duration>>,
    pub level: f32,
    pub animating: bool,
}

pub(crate) struct Pointer {
    pub hovered: Option<ElementId>,
    pub drag: Option<ElementId>,
    pub dragging: bool,
    pub origin: Option<(u16, u16)>,
    pub fraction: (f32, f32),
    pub press: Option<Press>,
    pub drop: Option<DropTarget>,
    pub drop_motion: Option<DropMotion>,
    pub drag_label: String,
    pub last_click: Option<(ElementId, Duration)>,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            hovered: None,
            drag: None,
            dragging: false,
            origin: None,
            fraction: (0.5, 0.5),
            press: None,
            drop: None,
            drop_motion: None,
            drag_label: String::new(),
            last_click: None,
        }
    }
}

impl Pointer {
    pub(crate) fn cancel_drag(&mut self) {
        self.drag = None;
        self.dragging = false;
        self.drop = None;
        self.drop_motion = None;
    }
}

impl SidebarUi {
    pub(crate) fn pointer_move(&mut self, model: &Model, x: u16, y: u16) {
        if let Some(slot) = self
            .pointer
            .drag
            .as_ref()
            .and_then(|id| self.editor_slot(id))
        {
            self.click_editor(slot, x, true);
            return;
        }
        if self
            .pointer
            .origin
            .is_some_and(|(px, py)| px.abs_diff(x).saturating_add(py.abs_diff(y)) > 1)
            && self.pointer.drag.is_some()
            && !self.pointer.dragging
        {
            self.pointer.dragging = true;
            let home = model.home.as_deref();
            self.pointer.drag_label = match &self.pointer.drag {
                Some(ElementId::Pane(tab, pane)) => model
                    .tabs
                    .get(tab)
                    .and_then(|tab| tab.panes.iter().find(|entry| entry.id == *pane))
                    .map(|pane| pane.label(home)),
                Some(other) => match other.row() {
                    ElementId::Tab(id) => model
                        .tabs
                        .get(&id)
                        .map(|tab| tab_name(tab, home).unwrap_or_else(|| tab.title.clone())),
                    _ => None,
                },
                None => None,
            }
            .unwrap_or_default();
            self.tooltip.hide();
        }
        if self.pointer.dragging {
            self.aim_drop(model, x, y);
        }
        let described = self
            .hit_test(x, y)
            .is_some_and(|hit| !hit.tooltip.is_empty());
        let hit = self.hit_test(x, y).map(|hit| hit.id.clone());
        if hit != self.pointer.hovered {
            self.pointer.hovered = hit;
            self.tooltip.shown = false;
            self.tooltip.deadline = (described
                && self.overlays.current.is_none()
                && self.host.focused
                && !self.pointer.dragging)
                .then_some(self.frame.now + motion::TOOLTIP_DELAY);
            self.start_effect(model);
            self.frame.dirty = true;
        }
    }

    pub(crate) fn pointer_down(
        &mut self,
        model: &Model,
        x: u16,
        y: u16,
        button: MouseButton,
        modifiers: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) {
        self.tooltip.hide();
        let hit = self.hit_test(x, y).map(|hit| hit.id.clone());
        if self.overlays.current.is_some() && !self.overlays.rect.contains(Position::new(x, y)) {
            if let Some(target) = self.editor_menu_target() {
                self.restore_editor(target);
            } else {
                self.dismiss();
            }
            return;
        }
        if hit != Some(ElementId::Editor) {
            self.finish_rename(true, intents);
        }
        self.pointer.origin = Some((x, y));
        self.pointer.dragging = false;
        if button == MouseButton::Right && self.overlays.current.is_none() {
            self.set_anchor(x, y);
        }
        match (button, hit) {
            (MouseButton::Right, Some(id)) => self.context_menu(model, id),
            (MouseButton::Middle, Some(ElementId::Tab(id))) => self.close_tab(model, id, intents),
            (MouseButton::Left, Some(id @ (ElementId::Editor | ElementId::SettingsSearch))) => {
                if let Some(slot) = self.editor_slot(&id) {
                    if slot == EditorSlot::SettingsSearch {
                        self.settings.search_focused = true;
                    }
                    self.click_editor(slot, x, modifiers.shift);
                }
                self.focused = Some(id.clone());
                self.pointer.drag = Some(id);
                self.frame.dirty = true;
            }
            (MouseButton::Left, Some(ElementId::Tab(id)))
                if self.double_clicked_title(id, x, y) =>
            {
                self.start_rename(model, id);
            }
            (MouseButton::Left, Some(id)) => {
                if matches!(self.overlays.current, Some(Overlay::Form(_))) {
                    self.caret.stop();
                }
                self.pointer.press = Some(Press {
                    id: match &id {
                        ElementId::Pane(..) => id.row(),
                        _ => id.clone(),
                    },
                    down: None,
                    up: None,
                    level: 0.0,
                    animating: true,
                });
                self.focused = Some(id.clone());
                self.pointer.drag = Some(id);
                self.frame.dirty = true;
            }
            (MouseButton::Right, None) if self.overlays.current.is_none() => self.root_menu(model),
            _ => {}
        }
    }

    pub(crate) fn pointer_up(
        &mut self,
        model: &Model,
        x: u16,
        y: u16,
        intents: &mut Vec<UiIntent>,
    ) {
        let target = self.drop_target(model, x, y);
        self.pointer.drop = None;
        let down = self.pointer.drag.take();
        self.pointer.origin = None;
        self.pointer.dragging = false;
        self.frame.dirty = true;
        if let Some(press) = &mut self.pointer.press {
            press.up.get_or_insert(None);
            press.animating = true;
        }
        let up = self.hit_test(x, y).map(|hit| hit.id.clone());
        if down.is_some() && down == up {
            let to = up.unwrap();
            if self.overlays.current.is_none() {
                self.set_anchor(x, y);
            }
            self.pointer.last_click = Some((to.clone(), self.frame.now));
            self.activate_element(model, to, intents);
            if self.overlays.current.is_none() {
                self.overlays.anchor = None;
            }
            return;
        }
        self.pointer.drop_motion = None;
        if let (Some(source), Some(target)) = (down, target) {
            self.apply_drop(model, source, target, intents);
        }
    }

    pub(crate) fn scroll(&mut self, model: &Model, x: u16, y: u16, rows: i32) {
        if let Some(overlay) = &mut self.overlays.current {
            match overlay {
                Overlay::Menu(menu) => {
                    menu.selected = list::offset(menu.selected, rows, menu.items.len());
                }
                Overlay::Form(_) => {}
            }
        } else if self.settings.open && self.settings.rect.contains(Position::new(x, y)) {
            self.settings_scroll_by(model, rows);
        } else if self.sidebar.spaces.contains(Position::new(x, y)) {
            self.sidebar.space_scroll =
                list::offset(self.sidebar.space_scroll, rows, model.spaces.len());
        } else {
            self.sidebar.scroll = list::offset(self.sidebar.scroll, rows, self.sidebar.rows.len());
        }
        self.frame.dirty = true;
    }

    fn double_clicked_title(&self, id: TabId, x: u16, y: u16) -> bool {
        self.overlays.current.is_none()
            && self
                .sidebar
                .title_rects
                .iter()
                .any(|(tab, rect)| *tab == id && rect.contains(Position::new(x, y)))
            && self.pointer.last_click.as_ref().is_some_and(|(last, at)| {
                *last == ElementId::Tab(id) && self.frame.now.saturating_sub(*at) <= DOUBLE_CLICK
            })
    }

    fn set_anchor(&mut self, x: u16, y: u16) {
        let origin = self.sidebar.rect;
        self.overlays.anchor = Some((
            i32::from(x) - i32::from(origin.x),
            i32::from(y) - i32::from(origin.y),
        ));
    }

    pub(crate) fn anchor_focused(&mut self, id: &ElementId) {
        if let Some(rect) = self.paint.hit(id).map(|hit| hit.rect) {
            self.set_anchor(rect.x, rect.bottom().saturating_sub(1));
        }
    }

    pub(crate) fn anchor_position(&self) -> Option<Position> {
        let origin = self.sidebar.rect;
        let resolve = |base: u16, offset: i32| {
            (i32::from(base) + offset).clamp(0, i32::from(u16::MAX)) as u16
        };
        self.overlays
            .anchor
            .map(|(x, y)| Position::new(resolve(origin.x, x), resolve(origin.y, y)))
    }
}

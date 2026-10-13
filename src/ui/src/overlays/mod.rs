pub(crate) mod colors;
mod forms;
pub(crate) mod menus;

use crate::SidebarUi;
use crate::actions::Action;
use crate::components::list;
use crate::element::{ElementId, EntityRef};
use crate::input::TextEditor;
use crate::views::launcher::Launcher;
use ratatui::layout::Rect;
use vtabs_core::{Model, SpaceId, TabId};

#[derive(Clone, Debug)]
pub(crate) struct MenuItem {
    pub id: String,
    pub icon: &'static str,
    pub icon_color: Option<ratatui::style::Color>,
    pub index: Option<usize>,
    pub label: String,
    pub hint: String,
    pub shortcuts: Vec<String>,
    pub keywords: String,
    pub action: Action,
    /// Shown as a trailing remove control on hover, and run by Delete.
    pub remove: Option<Action>,
    pub enabled: bool,
}

impl MenuItem {
    pub(crate) fn prune(&mut self, model: &Model) -> bool {
        if !self.action.prune(model) {
            return false;
        }
        if self
            .remove
            .as_mut()
            .is_some_and(|action| !action.prune(model))
        {
            self.remove = None;
        }
        true
    }

    pub(crate) fn new(id: impl Into<String>, label: impl Into<String>, action: Action) -> Self {
        Self {
            id: id.into(),
            icon: "",
            icon_color: None,
            index: None,
            label: label.into(),
            hint: String::new(),
            shortcuts: Vec::new(),
            keywords: String::new(),
            action,
            remove: None,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Menu {
    pub title: String,
    pub message: Option<String>,
    pub items: Vec<MenuItem>,
    pub selected: usize,
    pub scroll: usize,
    /// Wheel and scrollbar move only the viewport, never the selection.
    pub revealed: Option<usize>,
    pub visible: usize,
    pub search: Option<Launcher>,
    /// The control a dropdown opens beneath, and the option it currently holds.
    pub dropdown: Option<Rect>,
    pub checked: Option<usize>,
}

impl Menu {
    pub(crate) fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.into(),
            message: None,
            selected: items.iter().position(|item| item.enabled).unwrap_or(0),
            items,
            scroll: 0,
            revealed: None,
            visible: 0,
            search: None,
            dropdown: None,
            checked: None,
        }
    }

    pub(crate) fn follow_selection(&mut self, rows: usize) {
        self.scroll = list::follow(
            self.scroll,
            &mut self.revealed,
            self.selected,
            self.items.len(),
            rows,
        );
        self.visible = rows;
    }

    pub(crate) fn scroll_by(&mut self, rows: i32) {
        self.scroll = list::scroll_by(
            self.scroll,
            rows,
            self.items.len().saturating_sub(self.visible),
        );
    }

    pub(crate) fn has_leading_column(&self) -> bool {
        self.items
            .iter()
            .any(|item| !item.icon.is_empty() || item.index.is_some())
    }
}

#[derive(Clone, Debug)]
pub(crate) enum FormKind {
    CreateFolder,
    RenameFolder(String),
    CreateSpace,
    RenameSpace(SpaceId),
    SpaceIcon(SpaceId),
    SpaceAccent(SpaceId),
    SpaceRules(SpaceId),
    RenameTab(TabId),
    Setting(String),
}

impl FormKind {
    pub(crate) fn entity(&self) -> Option<EntityRef<'_>> {
        match self {
            Self::RenameFolder(id) => Some(EntityRef::Folder(id)),
            Self::RenameTab(id) => Some(EntityRef::Tab(*id)),
            Self::RenameSpace(id)
            | Self::SpaceIcon(id)
            | Self::SpaceAccent(id)
            | Self::SpaceRules(id) => Some(EntityRef::Space(id)),
            Self::CreateFolder | Self::CreateSpace | Self::Setting(_) => None,
        }
    }

    pub(crate) fn exists(&self, model: &Model) -> bool {
        self.entity().is_none_or(|entity| entity.exists(model))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Form {
    pub title: String,
    pub kind: FormKind,
    pub editor: TextEditor,
    pub error: Option<String>,
    pub color: Option<Box<colors::ColorPicker>>,
}

#[derive(Clone, Debug)]
pub(crate) enum Overlay {
    Menu(Menu),
    Form(Form),
}

impl Overlay {
    pub(crate) fn prune(&mut self, model: &Model) -> bool {
        match self {
            Self::Form(form) => form.kind.exists(model),
            Self::Menu(menu) => {
                menu.items.retain_mut(|item| item.prune(model));
                if let Some(search) = &mut menu.search {
                    search.all_items.retain_mut(|item| item.prune(model));
                }
                menu.selected = menu.selected.min(menu.items.len().saturating_sub(1));
                true
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Overlays {
    current: Option<Overlay>,
    stack: Vec<Overlay>,
    restore_focus: Option<ElementId>,
    pending_form: Option<u64>,
    rect: Rect,
    anchor: Option<(i32, i32)>,
}

impl Overlays {
    pub(crate) fn current(&self) -> Option<&Overlay> {
        self.current.as_ref()
    }
    pub(crate) fn current_mut(&mut self) -> Option<&mut Overlay> {
        self.current.as_mut()
    }
    pub(crate) fn is_open(&self) -> bool {
        self.current.is_some()
    }
    pub(crate) fn rect(&self) -> Rect {
        self.rect
    }
    pub(crate) fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }
    pub(crate) fn anchor(&self) -> Option<(i32, i32)> {
        self.anchor
    }
    pub(crate) fn set_anchor(&mut self, anchor: Option<(i32, i32)>) {
        self.anchor = anchor;
    }
    pub(crate) fn restore_focus_to(&mut self, focus: Option<ElementId>) {
        self.restore_focus = focus;
    }
    fn open(&mut self, overlay: Overlay, focus: Option<ElementId>) {
        if self.current.is_none() && self.stack.is_empty() {
            self.restore_focus = focus;
        }
        self.pending_form = None;
        self.current = Some(overlay);
    }
    fn push(&mut self, overlay: Overlay, focus: Option<ElementId>) {
        if let Some(overlay) = self.current.take() {
            self.stack.push(overlay);
        }
        self.open(overlay, focus);
    }
    fn back(&mut self) -> bool {
        let Some(previous) = self.stack.pop() else {
            return false;
        };
        self.pending_form = None;
        self.current = Some(previous);
        true
    }
    pub(crate) fn dismiss(&mut self) -> Option<ElementId> {
        self.current = None;
        self.pending_form = None;
        self.stack.clear();
        self.anchor = None;
        self.restore_focus.take()
    }
    pub(crate) fn submitted(&mut self, revision: u64) {
        if matches!(self.current, Some(Overlay::Form(_))) {
            self.pending_form = Some(revision);
        }
    }
    pub(crate) fn fail(&mut self, message: String) -> Result<(), String> {
        self.pending_form = None;
        match &mut self.current {
            Some(Overlay::Form(form)) => {
                form.error = Some(message);
                Ok(())
            }
            _ => Err(message),
        }
    }
    pub(crate) fn should_dismiss(&mut self, model: &Model) -> bool {
        if self
            .restore_focus
            .as_ref()
            .is_some_and(|id| !id.exists(model))
        {
            self.restore_focus = None;
        }
        self.pending_form.is_some_and(|revision| revision != model.revision)
            || self.current.as_mut().is_some_and(|overlay| !overlay.prune(model))
            // Editor context menus depend on their stashed forms.
            || self.stack.iter_mut().any(|overlay| !overlay.prune(model))
    }
}

impl SidebarUi {
    pub(crate) fn open_overlay(&mut self, overlay: Overlay) {
        self.overlays.open(overlay, self.focused.clone());
        self.overlay_opened();
    }

    pub(crate) fn push_overlay(&mut self, overlay: Overlay) {
        self.overlays.push(overlay, self.focused.clone());
        self.overlay_opened();
    }

    fn overlay_opened(&mut self) {
        self.cancel_settings_input();
        self.cancel_effects();
        self.caret.stop();
        self.caret.visible = true;
        self.tooltip.hide();
        self.frame.dirty = true;
    }

    pub(crate) fn back(&mut self) {
        if self.overlays.back() {
            self.caret.stop();
            self.frame.dirty = true;
        } else {
            self.dismiss();
        }
    }

    pub(crate) fn confirm(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        accept: &str,
        action: Action,
    ) {
        let items = vec![
            MenuItem::new("cancel", "Cancel", Action::Close),
            MenuItem::new("confirm", accept, action),
        ];
        self.push_overlay(Overlay::Menu(Menu {
            message: Some(message.into()),
            selected: 1,
            ..Menu::new(title, items)
        }));
    }

    pub(crate) fn menu(&mut self, title: impl Into<String>, items: Vec<MenuItem>) {
        self.open_overlay(Overlay::Menu(Menu::new(title, items)));
        self.caret.stop();
    }

    pub(crate) fn push_menu(&mut self, title: impl Into<String>, items: Vec<MenuItem>) {
        self.push_overlay(Overlay::Menu(Menu::new(title, items)));
    }

    pub(crate) fn editor_menu_target(&self) -> Option<ElementId> {
        match &self.overlays.current {
            Some(Overlay::Menu(menu)) => match &menu.items.first()?.action {
                Action::EditorCommand { target, .. } => Some(target.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn restore_editor(&mut self, target: ElementId) {
        self.back();
        self.focused = Some(target.clone());
        self.pointer.drag = None;
        self.pointer.origin = None;
        self.reset_caret();
        self.frame.dirty = true;
    }
}

pub(crate) fn next_enabled(items: &[MenuItem], current: usize, delta: isize) -> usize {
    list::next_enabled(items, current, delta, |item| item.enabled)
}

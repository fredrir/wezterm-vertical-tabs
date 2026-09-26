//! Menus, dialogs, forms and launchers stacked over the sidebar.
mod forms;
mod menus;

use crate::SidebarUi;
use crate::actions::Action;
use crate::components::list;
use crate::element::ElementId;
use crate::input::TextEditor;
use crate::views::launcher::Launcher;
use ratatui::layout::Rect;
use vtabs_core::{Model, SpaceId, TabId};

#[derive(Clone, Debug)]
pub(crate) struct MenuItem {
    pub id: String,
    /// Palette rows share the sidebar's icon slot and index badge.
    pub icon: &'static str,
    pub icon_color: Option<ratatui::style::Color>,
    pub index: Option<usize>,
    pub label: String,
    pub hint: String,
    /// Matched by search but not shown.
    pub keywords: String,
    pub action: Action,
    pub enabled: bool,
    pub actions: Vec<MenuItem>,
}

impl MenuItem {
    pub(crate) fn new(id: impl Into<String>, label: impl Into<String>, action: Action) -> Self {
        Self {
            id: id.into(),
            icon: "",
            icon_color: None,
            index: None,
            label: label.into(),
            hint: String::new(),
            keywords: String::new(),
            action,
            enabled: true,
            actions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Menu {
    pub title: String,
    /// A confirmation: the title asks, this explains, and the items read as buttons.
    pub message: Option<String>,
    pub items: Vec<MenuItem>,
    pub selected: usize,
    pub scroll: usize,
    pub search: Option<Launcher>,
}

impl Menu {
    pub(crate) fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.into(),
            message: None,
            selected: items.iter().position(|item| item.enabled).unwrap_or(0),
            items,
            scroll: 0,
            search: None,
        }
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

#[derive(Clone, Debug)]
pub(crate) struct Form {
    pub title: String,
    pub kind: FormKind,
    pub editor: TextEditor,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum Overlay {
    Menu(Menu),
    Form(Form),
}

#[derive(Default)]
pub(crate) struct Overlays {
    pub current: Option<Overlay>,
    pub stack: Vec<Overlay>,
    pub restore_focus: Option<ElementId>,
    pub pending_form: Option<u64>,
    pub rect: Rect,
    /// Sidebar-relative origin for the next context menu; sidebar placement changes
    /// between the sidebar-only grid and the window viewport.
    pub anchor: Option<(i32, i32)>,
}

impl Overlays {
    /// Keeps the current overlay to return to once the next one closes.
    pub(crate) fn stash(&mut self) {
        if let Some(overlay) = self.current.take() {
            self.stack.push(overlay);
        }
    }
}

impl SidebarUi {
    pub(crate) fn open_overlay(&mut self, overlay: Overlay) {
        self.cancel_effects();
        self.caret.stop();
        self.caret.visible = true;
        self.tooltip.hide();
        if self.overlays.current.is_none() && self.overlays.stack.is_empty() {
            self.overlays.restore_focus = self.focused.clone();
        }
        self.overlays.current = Some(overlay);
        self.frame.dirty = true;
    }

    pub(crate) fn back(&mut self) {
        if let Some(previous) = self.overlays.stack.pop() {
            self.overlays.current = Some(previous);
            self.caret.stop();
            self.frame.dirty = true;
        } else {
            self.dismiss();
        }
    }

    /// Every confirmation shares one dialog with the accepting button preselected.
    pub(crate) fn confirm(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        accept: &str,
        action: Action,
    ) {
        self.overlays.stash();
        let items = vec![
            MenuItem::new("cancel", "Cancel", Action::Close),
            MenuItem::new("confirm", accept, action),
        ];
        self.open_overlay(Overlay::Menu(Menu {
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
        self.overlays.stash();
        self.menu(title, items);
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
        if target == ElementId::SettingsSearch {
            self.settings.search_focused = true;
        }
        self.pointer.drag = None;
        self.pointer.origin = None;
        self.reset_caret();
        self.frame.dirty = true;
    }
}

pub(crate) fn next_enabled(items: &[MenuItem], current: usize, delta: isize) -> usize {
    list::next_enabled(items, current, delta, |item| item.enabled)
}

pub(crate) fn action_exists(model: &Model, action: &Action) -> bool {
    use vtabs_core::Intent;
    match action {
        Action::Domain(
            Intent::ActivateTab(id)
            | Intent::CloseTab(id)
            | Intent::CloseOthers(id)
            | Intent::ReturnToAuto(id)
            | Intent::MoveTabToNewWindow(id)
            | Intent::RenameTab { id, .. }
            | Intent::PinTab { id, .. }
            | Intent::MoveTab { id, .. },
        )
        | Action::RenameTab(id)
        | Action::MoveTab(id) => model.tabs.contains_key(id),
        Action::Domain(Intent::AssignTab { id, space_id }) => {
            model.tabs.contains_key(id) && model.spaces.iter().any(|space| &space.id == space_id)
        }
        Action::Domain(
            Intent::SelectSpace(id)
            | Intent::RenameSpace { id, .. }
            | Intent::EditSpace { id, .. }
            | Intent::MoveSpace { id, .. },
        )
        | Action::RenameSpace(id)
        | Action::EditSpaceIcon(id)
        | Action::EditSpaceAccent(id)
        | Action::EditSpaceRules(id)
        | Action::DeleteSpace(id) => model.spaces.iter().any(|space| &space.id == id),
        Action::Domain(Intent::DeleteSpace { id, destination }) => {
            model.spaces.len() > 1
                && model.spaces.iter().any(|space| &space.id == id)
                && destination.as_ref().is_none_or(|destination| {
                    model.spaces.iter().any(|space| &space.id == destination)
                })
        }
        Action::KillPane(tab, pane) => model
            .tabs
            .get(tab)
            .is_some_and(|tab| tab.panes.iter().any(|entry| entry.id == *pane)),
        Action::Confirm { action, .. } => action_exists(model, action),
        Action::Submenu { items, .. } => {
            items.iter().any(|item| action_exists(model, &item.action))
        }
        _ => true,
    }
}

#[cfg(test)]
#[path = "../../tests/overlays.rs"]
mod tests;

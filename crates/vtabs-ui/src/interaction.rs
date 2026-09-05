use crate::*;
use ratatui::layout::Position;
use serde_json::Value;
use std::time::Duration;
use vtabs_core::{Intent, Model, RailMode, SettingKind, settings};

impl SidebarUi {
    pub fn event(&mut self, model: &Model, event: UiInput) -> Vec<UiIntent> {
        if self
            .pending_form
            .is_some_and(|revision| revision != model.revision)
        {
            self.dismiss();
        }
        let mut intents = Vec::new();
        match event {
            UiInput::Focus(focused) => {
                self.window_focused = focused;
                if !focused {
                    self.cancel_effects();
                    self.caret_deadline = None;
                    self.tooltip_deadline = None;
                    self.drag = None;
                } else if matches!(self.overlay, Some(Overlay::Form(_))) {
                    self.reset_caret();
                }
                self.dirty = true;
            }
            UiInput::Visibility(visible) => {
                self.visible = visible;
                if !visible {
                    self.cancel_effects();
                    self.caret_deadline = None;
                    self.tooltip_deadline = None;
                } else {
                    self.dirty = true;
                    if matches!(self.overlay, Some(Overlay::Form(_))) {
                        self.reset_caret();
                    }
                }
            }
            UiInput::PointerMove { x, y } => {
                if self.drag == Some(ElementId::Editor)
                    && let Some(Overlay::Form(form)) = &mut self.overlay
                {
                    form.editor
                        .click_column(usize::from(x.saturating_sub(self.editor_rect.x)), true);
                    self.reset_caret();
                    self.dirty = true;
                    return intents;
                }
                let hit = self.hit_test(x, y).map(|hit| hit.id.clone());
                if hit != self.hovered {
                    self.hovered = hit;
                    self.show_tooltip = false;
                    self.tooltip_deadline =
                        (self.hovered.is_some() && self.overlay.is_none() && self.window_focused)
                            .then_some(self.now + Duration::from_millis(600));
                    self.dirty = true;
                }
            }
            UiInput::PointerDown {
                x,
                y,
                button,
                modifiers,
            } => {
                self.show_tooltip = false;
                self.tooltip_deadline = None;
                let hit = self.hit_test(x, y).map(|hit| hit.id.clone());
                if self.overlay.is_some() && !self.overlay_rect.contains(Position::new(x, y)) {
                    self.dismiss();
                    return intents;
                }
                match (button, hit) {
                    (MouseButton::Right, Some(id)) => self.context_menu(model, id),
                    (MouseButton::Middle, Some(ElementId::Tab(id))) => {
                        self.close_tab(model, id, &mut intents)
                    }
                    (MouseButton::Left, Some(ElementId::Editor)) => {
                        if let Some(Overlay::Form(form)) = &mut self.overlay {
                            form.editor.click_column(
                                usize::from(x.saturating_sub(self.editor_rect.x)),
                                modifiers.shift,
                            );
                        }
                        self.drag = Some(ElementId::Editor);
                        self.focused = Some(ElementId::Editor);
                        self.reset_caret();
                        self.dirty = true;
                    }
                    (MouseButton::Left, Some(id)) => {
                        self.focused = Some(id.clone());
                        self.drag = Some(id);
                        self.dirty = true;
                    }
                    (MouseButton::Right, None) if self.overlay.is_none() => self.root_menu(model),
                    _ => {}
                }
            }
            UiInput::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            } => {
                let down = self.drag.take();
                let up = self.hit_test(x, y).map(|hit| hit.id.clone());
                match (down, up) {
                    (Some(from), Some(to)) if from == to => {
                        self.activate_element(model, to, &mut intents)
                    }
                    (Some(ElementId::Tab(id)), Some(ElementId::Tab(target))) => {
                        if let Some(index) =
                            model.visible_ids().iter().position(|tab| *tab == target)
                        {
                            intents.push(UiIntent::Domain(Intent::MoveTab { id, index }));
                        }
                    }
                    (Some(ElementId::Tab(id)), Some(ElementId::Space(space_id))) => {
                        intents.push(UiIntent::Domain(Intent::AssignTab { id, space_id }))
                    }
                    (Some(ElementId::Space(id)), Some(ElementId::Space(target))) => {
                        if let Some(index) =
                            model.spaces.iter().position(|space| space.id == target)
                        {
                            intents.push(UiIntent::Domain(Intent::MoveSpace { id, index }));
                        }
                    }
                    _ => {}
                }
            }
            UiInput::PointerUp { .. } => {}
            UiInput::Scroll { x, y, rows } => {
                if let Some(overlay) = &mut self.overlay {
                    match overlay {
                        Overlay::Menu(menu) => {
                            menu.selected = offset(menu.selected, rows, menu.items.len());
                        }
                        Overlay::Settings { selected, .. } => {
                            *selected = offset(*selected, rows, settings::descriptors().len());
                        }
                        Overlay::Form(_) => {}
                    }
                } else if self.spaces_rect.contains(Position::new(x, y)) {
                    self.space_scroll = offset(self.space_scroll, rows, model.spaces.len());
                } else {
                    self.tab_scroll = offset(self.tab_scroll, rows, model.visible_ids().len());
                }
                self.dirty = true;
            }
            UiInput::Text(text) | UiInput::Paste(text) | UiInput::ImeCommit(text) => {
                if let Some(Overlay::Form(form)) = &mut self.overlay {
                    form.editor.insert(&text);
                    form.error = None;
                    self.reset_caret();
                    self.dirty = true;
                } else if let Some(Overlay::Menu(menu)) = &mut self.overlay
                    && let Some(search) = &mut menu.search
                {
                    search.editor.insert(&text);
                    filter_menu(menu);
                    self.dirty = true;
                }
            }
            UiInput::ImePreedit { text, cursor } => {
                if let Some(Overlay::Form(form)) = &mut self.overlay {
                    form.editor.set_preedit(&text, cursor);
                    self.reset_caret();
                    self.dirty = true;
                } else if let Some(Overlay::Menu(menu)) = &mut self.overlay
                    && let Some(search) = &mut menu.search
                {
                    search.editor.set_preedit(&text, cursor);
                    self.dirty = true;
                }
            }
            UiInput::Key { key, modifiers } => self.key(model, key, modifiers, &mut intents),
        }
        intents
    }

    fn key(&mut self, model: &Model, key: Key, modifiers: Modifiers, intents: &mut Vec<UiIntent>) {
        if let Some(Overlay::Form(form)) = &mut self.overlay {
            match form.editor.key(&key, modifiers) {
                EditResult::Submit => self.submit_form(model, intents),
                EditResult::Cancel => self.back(),
                EditResult::Copy(text) => {
                    intents.push(UiIntent::SetClipboard(text));
                    self.dirty = true;
                }
                EditResult::Paste => intents.push(UiIntent::RequestClipboard),
                EditResult::Changed => {
                    form.error = None;
                    self.reset_caret();
                    self.dirty = true;
                }
                EditResult::Unhandled => {}
            }
            return;
        }
        if let Some(Overlay::Menu(menu)) = &mut self.overlay {
            if let Some(search) = &mut menu.search
                && matches!(key, Key::Character(_) | Key::Backspace | Key::Delete)
            {
                match search.editor.key(&key, modifiers) {
                    EditResult::Changed => {
                        filter_menu(menu);
                        self.dirty = true;
                    }
                    EditResult::Copy(text) => intents.push(UiIntent::SetClipboard(text)),
                    EditResult::Paste => intents.push(UiIntent::RequestClipboard),
                    _ => {}
                }
                return;
            }
            match key {
                Key::Escape | Key::Left => self.back(),
                Key::Down | Key::Tab if !modifiers.shift => {
                    menu.selected = next_enabled(&menu.items, menu.selected, 1);
                    self.dirty = true;
                }
                Key::Up | Key::Tab => {
                    menu.selected = next_enabled(&menu.items, menu.selected, -1);
                    self.dirty = true;
                }
                Key::Home => {
                    menu.selected = menu.items.iter().position(|i| i.enabled).unwrap_or(0);
                    self.dirty = true;
                }
                Key::End => {
                    menu.selected = menu.items.iter().rposition(|i| i.enabled).unwrap_or(0);
                    self.dirty = true;
                }
                Key::Enter | Key::Right | Key::Character(' ') => {
                    let action = menu
                        .items
                        .get(menu.selected)
                        .filter(|item| item.enabled)
                        .map(|item| item.action.clone());
                    if let Some(action) = action {
                        self.run_action(model, action, intents);
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(Overlay::Settings { selected, .. }) = &mut self.overlay {
            let descriptors = settings::descriptors();
            match key {
                Key::Escape => self.back(),
                Key::Down | Key::Tab if !modifiers.shift => {
                    *selected = (*selected + 1) % descriptors.len();
                    self.dirty = true;
                }
                Key::Up | Key::Tab => {
                    *selected = (*selected + descriptors.len() - 1) % descriptors.len();
                    self.dirty = true;
                }
                Key::Home => {
                    *selected = 0;
                    self.dirty = true;
                }
                Key::End => {
                    *selected = descriptors.len() - 1;
                    self.dirty = true;
                }
                Key::PageDown => {
                    *selected = offset(*selected, 10, descriptors.len());
                    self.dirty = true;
                }
                Key::PageUp => {
                    *selected = offset(*selected, -10, descriptors.len());
                    self.dirty = true;
                }
                Key::Delete | Key::Backspace => {
                    let key = descriptors[*selected].key.to_owned();
                    self.run_action(model, Action::ResetSetting(key), intents);
                }
                Key::Enter | Key::Right | Key::Character(' ') => {
                    let key = descriptors[*selected].key.to_owned();
                    self.edit_setting(model, &key, intents);
                }
                _ => {}
            }
            return;
        }
        match key {
            Key::Tab => {
                if self.hits.is_empty() {
                    return;
                }
                let at = self
                    .focused
                    .as_ref()
                    .and_then(|id| self.hits.iter().position(|hit| &hit.id == id));
                let i = if modifiers.shift {
                    at.map_or(self.hits.len() - 1, |i| {
                        (i + self.hits.len() - 1) % self.hits.len()
                    })
                } else {
                    at.map_or(0, |i| (i + 1) % self.hits.len())
                };
                self.focused = Some(self.hits[i].id.clone());
                self.dirty = true;
            }
            Key::Enter | Key::Character(' ') => {
                if let Some(id) = self.focused.clone() {
                    self.activate_element(model, id, intents);
                }
            }
            Key::F10 => {
                if let Some(id) = self.focused.clone() {
                    self.context_menu(model, id);
                } else {
                    self.root_menu(model);
                }
            }
            Key::F2 => match self.focused.clone() {
                Some(ElementId::Space(id)) => {
                    self.run_action(model, Action::RenameSpace(id), intents)
                }
                Some(ElementId::Tab(id)) => self.run_action(model, Action::RenameTab(id), intents),
                _ => {}
            },
            Key::Up | Key::Down => {
                let delta = if key == Key::Up { -1 } else { 1 };
                if let Some(ElementId::Space(id)) = &self.focused {
                    if let Some(at) = model.spaces.iter().position(|space| &space.id == id) {
                        let i = offset(at, delta, model.spaces.len());
                        let id = model.spaces[i].id.clone();
                        self.focused = Some(ElementId::Space(id.clone()));
                        intents.push(UiIntent::Domain(Intent::SelectSpace(id)));
                        self.space_scroll = i.min(self.space_scroll).max(i.saturating_sub(
                            usize::from(self.spaces_rect.height).saturating_sub(1),
                        ));
                    }
                } else {
                    let at = model
                        .selected_tab
                        .and_then(|id| model.visible_ids().iter().position(|tab| *tab == id))
                        .unwrap_or(0);
                    if let Some(id) =
                        model
                            .visible_ids()
                            .get(offset(at, delta, model.visible_ids().len()))
                    {
                        self.focused = Some(ElementId::Tab(*id));
                        intents.push(UiIntent::Domain(Intent::ActivateTab(*id)));
                        self.ensure_tab_visible(model, *id);
                    }
                }
                self.dirty = true;
            }
            Key::PageUp => {
                self.tab_scroll = self.tab_scroll.saturating_sub(10);
                self.dirty = true;
            }
            Key::PageDown => {
                self.tab_scroll = offset(self.tab_scroll, 10, model.visible_ids().len());
                self.dirty = true;
            }
            Key::Home => {
                self.tab_scroll = 0;
                self.dirty = true;
            }
            Key::End => {
                self.tab_scroll = model.visible_ids().len();
                self.dirty = true;
            }
            Key::Left => intents.push(UiIntent::Domain(Intent::SetRail(RailMode::Collapsed))),
            Key::Right => intents.push(UiIntent::Domain(Intent::SetRail(RailMode::Expanded))),
            Key::Delete => {
                if let Some(ElementId::Tab(id)) = self.focused.clone() {
                    self.close_tab(model, id, intents);
                }
            }
            Key::Escape => {
                self.focused = None;
                self.cancel_effects();
                self.dirty = true;
            }
            Key::Character('+') => self.open_create_space(),
            _ => {}
        }
    }

    pub(crate) fn ensure_tab_visible(&mut self, model: &Model, id: TabId) {
        if let Some(at) = model.visible_ids().iter().position(|tab| *tab == id) {
            let card =
                if model.settings.cards && self.tabs_rect.width >= 12 && self.tabs_rect.height >= 4
                {
                    if model.settings.show_metadata { 3 } else { 2 }
                } else {
                    1
                };
            let rows = usize::from(self.tabs_rect.height / card).max(1);
            if at < self.tab_scroll {
                self.tab_scroll = at;
            } else if at >= self.tab_scroll + rows {
                self.tab_scroll = at + 1 - rows;
            }
        }
    }

    fn activate_element(&mut self, model: &Model, id: ElementId, intents: &mut Vec<UiIntent>) {
        match id {
            ElementId::CreateSpace => self.open_create_space(),
            ElementId::NewTab => intents.push(UiIntent::Domain(Intent::NewTab)),
            ElementId::Settings => self.open_settings(),
            ElementId::Rail => intents.push(UiIntent::Domain(Intent::SetRail(
                if model.settings.rail == RailMode::Expanded {
                    RailMode::Collapsed
                } else {
                    RailMode::Expanded
                },
            ))),
            ElementId::Space(id) => {
                self.tab_scroll = 0;
                intents.push(UiIntent::Domain(Intent::SelectSpace(id)));
                self.start_effect(model);
            }
            ElementId::Tab(id) => {
                intents.push(UiIntent::Domain(Intent::ActivateTab(id)));
                self.start_effect(model);
            }
            ElementId::CloseTab(id) => self.close_tab(model, id, intents),
            ElementId::Menu(id) => {
                let action = match &self.overlay {
                    Some(Overlay::Menu(menu)) => menu
                        .items
                        .iter()
                        .find(|item| item.id == id && item.enabled)
                        .map(|item| item.action.clone()),
                    _ => None,
                };
                if let Some(action) = action {
                    self.run_action(model, action, intents);
                }
            }
            ElementId::Setting(key) => self.edit_setting(model, &key, intents),
            ElementId::Submit => self.submit_form(model, intents),
            ElementId::Cancel => self.back(),
            ElementId::Editor | ElementId::PrivateInfo => {}
        }
    }

    fn close_tab(&mut self, model: &Model, id: TabId, intents: &mut Vec<UiIntent>) {
        if !model.tabs.contains_key(&id) {
            return;
        }
        let action = Action::Domain(Intent::CloseTab(id));
        if model.settings.confirm_close {
            self.run_action(
                model,
                Action::Confirm {
                    label: "Close this tab?".into(),
                    action: Box::new(action),
                },
                intents,
            );
        } else {
            self.run_action(model, action, intents);
        }
    }
    fn menu(&mut self, title: impl Into<String>, items: Vec<MenuItem>) {
        self.open_overlay(Overlay::Menu(Menu {
            title: title.into(),
            selected: items.iter().position(|item| item.enabled).unwrap_or(0),
            items,
            scroll: 0,
            search: None,
        }));
        self.caret_deadline = None;
    }
    fn root_menu(&mut self, model: &Model) {
        let mut items = vec![
            MenuItem::new("new-tab", "New tab", Action::Domain(Intent::NewTab)),
            MenuItem::new("new-space", "Create space", Action::NewSpace),
            MenuItem::new(
                "private",
                "New private window",
                Action::Domain(Intent::PrivateWindow),
            ),
            MenuItem::new(
                "reopen",
                "Reopen closed tab",
                Action::Domain(Intent::Reopen),
            ),
            MenuItem::new("settings", "Settings", Action::Settings),
            MenuItem::new(
                "reset-settings",
                "Reset settings",
                Action::Confirm {
                    label: "Reset persisted settings?".into(),
                    action: Box::new(Action::Domain(Intent::ResetSettings)),
                },
            ),
            MenuItem::new(
                "hide",
                "Hide sidebar",
                Action::Domain(Intent::SetRail(RailMode::Hidden)),
            ),
        ];
        items[3].enabled = model.can_reopen();
        items.extend(custom_menu(&model.settings.menus, "custom"));
        self.menu("Vertical tabs", items);
    }
    fn context_menu(&mut self, model: &Model, id: ElementId) {
        match id {
            ElementId::Tab(id) | ElementId::CloseTab(id) => {
                let Some(tab) = model.tabs.get(&id) else {
                    return;
                };
                let close = Action::Domain(Intent::CloseTab(id));
                let others = Action::Domain(Intent::CloseOthers(id));
                let mut items = vec![
                    MenuItem::new(
                        "activate",
                        "Activate",
                        Action::Domain(Intent::ActivateTab(id)),
                    ),
                    MenuItem::new("rename", "Rename", Action::RenameTab(id)),
                    MenuItem::new(
                        "pin",
                        if tab.pinned { "Unpin" } else { "Pin" },
                        Action::Domain(Intent::PinTab {
                            id,
                            pinned: !tab.pinned,
                        }),
                    ),
                    MenuItem::new("move-space", "Move to space ›", Action::MoveTab(id)),
                    MenuItem::new(
                        "auto",
                        "Return to automatic routing",
                        Action::Domain(Intent::ReturnToAuto(id)),
                    ),
                    MenuItem::new(
                        "move-window",
                        "Move to new window",
                        Action::Domain(Intent::MoveTabToNewWindow(id)),
                    ),
                    MenuItem::new(
                        "close-others",
                        "Close other unpinned tabs",
                        Action::Confirm {
                            label: "Close other unpinned tabs?".into(),
                            action: Box::new(others),
                        },
                    ),
                    MenuItem::new(
                        "close",
                        "Close",
                        if model.settings.confirm_close {
                            Action::Confirm {
                                label: "Close this tab?".into(),
                                action: Box::new(close),
                            }
                        } else {
                            close
                        },
                    ),
                ];
                items[4].enabled = tab.manual_assignment;
                items.extend(custom_menu(&model.settings.menus, "custom"));
                self.menu(tab.display_title(), items);
            }
            ElementId::Space(id) => {
                let Some(space) = model.spaces.iter().find(|space| space.id == id) else {
                    return;
                };
                let at = model
                    .spaces
                    .iter()
                    .position(|space| space.id == id)
                    .unwrap_or(0);
                let mut items = vec![
                    MenuItem::new(
                        "select",
                        "Select",
                        Action::Domain(Intent::SelectSpace(id.clone())),
                    ),
                    MenuItem::new("rename", "Rename", Action::RenameSpace(id.clone())),
                    MenuItem::new("icon", "Edit icon", Action::EditSpaceIcon(id.clone())),
                    MenuItem::new("accent", "Edit accent", Action::EditSpaceAccent(id.clone())),
                    MenuItem::new(
                        "rules",
                        "Edit routing rules",
                        Action::EditSpaceRules(id.clone()),
                    ),
                    MenuItem::new(
                        "up",
                        "Move up",
                        Action::Domain(Intent::MoveSpace {
                            id: id.clone(),
                            index: at.saturating_sub(1),
                        }),
                    ),
                    MenuItem::new(
                        "down",
                        "Move down",
                        Action::Domain(Intent::MoveSpace {
                            id: id.clone(),
                            index: (at + 1).min(model.spaces.len() - 1),
                        }),
                    ),
                    MenuItem::new("delete", "Delete space", Action::DeleteSpace(id)),
                ];
                items[5].enabled = at > 0;
                items[6].enabled = at + 1 < model.spaces.len();
                items[7].enabled = model.spaces.len() > 1;
                self.menu(&space.name, items);
            }
            ElementId::Setting(key) => {
                let mut items = vec![
                    MenuItem::new("edit", "Edit", Action::EditSetting(key.clone())),
                    MenuItem::new(
                        "reset",
                        "Reset to default",
                        Action::ResetSetting(key.clone()),
                    ),
                ];
                for item in &mut items {
                    item.enabled = !model.config_owned.contains(&key);
                }
                if let Some(overlay) = self.overlay.take() {
                    self.overlay_stack.push(overlay);
                }
                self.menu(key, items);
            }
            _ => self.root_menu(model),
        }
    }

    fn run_action(&mut self, model: &Model, action: Action, intents: &mut Vec<UiIntent>) {
        match action {
            Action::Domain(intent) => {
                intents.push(UiIntent::Domain(intent));
                self.dismiss();
            }
            Action::NewSpace => self.open_create_space(),
            Action::RenameSpace(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form("Rename space", FormKind::RenameSpace(id), &space.name);
                }
            }
            Action::EditSpaceIcon(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form("Space icon", FormKind::SpaceIcon(id), &space.icon);
                }
            }
            Action::EditSpaceAccent(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form(
                        "Accent (#RRGGBB or empty)",
                        FormKind::SpaceAccent(id),
                        space.accent.as_deref().unwrap_or(""),
                    );
                }
            }
            Action::EditSpaceRules(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form(
                        "Routing rules (JSON)",
                        FormKind::SpaceRules(id),
                        &serde_json::to_string(&space.rules).unwrap_or_else(|_| "[]".into()),
                    );
                }
            }
            Action::RenameTab(id) => {
                if let Some(tab) = model.tabs.get(&id) {
                    self.open_form("Rename tab", FormKind::RenameTab(id), tab.display_title());
                }
            }
            Action::MoveTab(id) => {
                let items = model
                    .spaces
                    .iter()
                    .map(|space| {
                        let mut item = MenuItem::new(
                            space.id.clone(),
                            space.name.clone(),
                            Action::Domain(Intent::AssignTab {
                                id,
                                space_id: space.id.clone(),
                            }),
                        );
                        item.enabled = model
                            .tabs
                            .get(&id)
                            .is_some_and(|tab| tab.space_id != space.id);
                        item
                    })
                    .collect();
                self.push_menu("Move to space", items);
            }
            Action::DeleteSpace(id) => {
                if model.spaces.len() <= 1 {
                    return;
                }
                if model.tabs.values().any(|tab| tab.space_id == id) {
                    let items = model
                        .spaces
                        .iter()
                        .filter(|space| space.id != id)
                        .map(|space| {
                            MenuItem::new(
                                space.id.clone(),
                                format!("Move tabs to {}", space.name),
                                Action::Confirm {
                                    label: "Move tabs and delete space?".into(),
                                    action: Box::new(Action::Domain(Intent::DeleteSpace {
                                        id: id.clone(),
                                        destination: Some(space.id.clone()),
                                    })),
                                },
                            )
                        })
                        .collect();
                    self.push_menu("Choose destination", items);
                } else {
                    self.run_action(
                        model,
                        Action::Confirm {
                            label: "Delete empty space?".into(),
                            action: Box::new(Action::Domain(Intent::DeleteSpace {
                                id,
                                destination: None,
                            })),
                        },
                        intents,
                    );
                }
            }
            Action::Settings => self.open_settings(),
            Action::EditSetting(key) => self.edit_setting(model, &key, intents),
            Action::ResetSetting(key) => {
                if !model.config_owned.contains(&key) {
                    intents.push(UiIntent::Domain(Intent::ResetSetting(key)));
                    self.dirty = true;
                }
            }
            Action::Submenu { title, items } => self.push_menu(title, items),
            Action::Confirm { label, action } => self.push_menu(
                label,
                vec![
                    MenuItem::new("cancel", "Cancel", Action::Close),
                    MenuItem::new("confirm", "Confirm", *action),
                ],
            ),
            Action::Close => self.back(),
        }
    }
    fn push_menu(&mut self, title: impl Into<String>, items: Vec<MenuItem>) {
        if let Some(overlay) = self.overlay.take() {
            self.overlay_stack.push(overlay);
        }
        self.menu(title, items);
    }
    fn edit_setting(&mut self, model: &Model, key: &str, intents: &mut Vec<UiIntent>) {
        if model.config_owned.contains(key) {
            return;
        }
        let Some(descriptor) = settings::descriptor(key) else {
            return;
        };
        let value = model.settings.get(key).unwrap_or_default();
        match descriptor.kind {
            SettingKind::Bool => {
                intents.push(UiIntent::Domain(Intent::SetSetting {
                    key: key.into(),
                    value: Value::Bool(!value.as_bool().unwrap_or(false)),
                }));
                self.dirty = true;
            }
            SettingKind::Choice(choices) => {
                let items = choices
                    .iter()
                    .map(|choice| {
                        MenuItem::new(
                            *choice,
                            *choice,
                            Action::Domain(Intent::SetSetting {
                                key: key.into(),
                                value: Value::String((*choice).into()),
                            }),
                        )
                    })
                    .collect();
                self.push_menu(descriptor.label, items);
            }
            _ => {
                let value = value.as_str().map_or_else(
                    || {
                        if value.is_null() {
                            String::new()
                        } else {
                            value.to_string()
                        }
                    },
                    str::to_owned,
                );
                self.open_form(descriptor.label, FormKind::Setting(key.into()), &value);
            }
        }
    }
    fn submit_form(&mut self, model: &Model, intents: &mut Vec<UiIntent>) {
        let Some(Overlay::Form(form)) = &mut self.overlay else {
            return;
        };
        let value = form.editor.text().trim().to_owned();
        let intent: Result<Intent, String> = (|| {
            Ok(match &form.kind {
                FormKind::CreateSpace => {
                    valid_name(&value)?;
                    Intent::CreateSpace {
                        name: value.clone(),
                    }
                }
                FormKind::RenameSpace(id) => {
                    valid_name(&value)?;
                    Intent::RenameSpace {
                        id: id.clone(),
                        name: value.clone(),
                    }
                }
                FormKind::RenameTab(id) => {
                    if value.chars().count() > 512 {
                        return Err("Use at most 512 characters".into());
                    }
                    Intent::RenameTab {
                        id: *id,
                        title: value.clone(),
                    }
                }
                FormKind::SpaceIcon(id) | FormKind::SpaceAccent(id) | FormKind::SpaceRules(id) => {
                    let space = model
                        .spaces
                        .iter()
                        .find(|s| &s.id == id)
                        .ok_or("This space no longer exists")?;
                    let mut icon = space.icon.clone();
                    let mut accent = space.accent.clone();
                    let mut rules = space.rules.clone();
                    match &form.kind {
                        FormKind::SpaceIcon(_) => {
                            if value.chars().count() > 16 {
                                return Err("Use at most 16 characters".into());
                            }
                            icon = value.clone();
                        }
                        FormKind::SpaceAccent(_) => {
                            if !value.is_empty() && !settings::valid_color(&value) {
                                return Err("Use #RRGGBB or leave empty".into());
                            }
                            accent = (!value.is_empty()).then_some(value.clone());
                        }
                        FormKind::SpaceRules(_) => {
                            rules = serde_json::from_str(&value)
                                .map_err(|error| format!("Invalid rules: {error}"))?;
                        }
                        _ => {}
                    }
                    Intent::EditSpace {
                        id: id.clone(),
                        icon,
                        accent,
                        rules,
                    }
                }
                FormKind::Setting(key) => {
                    let descriptor =
                        settings::descriptor(key).ok_or("This setting no longer exists")?;
                    let value = match descriptor.kind {
                        SettingKind::Number { .. } | SettingKind::Object | SettingKind::List => {
                            serde_json::from_str(&value)
                                .map_err(|error| format!("Invalid value: {error}"))?
                        }
                        SettingKind::Text if value.is_empty() => Value::Null,
                        _ => Value::String(value.clone()),
                    };
                    settings::validate_value(key, &value)?;
                    Intent::SetSetting {
                        key: key.clone(),
                        value,
                    }
                }
            })
        })();
        match intent {
            Ok(intent) => {
                intents.push(UiIntent::Domain(intent));
                self.pending_form = Some(model.revision);
                self.dirty = true;
            }
            Err(error) => {
                if let Some(Overlay::Form(form)) = &mut self.overlay {
                    form.error = Some(error);
                }
                self.dirty = true;
            }
        }
    }
}

fn offset(current: usize, delta: i32, len: usize) -> usize {
    if delta >= 0 {
        current
            .saturating_add(delta as usize)
            .min(len.saturating_sub(1))
    } else {
        current.saturating_sub(delta.unsigned_abs() as usize)
    }
}
fn next_enabled(items: &[MenuItem], current: usize, delta: isize) -> usize {
    if items.is_empty() {
        return 0;
    }
    for step in 1..=items.len() {
        let i =
            (current as isize + delta * step as isize).rem_euclid(items.len() as isize) as usize;
        if items[i].enabled {
            return i;
        }
    }
    current.min(items.len() - 1)
}
fn valid_name(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().count() > 128 {
        Err("Use 1–128 printable characters".into())
    } else {
        Ok(())
    }
}
fn custom_menu(entries: &[settings::MenuEntry], prefix: &str) -> Vec<MenuItem> {
    entries
        .iter()
        .map(|entry| {
            let id = format!("{prefix}/{}", entry.id);
            let action = if entry.children.is_empty() {
                let action = Action::Domain(Intent::CustomAction(
                    entry.action.clone().unwrap_or_default(),
                ));
                if entry.confirm {
                    Action::Confirm {
                        label: entry.label.clone(),
                        action: Box::new(action),
                    }
                } else {
                    action
                }
            } else {
                Action::Submenu {
                    title: entry.label.clone(),
                    items: custom_menu(&entry.children, &id),
                }
            };
            let mut item = MenuItem::new(
                id,
                format!(
                    "{}{}",
                    entry.label,
                    if entry.children.is_empty() {
                        ""
                    } else {
                        " ›"
                    }
                ),
                action,
            );
            item.enabled = !entry.children.is_empty()
                || entry
                    .action
                    .as_ref()
                    .is_some_and(|action| !action.is_empty());
            item
        })
        .collect()
}

fn filter_menu(menu: &mut Menu) {
    let Some(search) = &menu.search else {
        return;
    };
    let query = search.editor.text().to_lowercase();
    let selected_id = menu.items.get(menu.selected).map(|item| item.id.clone());
    menu.items.clear();
    menu.items.extend(
        search
            .all_items
            .iter()
            .filter(|item| item.label.to_lowercase().contains(&query))
            .cloned(),
    );
    menu.selected = selected_id
        .and_then(|id| menu.items.iter().position(|item| item.id == id))
        .unwrap_or(0);
    menu.scroll = 0;
}

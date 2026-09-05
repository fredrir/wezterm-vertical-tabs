use crate::*;
use ratatui::layout::Position;
use serde_json::Value;
use std::time::Duration;
use vtabs_core::{Intent, Model, RailMode, SettingKind, settings};

impl SidebarUi {
    pub fn text_input_active(&self) -> bool {
        match &self.overlay {
            Some(Overlay::Form(_)) => self.focused == Some(ElementId::Editor),
            Some(Overlay::Menu(menu)) => {
                menu.search.is_some() || self.editor_menu_target().is_some()
            }
            None => {
                self.settings_page
                    && self.settings_search_focused
                    && self.focused == Some(ElementId::SettingsSearch)
            }
        }
    }

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
                    self.dragging = false;
                    self.pointer_origin = None;
                    self.show_tooltip = false;
                } else if (matches!(self.overlay, Some(Overlay::Form(_)))
                    && self.focused == Some(ElementId::Editor))
                    || (self.settings_page && self.settings_search_focused)
                {
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
                    self.drag = None;
                    self.dragging = false;
                    self.pointer_origin = None;
                    self.show_tooltip = false;
                } else {
                    self.dirty = true;
                    if (matches!(self.overlay, Some(Overlay::Form(_)))
                        && self.focused == Some(ElementId::Editor))
                        || (self.settings_page && self.settings_search_focused)
                    {
                        self.reset_caret();
                    }
                }
            }
            UiInput::PointerMove { x, y } => {
                if self.drag == Some(ElementId::Editor)
                    && let Some(Overlay::Menu(menu)) = &mut self.overlay
                    && let Some(search) = &mut menu.search
                {
                    search
                        .editor
                        .click_column(usize::from(x.saturating_sub(self.editor_rect.x)), true);
                    self.dirty = true;
                    return intents;
                }
                if self.drag == Some(ElementId::Editor)
                    && let Some(Overlay::Form(form)) = &mut self.overlay
                {
                    form.editor
                        .click_column(usize::from(x.saturating_sub(self.editor_rect.x)), true);
                    self.reset_caret();
                    self.dirty = true;
                    return intents;
                }
                if self
                    .pointer_origin
                    .is_some_and(|(px, py)| px.abs_diff(x).saturating_add(py.abs_diff(y)) > 1)
                    && self.drag.is_some()
                {
                    self.dragging = true;
                }
                if self.drag == Some(ElementId::SettingsSearch) && self.settings_page {
                    self.settings_query
                        .click_column(usize::from(x.saturating_sub(self.editor_rect.x)), true);
                    self.reset_caret();
                    self.dirty = true;
                    return intents;
                }
                let hit = self.hit_test(x, y).map(|hit| hit.id.clone());
                if hit != self.hovered {
                    self.hovered = hit;
                    self.show_tooltip = false;
                    self.tooltip_deadline = (self.hovered.is_some()
                        && self.overlay.is_none()
                        && self.window_focused
                        && !self.dragging)
                        .then_some(self.now + Duration::from_millis(600));
                    self.start_effect(model);
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
                    if let Some(target) = self.editor_menu_target() {
                        self.restore_editor(target);
                    } else {
                        self.dismiss();
                    }
                    return intents;
                }
                self.pointer_origin = Some((x, y));
                self.dragging = false;
                match (button, hit) {
                    (MouseButton::Right, Some(id)) => self.context_menu(model, id),
                    (MouseButton::Middle, Some(ElementId::Tab(id))) => {
                        self.close_tab(model, id, &mut intents)
                    }
                    (MouseButton::Left, Some(ElementId::SettingsSearch)) => {
                        self.settings_search_focused = true;
                        self.focused = Some(ElementId::SettingsSearch);
                        self.settings_query.click_column(
                            usize::from(x.saturating_sub(self.editor_rect.x)),
                            modifiers.shift,
                        );
                        self.drag = Some(ElementId::SettingsSearch);
                        self.reset_caret();
                        self.dirty = true;
                    }
                    (MouseButton::Left, Some(ElementId::Editor)) => {
                        if let Some(Overlay::Menu(menu)) = &mut self.overlay
                            && let Some(search) = &mut menu.search
                        {
                            search.editor.click_column(
                                usize::from(x.saturating_sub(self.editor_rect.x)),
                                modifiers.shift,
                            );
                        }
                        if let Some(Overlay::Form(form)) = &mut self.overlay {
                            form.editor.click_column(
                                usize::from(x.saturating_sub(self.editor_rect.x)),
                                modifiers.shift,
                            );
                        }
                        self.drag = Some(ElementId::Editor);
                        self.focused = Some(ElementId::Editor);
                        if matches!(self.overlay, Some(Overlay::Form(_))) {
                            self.reset_caret();
                        }
                        self.dirty = true;
                    }
                    (MouseButton::Left, Some(id)) => {
                        if matches!(self.overlay, Some(Overlay::Form(_))) {
                            self.caret_deadline = None;
                        }
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
                self.pointer_origin = None;
                self.dragging = false;
                self.dirty = true;
                let up = self.hit_test(x, y).map(|hit| hit.id.clone());
                match (down, up) {
                    (Some(from), Some(to)) if from == to => {
                        self.activate_element(model, to, &mut intents)
                    }
                    (Some(ElementId::Tab(id)), Some(ElementId::Tab(target))) => {
                        if let Some(tab) = model.tabs.get(&target) {
                            if model
                                .tabs
                                .get(&id)
                                .is_some_and(|from| from.folder_id != tab.folder_id)
                            {
                                intents.push(UiIntent::Domain(Intent::AssignFolder {
                                    tab_id: id,
                                    folder_id: tab.folder_id.clone(),
                                }));
                            }
                            if model
                                .tabs
                                .get(&id)
                                .is_some_and(|from| from.pinned != tab.pinned)
                            {
                                intents.push(UiIntent::Domain(Intent::PinTab {
                                    id,
                                    pinned: tab.pinned,
                                }));
                            }
                            if let Some(index) =
                                model.visible_ids().iter().position(|tab| *tab == target)
                            {
                                intents.push(UiIntent::Domain(Intent::MoveTab { id, index }));
                            }
                        }
                    }
                    (Some(ElementId::Tab(tab_id)), Some(ElementId::Folder(folder_id))) => {
                        intents.push(UiIntent::Domain(Intent::AssignFolder {
                            tab_id,
                            folder_id: Some(folder_id),
                        }));
                    }
                    (Some(ElementId::Tab(id)), Some(ElementId::NewTab)) => {
                        intents.push(UiIntent::Domain(Intent::AssignFolder {
                            tab_id: id,
                            folder_id: None,
                        }));
                        intents.push(UiIntent::Domain(Intent::PinTab { id, pinned: false }));
                    }
                    (Some(ElementId::Folder(id)), Some(ElementId::Folder(target))) => {
                        if let Some(index) = model.selected_folders().position(|f| f.id == target) {
                            intents.push(UiIntent::Domain(Intent::MoveFolder { id, index }));
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
                        Overlay::Form(_) => {}
                    }
                } else if self.settings_page && self.page_rect.contains(Position::new(x, y)) {
                    self.settings_scroll_by(model, rows);
                } else if self.spaces_rect.contains(Position::new(x, y)) {
                    self.space_scroll = offset(self.space_scroll, rows, model.spaces.len());
                } else {
                    self.tab_scroll = offset(self.tab_scroll, rows, self.sidebar_rows.len());
                }
                self.dirty = true;
            }
            UiInput::Text(text) | UiInput::Paste(text) | UiInput::ImeCommit(text) => {
                if let Some(Overlay::Form(form)) = &mut self.overlay
                    && self.focused == Some(ElementId::Editor)
                {
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
                } else if self.overlay.is_none()
                    && self.settings_page
                    && self.settings_search_focused
                    && self.focused == Some(ElementId::SettingsSearch)
                {
                    self.settings_input_text(&text);
                }
            }
            UiInput::ImePreedit { text, cursor } => {
                if let Some(Overlay::Form(form)) = &mut self.overlay
                    && self.focused == Some(ElementId::Editor)
                {
                    form.editor.set_preedit(&text, cursor);
                    self.reset_caret();
                    self.dirty = true;
                } else if let Some(Overlay::Menu(menu)) = &mut self.overlay
                    && let Some(search) = &mut menu.search
                {
                    search.editor.set_preedit(&text, cursor);
                    self.dirty = true;
                } else if self.overlay.is_none()
                    && self.settings_page
                    && self.settings_search_focused
                    && self.focused == Some(ElementId::SettingsSearch)
                {
                    self.settings_query.set_preedit(&text, cursor);
                    self.reset_caret();
                    self.dirty = true;
                }
            }
            UiInput::Key { key, modifiers } => self.key(model, key, modifiers, &mut intents),
        }
        intents
    }

    fn key(&mut self, model: &Model, key: Key, modifiers: Modifiers, intents: &mut Vec<UiIntent>) {
        if self.shortcut(model, &key, modifiers, intents) {
            return;
        }
        if let Some(target) = self.editor_menu_target() {
            if matches!(key, Key::Escape | Key::Left) {
                self.restore_editor(target);
                return;
            }
            if modifiers.command()
                && !modifiers.alt
                && matches!(
                    key,
                    Key::Character('a' | 'A' | 'c' | 'C' | 'x' | 'X' | 'v' | 'V')
                )
            {
                self.restore_editor(target);
            }
        } else if key == Key::F10 && self.text_input_active() {
            let target = if self.overlay.is_some() {
                ElementId::Editor
            } else {
                ElementId::SettingsSearch
            };
            self.context_menu(model, target);
            return;
        }
        if let Some(Overlay::Form(form)) = &mut self.overlay {
            if key == Key::Tab {
                let mut order: Vec<_> = [ElementId::Editor, ElementId::Submit, ElementId::Cancel]
                    .into_iter()
                    .filter(|id| self.hits.iter().any(|hit| &hit.id == id))
                    .collect();
                if order.is_empty() {
                    order.push(ElementId::Editor);
                }
                let at = self
                    .focused
                    .as_ref()
                    .and_then(|id| order.iter().position(|x| x == id))
                    .unwrap_or(0);
                let delta = if modifiers.shift { order.len() - 1 } else { 1 };
                self.focused = Some(order[(at + delta) % order.len()].clone());
                if self.focused == Some(ElementId::Editor) {
                    self.reset_caret();
                } else {
                    self.caret_deadline = None;
                }
                self.dirty = true;
                return;
            }
            if matches!(self.focused, Some(ElementId::Submit | ElementId::Cancel)) {
                if key == Key::Escape {
                    self.back();
                } else if matches!(key, Key::Enter | Key::Character(' '))
                    && !modifiers.command()
                    && !modifiers.alt
                {
                    if self.focused == Some(ElementId::Cancel) {
                        self.back();
                    } else {
                        self.submit_form(model, intents);
                    }
                }
                return;
            }
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
                && (matches!(
                    key,
                    Key::Character(_)
                        | Key::Backspace
                        | Key::Delete
                        | Key::Left
                        | Key::Right
                        | Key::Home
                        | Key::End
                ) || (key == Key::Escape && !search.editor.preedit().is_empty()))
            {
                let previous_query = search.editor.text().to_owned();
                let result = search.editor.key(&key, modifiers);
                let query_changed = search.editor.text() != previous_query;
                if query_changed {
                    filter_menu(menu);
                }
                match result {
                    EditResult::Changed => {
                        self.dirty = true;
                    }
                    EditResult::Copy(text) => {
                        intents.push(UiIntent::SetClipboard(text));
                        self.dirty |= query_changed;
                    }
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
        if self.settings_page
            && !matches!(
                self.focused,
                Some(
                    ElementId::Tab(_)
                        | ElementId::Space(_)
                        | ElementId::Folder(_)
                        | ElementId::NewTab
                        | ElementId::CreateSpace
                        | ElementId::CreateFolder
                        | ElementId::Rail
                        | ElementId::Refresh
                        | ElementId::Search
                        | ElementId::Settings
                )
            )
        {
            self.settings_key(model, key, modifiers, intents);
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
                Some(ElementId::Folder(id)) => {
                    self.run_action(model, Action::RenameFolder(id), intents)
                }
                _ => {}
            },
            Key::Up | Key::Down => {
                let delta = if key == Key::Up { -1 } else { 1 };
                if matches!(self.focused, Some(ElementId::Space(_))) {
                    self.navigate_space(model, delta, intents);
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
                self.tab_scroll = offset(self.tab_scroll, 10, self.sidebar_rows.len());
                self.dirty = true;
            }
            Key::Home => {
                self.tab_scroll = 0;
                self.dirty = true;
            }
            Key::End => {
                self.tab_scroll = self.sidebar_rows.len();
                self.dirty = true;
            }
            Key::Left | Key::Right => match self.focused.clone() {
                Some(ElementId::Folder(id)) => {
                    if let Some(folder) = model.folders.iter().find(|folder| folder.id == id)
                        && folder.collapsed == (key == Key::Right)
                    {
                        intents.push(UiIntent::Domain(Intent::ToggleFolder(id)));
                    }
                }
                Some(ElementId::Space(_)) => {
                    self.navigate_space(model, if key == Key::Left { -1 } else { 1 }, intents);
                }
                _ => intents.push(UiIntent::Domain(Intent::SetRail(if key == Key::Left {
                    RailMode::Collapsed
                } else {
                    RailMode::Expanded
                }))),
            },
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

    fn navigate_space(&mut self, model: &Model, delta: i32, intents: &mut Vec<UiIntent>) {
        let Some(ElementId::Space(id)) = &self.focused else {
            return;
        };
        let Some(at) = model.spaces.iter().position(|space| &space.id == id) else {
            return;
        };
        let index = offset(at, delta, model.spaces.len());
        if index == at {
            return;
        }
        let id = model.spaces[index].id.clone();
        self.focused = Some(ElementId::Space(id.clone()));
        intents.push(UiIntent::Domain(Intent::SelectSpace(id)));
        let slots = self
            .hits
            .iter()
            .filter(|hit| matches!(hit.id, ElementId::Space(_)))
            .count()
            .max(1);
        if index < self.space_scroll {
            self.space_scroll = index;
        } else if index >= self.space_scroll + slots {
            self.space_scroll = index + 1 - slots;
        }
        self.dirty = true;
    }

    pub(crate) fn ensure_tab_visible(&mut self, model: &Model, id: TabId) {
        let entries = Self::sidebar_entries(model);
        let target = if let Some(tab) = model.tabs.get(&id)
            && let Some(folder) = &tab.folder_id
            && model.folders.iter().any(|f| &f.id == folder && f.collapsed)
        {
            SidebarRow::Folder(folder.clone())
        } else {
            SidebarRow::Tab(id)
        };
        if let Some(at) = entries.iter().position(|row| *row == target) {
            let rows = usize::from(self.tabs_rect.height / self.row_height(model)).max(1);
            if at < self.tab_scroll {
                self.tab_scroll = at;
            } else if at >= self.tab_scroll + rows {
                self.tab_scroll = at + 1 - rows;
            }
        }
    }

    pub(crate) fn activate_element(
        &mut self,
        model: &Model,
        id: ElementId,
        intents: &mut Vec<UiIntent>,
    ) {
        match id {
            ElementId::Search => self.open_tab_navigator(model),
            ElementId::Refresh => intents.push(UiIntent::Refresh),
            ElementId::CreateFolder => self.open_create_folder(),
            ElementId::Folder(id) => intents.push(UiIntent::Domain(Intent::ToggleFolder(id))),
            ElementId::SettingsCategory(category) => self.settings_select_category(category),
            ElementId::SettingsSearch => {
                self.settings_search_focused = true;
                self.focused = Some(ElementId::SettingsSearch);
                self.reset_caret();
                self.dirty = true;
            }
            ElementId::CloseSettings => self.close_settings(),
            ElementId::ResetSettings => self.run_action(
                model,
                Action::Confirm {
                    label: "Reset saved settings?".into(),
                    action: Box::new(Action::Domain(Intent::ResetSettings)),
                },
                intents,
            ),
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
                if self.settings_page {
                    self.close_settings();
                }
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
            ElementId::Setting(key) => {
                self.settings_focus_setting(model, &key);
                self.edit_setting(model, &key, intents);
            }
            ElementId::Submit => self.submit_form(model, intents),
            ElementId::Cancel => self.back(),
            ElementId::Editor | ElementId::PrivateInfo => {}
        }
    }

    pub(crate) fn close_tab(&mut self, model: &Model, id: TabId, intents: &mut Vec<UiIntent>) {
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
        items.push(MenuItem::new("new-folder", "New folder", Action::NewFolder));
        items.extend(custom_menu(&model.settings.menus, "custom"));
        self.menu("Vertical tabs", items);
    }
    fn context_menu(&mut self, model: &Model, id: ElementId) {
        match id {
            ElementId::Editor | ElementId::SettingsSearch => {
                let editor = match (&self.overlay, &id) {
                    (Some(Overlay::Form(form)), ElementId::Editor) => Some(&form.editor),
                    (Some(Overlay::Menu(menu)), ElementId::Editor) => {
                        menu.search.as_ref().map(|search| &search.editor)
                    }
                    (None, ElementId::SettingsSearch) if self.settings_page => {
                        Some(&self.settings_query)
                    }
                    _ => None,
                };
                let Some(editor) = editor else {
                    return;
                };
                let has_selection = editor.selection().is_some();
                let has_text = !editor.text().is_empty();
                let items = [
                    ("cut", "Cut", 'x', has_selection),
                    ("copy", "Copy", 'c', has_selection),
                    ("paste", "Paste", 'v', true),
                    ("select-all", "Select all", 'a', has_text),
                ]
                .into_iter()
                .map(|(name, label, key, enabled)| {
                    let mut item = MenuItem::new(
                        name,
                        label,
                        Action::EditorCommand {
                            key: Key::Character(key),
                            target: id.clone(),
                        },
                    );
                    item.enabled = enabled;
                    item
                })
                .collect();
                self.focused = Some(id.clone());
                if id == ElementId::SettingsSearch {
                    self.settings_search_focused = true;
                }
                self.drag = None;
                self.pointer_origin = None;
                self.push_menu("Edit text", items);
            }
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
                items.insert(
                    4,
                    MenuItem::new("move-folder", "Move to folder ›", Action::MoveToFolder(id)),
                );
                let at = model
                    .visible_ids()
                    .iter()
                    .position(|t| *t == id)
                    .unwrap_or(0);
                items.push(MenuItem::new(
                    "move-up",
                    "Move up",
                    Action::Domain(Intent::MoveTab {
                        id,
                        index: at.saturating_sub(1),
                    }),
                ));
                items.push(MenuItem::new(
                    "move-down",
                    "Move down",
                    Action::Domain(Intent::MoveTab {
                        id,
                        index: at.saturating_add(1),
                    }),
                ));
                items.extend(custom_menu(&model.settings.menus, "custom"));
                self.menu(tab.display_title(), items);
            }
            ElementId::Folder(id) => {
                let Some(folder) = model.folders.iter().find(|f| f.id == id) else {
                    return;
                };
                let index = model
                    .selected_folders()
                    .position(|f| f.id == id)
                    .unwrap_or(0);
                self.menu(
                    &folder.name,
                    vec![
                        MenuItem::new(
                            "new-tab",
                            "New tab in folder",
                            Action::Domain(Intent::NewTabInFolder(id.clone())),
                        ),
                        MenuItem::new(
                            "toggle",
                            if folder.collapsed {
                                "Expand"
                            } else {
                                "Collapse"
                            },
                            Action::Domain(Intent::ToggleFolder(id.clone())),
                        ),
                        MenuItem::new("rename", "Rename folder", Action::RenameFolder(id.clone())),
                        MenuItem::new(
                            "up",
                            "Move up",
                            Action::Domain(Intent::MoveFolder {
                                id: id.clone(),
                                index: index.saturating_sub(1),
                            }),
                        ),
                        MenuItem::new(
                            "down",
                            "Move down",
                            Action::Domain(Intent::MoveFolder {
                                id: id.clone(),
                                index: index + 1,
                            }),
                        ),
                        MenuItem::new(
                            "ungroup",
                            "Ungroup tabs",
                            Action::Domain(Intent::DeleteFolder(id)),
                        ),
                    ],
                );
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
            _ if self.overlay.is_none() => self.root_menu(model),
            _ => {}
        }
    }

    fn editor_menu_target(&self) -> Option<ElementId> {
        match &self.overlay {
            Some(Overlay::Menu(menu)) => match &menu.items.first()?.action {
                Action::EditorCommand { target, .. } => Some(target.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn restore_editor(&mut self, target: ElementId) {
        self.back();
        self.focused = Some(target.clone());
        if target == ElementId::SettingsSearch {
            self.settings_search_focused = true;
        }
        self.drag = None;
        self.pointer_origin = None;
        self.reset_caret();
        self.dirty = true;
    }

    fn run_action(&mut self, model: &Model, action: Action, intents: &mut Vec<UiIntent>) {
        match action {
            Action::Domain(intent) => {
                intents.push(UiIntent::Domain(intent));
                self.dismiss();
            }
            Action::NewSpace => self.open_create_space(),
            Action::NewFolder => self.open_create_folder(),
            Action::RenameFolder(id) => {
                if let Some(folder) = model.folders.iter().find(|f| f.id == id) {
                    self.open_form("Rename folder", FormKind::RenameFolder(id), &folder.name);
                }
            }
            Action::MoveToFolder(tab_id) => {
                let mut items: Vec<_> = model
                    .selected_folders()
                    .map(|folder| {
                        MenuItem::new(
                            folder.id.clone(),
                            folder.name.clone(),
                            Action::Domain(Intent::AssignFolder {
                                tab_id,
                                folder_id: Some(folder.id.clone()),
                            }),
                        )
                    })
                    .collect();
                items.push(MenuItem::new(
                    "ungroup",
                    "Remove from folder",
                    Action::Domain(Intent::AssignFolder {
                        tab_id,
                        folder_id: None,
                    }),
                ));
                self.push_menu("Move to folder", items);
            }
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
            Action::EditorCommand { key, target } => {
                self.restore_editor(target);
                self.key(
                    model,
                    key,
                    Modifiers {
                        control: true,
                        ..Modifiers::default()
                    },
                    intents,
                );
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
    pub(crate) fn edit_setting(&mut self, model: &Model, key: &str, intents: &mut Vec<UiIntent>) {
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
                FormKind::CreateFolder => {
                    valid_name(&value)?;
                    Intent::CreateFolder {
                        name: value.clone(),
                    }
                }
                FormKind::RenameFolder(id) => {
                    valid_name(&value)?;
                    Intent::RenameFolder {
                        id: id.clone(),
                        name: value.clone(),
                    }
                }
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

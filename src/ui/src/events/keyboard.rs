use crate::SidebarUi;
use crate::actions::Action;
use crate::components::list;
use crate::element::ElementId;
use crate::events::EditorSlot;
use crate::input::{EditResult, Key, Modifiers};
use crate::intent::UiIntent;
use crate::overlays::{Menu, Overlay, next_enabled};
use vtabs_core::Model;

impl SidebarUi {
    pub(crate) fn key(
        &mut self,
        model: &Model,
        key: Key,
        modifiers: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) {
        if self.shortcut(model, &key, modifiers, intents) {
            return;
        }
        if self.overlays.current.is_none() && self.sidebar.rename.is_some() {
            match self.editor_key(EditorSlot::Rename, &key, modifiers, intents) {
                EditResult::Submit => self.finish_rename(true, intents),
                EditResult::Cancel => self.finish_rename(false, intents),
                _ => {}
            }
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
        } else if key == Key::F10 && self.launcher_actions(None) {
            return;
        } else if key == Key::F10 && self.text_input_active() {
            let target = if self.overlays.current.is_some() {
                ElementId::Editor
            } else {
                ElementId::SettingsSearch
            };
            self.anchor_focused(&target);
            self.context_menu(model, target);
            return;
        }
        if matches!(self.overlays.current, Some(Overlay::Form(_))) {
            if key == Key::Tab {
                let mut order: Vec<_> = [ElementId::Editor, ElementId::Submit, ElementId::Cancel]
                    .into_iter()
                    .filter(|id| self.paint.hit(id).is_some())
                    .collect();
                if order.is_empty() {
                    order.push(ElementId::Editor);
                }
                let current = self
                    .focused
                    .as_ref()
                    .filter(|id| order.contains(id))
                    .unwrap_or(&order[0]);
                self.focused = list::cycle(&order, Some(current), modifiers.shift);
                if self.focused == Some(ElementId::Editor) {
                    self.reset_caret();
                } else {
                    self.caret.stop();
                }
                self.frame.dirty = true;
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
            match self.editor_key(EditorSlot::Form, &key, modifiers, intents) {
                EditResult::Submit => self.submit_form(model, intents),
                EditResult::Cancel => self.back(),
                _ => {}
            }
            return;
        }
        let edits_query = matches!(
            &self.overlays.current,
            Some(Overlay::Menu(Menu { search: Some(search), .. }))
                if matches!(
                    key,
                    Key::Character(_)
                        | Key::Backspace
                        | Key::Delete
                        | Key::Left
                        | Key::Right
                        | Key::Home
                        | Key::End
                ) || (key == Key::Escape && !search.editor.preedit().is_empty())
        );
        if edits_query {
            self.editor_key(EditorSlot::Palette, &key, modifiers, intents);
            return;
        }
        if let Some(Overlay::Menu(menu)) = &mut self.overlays.current {
            match key {
                Key::Left | Key::Right if menu.message.is_some() => {
                    menu.selected = next_enabled(&menu.items, menu.selected, 1);
                    self.frame.dirty = true;
                }
                Key::Escape | Key::Left => self.back(),
                Key::Down | Key::Tab if !modifiers.shift => {
                    menu.selected = next_enabled(&menu.items, menu.selected, 1);
                    self.frame.dirty = true;
                }
                Key::Up | Key::Tab => {
                    menu.selected = next_enabled(&menu.items, menu.selected, -1);
                    self.frame.dirty = true;
                }
                Key::Home => {
                    menu.selected = menu.items.iter().position(|i| i.enabled).unwrap_or(0);
                    self.frame.dirty = true;
                }
                Key::End => {
                    menu.selected = menu.items.iter().rposition(|i| i.enabled).unwrap_or(0);
                    self.frame.dirty = true;
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
        if self.settings.open
            && self
                .focused
                .as_ref()
                .is_none_or(ElementId::on_settings_page)
        {
            self.settings_key(model, key, modifiers, intents);
            return;
        }
        match key {
            Key::Tab => {
                let ids: Vec<_> = self.paint.hits.iter().map(|hit| hit.id.clone()).collect();
                if let Some(id) = list::cycle(&ids, self.focused.as_ref(), modifiers.shift) {
                    self.focused = Some(id);
                    self.frame.dirty = true;
                }
            }
            Key::Enter | Key::Character(' ') => {
                if let Some(id) = self.focused.clone() {
                    self.activate_element(model, id, intents);
                }
            }
            Key::F10 => {
                if let Some(id) = self.focused.clone() {
                    self.anchor_focused(&id);
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
            Key::PageUp => {
                self.sidebar.scroll = self.sidebar.scroll.saturating_sub(10);
                self.frame.dirty = true;
            }
            Key::PageDown => {
                self.sidebar.scroll =
                    list::offset(self.sidebar.scroll, 10, self.sidebar.rows.len());
                self.frame.dirty = true;
            }
            Key::Home => {
                self.sidebar.scroll = 0;
                self.frame.dirty = true;
            }
            Key::End => {
                self.sidebar.scroll = self.sidebar.rows.len();
                self.frame.dirty = true;
            }
            Key::Delete => match self.focused.clone() {
                Some(ElementId::Tab(id)) => self.close_tab(model, id, intents),
                Some(ElementId::SettingsTab) => self.close_settings(),
                _ => {}
            },
            Key::Escape => {
                self.focused = None;
                self.pointer.cancel_drag();
                self.cancel_effects();
                self.frame.dirty = true;
            }
            Key::Character('+') => self.open_create_space(),
            _ => {}
        }
    }
}

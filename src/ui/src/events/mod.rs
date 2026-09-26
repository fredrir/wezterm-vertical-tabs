mod drag_drop;
mod keyboard;
mod pointer;

pub(crate) use drag_drop::{DropMotion, DropTarget};
pub(crate) use pointer::{Pointer, Press};

use crate::SidebarUi;
use crate::element::ElementId;
use crate::input::{EditResult, Key, Modifiers, MouseButton, TextEditor, UiInput};
use crate::intent::UiIntent;
use crate::overlays::Overlay;
use crate::views::launcher::filter_menu;
use vtabs_core::Model;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorSlot {
    Form,
    Palette,
    Rename,
    SettingsSearch,
}

impl EditorSlot {
    fn blinks(self) -> bool {
        self != Self::Palette
    }
    pub(crate) fn element(self) -> ElementId {
        match self {
            Self::SettingsSearch => ElementId::SettingsSearch,
            Self::Form | Self::Palette | Self::Rename => ElementId::Editor,
        }
    }
}

impl SidebarUi {
    pub fn event(&mut self, model: &Model, event: UiInput) -> Vec<UiIntent> {
        if self
            .overlays
            .pending_form
            .is_some_and(|revision| revision != model.revision)
        {
            self.dismiss();
        }
        let mut intents = Vec::new();
        match event {
            UiInput::Focus(focused) => {
                self.host.focused = focused;
                if !focused {
                    self.suspend();
                } else if self.caret_blinks() {
                    self.reset_caret();
                }
                self.frame.dirty = true;
            }
            UiInput::Visibility(visible) => {
                self.host.visible = visible;
                if !visible {
                    self.suspend();
                } else {
                    self.frame.dirty = true;
                    if self.caret_blinks() {
                        self.reset_caret();
                    }
                }
            }
            UiInput::PointerMove { x, y } => self.pointer_move(model, x, y),
            UiInput::PointerDown {
                x,
                y,
                button,
                modifiers,
            } => self.pointer_down(model, x, y, button, modifiers, &mut intents),
            UiInput::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            } => self.pointer_up(model, x, y, &mut intents),
            UiInput::PointerUp { .. } => {}
            UiInput::Scroll { x, y, rows } => self.scroll(model, x, y, rows),
            UiInput::Text(text) | UiInput::Paste(text) | UiInput::ImeCommit(text) => {
                if let Some(slot) = self.active_editor() {
                    if let Some(editor) = self.editor_mut(slot) {
                        editor.insert(&text);
                    }
                    self.edited(slot);
                }
            }
            UiInput::ImePreedit { text, cursor } => {
                if let Some(slot) = self.active_editor() {
                    if let Some(editor) = self.editor_mut(slot) {
                        editor.set_preedit(&text, cursor);
                    }
                    self.caret_moved(slot);
                }
            }
            UiInput::Key { key, modifiers } => self.key(model, key, modifiers, &mut intents),
        }
        intents
    }

    pub fn clipboard_failed(&mut self, message: &str) {
        if matches!(self.overlays.current, Some(Overlay::Form(_))) {
            self.set_error(message);
        } else {
            self.overlays.stash();
            self.show_error(message);
        }
    }

    pub fn text_input_active(&self) -> bool {
        self.active_editor().is_some() || self.editor_menu_target().is_some()
    }

    fn suspend(&mut self) {
        self.cancel_effects();
        self.caret.stop();
        self.tooltip.hide();
        self.pointer.cancel_drag();
        self.pointer.press = None;
        self.pointer.origin = None;
    }

    fn caret_blinks(&self) -> bool {
        self.active_editor().is_some_and(EditorSlot::blinks)
    }

    pub(crate) fn editor_key(
        &mut self,
        slot: EditorSlot,
        key: &Key,
        modifiers: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) -> EditResult {
        let Some(editor) = self.editor_mut(slot) else {
            return EditResult::Unhandled;
        };
        let before = editor.text().to_owned();
        let result = editor.key(key, modifiers);
        let changed = editor.text() != before;
        match &result {
            EditResult::Copy(text) => intents.push(UiIntent::SetClipboard(text.clone())),
            EditResult::Paste => intents.push(UiIntent::RequestClipboard),
            _ => {}
        }
        if changed {
            self.edited(slot);
        } else if result == EditResult::Changed {
            self.caret_moved(slot);
        }
        result
    }

    pub(crate) fn edited(&mut self, slot: EditorSlot) {
        match (slot, &mut self.overlays.current) {
            (EditorSlot::Form, Some(Overlay::Form(form))) => form.error = None,
            (EditorSlot::Palette, Some(Overlay::Menu(menu))) => filter_menu(menu),
            (EditorSlot::SettingsSearch, _) => {
                self.settings.selected = 0;
                self.settings.scroll = 0;
            }
            _ => {}
        }
        self.caret_moved(slot);
    }

    fn caret_moved(&mut self, slot: EditorSlot) {
        if slot.blinks() {
            self.reset_caret();
        }
        self.frame.dirty = true;
    }

    pub(crate) fn click_editor(&mut self, slot: EditorSlot, x: u16, select: bool) {
        let left = self
            .paint
            .field(&slot.element())
            .map_or(0, |field| field.rect.x);
        let column = usize::from(x.saturating_sub(left));
        if let Some(editor) = self.editor_mut(slot) {
            editor.click_column(column, select);
        }
        if slot.blinks() {
            self.reset_caret();
        }
        self.frame.dirty = true;
    }

    pub(crate) fn active_editor(&self) -> Option<EditorSlot> {
        match &self.overlays.current {
            Some(Overlay::Form(_)) => {
                (self.focused == Some(ElementId::Editor)).then_some(EditorSlot::Form)
            }
            Some(Overlay::Menu(menu)) => menu.search.as_ref().map(|_| EditorSlot::Palette),
            None if self.sidebar.rename.is_some() => Some(EditorSlot::Rename),
            None => (self.settings.open
                && self.settings.search_focused
                && self.focused == Some(ElementId::SettingsSearch))
            .then_some(EditorSlot::SettingsSearch),
        }
    }

    pub(crate) fn editor_slot(&self, id: &ElementId) -> Option<EditorSlot> {
        match (id, &self.overlays.current) {
            (ElementId::Editor, Some(Overlay::Form(_))) => Some(EditorSlot::Form),
            (ElementId::Editor, Some(Overlay::Menu(menu))) => {
                menu.search.as_ref().map(|_| EditorSlot::Palette)
            }
            (ElementId::Editor, None) => self.sidebar.rename.as_ref().map(|_| EditorSlot::Rename),
            (ElementId::SettingsSearch, None) => {
                self.settings.open.then_some(EditorSlot::SettingsSearch)
            }
            _ => None,
        }
    }

    pub(crate) fn editor(&self, slot: EditorSlot) -> Option<&TextEditor> {
        match (slot, &self.overlays.current) {
            (EditorSlot::Form, Some(Overlay::Form(form))) => Some(&form.editor),
            (EditorSlot::Palette, Some(Overlay::Menu(menu))) => {
                menu.search.as_ref().map(|search| &search.editor)
            }
            (EditorSlot::Rename, _) => self.sidebar.rename.as_ref().map(|rename| &rename.editor),
            (EditorSlot::SettingsSearch, _) => Some(&self.settings.query),
            _ => None,
        }
    }

    fn editor_mut(&mut self, slot: EditorSlot) -> Option<&mut TextEditor> {
        match (slot, &mut self.overlays.current) {
            (EditorSlot::Form, Some(Overlay::Form(form))) => Some(&mut form.editor),
            (EditorSlot::Palette, Some(Overlay::Menu(menu))) => {
                menu.search.as_mut().map(|search| &mut search.editor)
            }
            (EditorSlot::Rename, _) => self
                .sidebar
                .rename
                .as_mut()
                .map(|rename| &mut rename.editor),
            (EditorSlot::SettingsSearch, _) => Some(&mut self.settings.query),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/events.rs"]
mod tests;

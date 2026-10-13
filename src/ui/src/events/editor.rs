use crate::SidebarUi;
use crate::element::ElementId;
use crate::input::{EditResult, Key, Modifiers, TextEditor};
use crate::intent::UiIntent;
use crate::overlays::Overlay;
use crate::overlays::launcher::filter_menu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorSlot {
    Form,
    Palette,
    Rename,
    SettingsSearch,
    Entry,
}

impl EditorSlot {
    fn blinks(self) -> bool {
        self != Self::Palette
    }
    pub(crate) fn element(self) -> ElementId {
        match self {
            Self::SettingsSearch => ElementId::SettingsSearch,
            Self::Entry => ElementId::SettingsEntry,
            Self::Form | Self::Palette | Self::Rename => ElementId::Editor,
        }
    }
}

impl SidebarUi {
    pub(super) fn caret_blinks(&self) -> bool {
        self.active_editor().is_some_and(EditorSlot::blinks)
    }

    pub(super) fn insert_text(&mut self, text: &str) {
        if let Some(slot) = self.active_editor() {
            if let Some(editor) = self.editor_mut(slot) {
                editor.insert(text);
            }
            self.edited(slot);
        }
    }

    pub(super) fn set_preedit(&mut self, text: &str, cursor: Option<usize>) {
        if let Some(slot) = self.active_editor() {
            if let Some(editor) = self.editor_mut(slot) {
                editor.set_preedit(text, cursor);
            }
            self.caret_moved(slot);
        }
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
        match (slot, self.overlays.current_mut()) {
            (EditorSlot::Form, Some(Overlay::Form(form))) => {
                form.error = form
                    .color
                    .as_mut()
                    .and_then(|color| color.edited(form.editor.text()).err().map(str::to_owned));
            }
            (EditorSlot::Palette, Some(Overlay::Menu(menu))) => filter_menu(menu),
            (EditorSlot::SettingsSearch, _) => {
                self.settings.chord = None;
                self.settings.selected = 0;
                self.settings.scroll = 0;
            }
            (EditorSlot::Entry, _) => self.sanitize_entry(),
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
        match self.overlays.current() {
            Some(Overlay::Form(_)) => {
                (self.focused == Some(ElementId::Editor)).then_some(EditorSlot::Form)
            }
            Some(Overlay::Menu(menu)) => menu
                .search
                .as_ref()
                .filter(|search| search.edits_query())
                .map(|_| EditorSlot::Palette),
            None if self.sidebar.rename.is_some() => Some(EditorSlot::Rename),
            None if self.settings.open && self.settings.editing.has_entry() => {
                Some(EditorSlot::Entry)
            }
            None => (self.settings.open && self.focused == Some(ElementId::SettingsSearch))
                .then_some(EditorSlot::SettingsSearch),
        }
    }

    pub(crate) fn editor_slot(&self, id: &ElementId) -> Option<EditorSlot> {
        match (id, self.overlays.current()) {
            (ElementId::Editor, Some(Overlay::Form(_))) => Some(EditorSlot::Form),
            (ElementId::Editor, Some(Overlay::Menu(menu))) => menu
                .search
                .as_ref()
                .filter(|search| search.edits_query())
                .map(|_| EditorSlot::Palette),
            (ElementId::Editor, None) => self.sidebar.rename.as_ref().map(|_| EditorSlot::Rename),
            (ElementId::SettingsSearch, None) => {
                self.settings.open.then_some(EditorSlot::SettingsSearch)
            }
            (ElementId::SettingsEntry, None) => self
                .settings
                .editing
                .has_entry()
                .then_some(EditorSlot::Entry),
            _ => None,
        }
    }

    pub(crate) fn editor(&self, slot: EditorSlot) -> Option<&TextEditor> {
        match (slot, self.overlays.current()) {
            (EditorSlot::Form, Some(Overlay::Form(form))) => Some(&form.editor),
            (EditorSlot::Palette, Some(Overlay::Menu(menu))) => {
                menu.search.as_ref().map(|search| &search.editor)
            }
            (EditorSlot::Rename, _) => self.sidebar.rename.as_ref().map(|rename| &rename.editor),
            (EditorSlot::SettingsSearch, _) => Some(&self.settings.query),
            (EditorSlot::Entry, _) => self.settings.editing.editor(),
            _ => None,
        }
    }

    fn editor_mut(&mut self, slot: EditorSlot) -> Option<&mut TextEditor> {
        match (slot, self.overlays.current_mut()) {
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
            (EditorSlot::Entry, _) => self.settings.editing.editor_mut(),
            _ => None,
        }
    }
}

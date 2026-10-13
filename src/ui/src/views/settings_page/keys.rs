use super::catalog::{CATEGORIES, choice_label};
use super::range::Range;
use crate::SidebarUi;
use crate::actions::Action;
use crate::components::list;
use crate::element::ElementId;
use crate::events::EditorSlot;
use crate::input::{EditResult, Key, Modifiers, TextEditor};
use crate::intent::UiIntent;
use crate::overlays::{FormKind, MenuItem, Overlay};
use crate::settings::{SettingTarget, managed_in_lua};
use serde_json::Value;
use vtabs_core::{Intent, Model, SettingKind, Settings};

impl SidebarUi {
    pub fn chord_label(&self) -> fn(&str) -> String {
        self.settings.chord_label
    }

    pub fn set_chord_label(&mut self, label: fn(&str) -> String) {
        self.settings.chord_label = label;
        self.frame.dirty = true;
    }

    pub(crate) fn settings_select_category(&mut self, category: String) {
        if !CATEGORIES.iter().any(|(id, ..)| *id == category) {
            return;
        }
        if self.settings.chord.take().is_some() {
            self.settings.query = TextEditor::default();
        }
        self.settings.category = category.clone();
        self.settings.selected = 0;
        self.settings.scroll = 0;
        self.focused = Some(ElementId::SettingsCategory(category));
        self.caret.stop();
        self.frame.dirty = true;
    }

    fn settings_input_text(&mut self, text: &str) {
        self.focused = Some(ElementId::SettingsSearch);
        self.settings.query.insert(text);
        self.edited(EditorSlot::SettingsSearch);
    }

    pub(crate) fn settings_focus_setting(&mut self, model: &Model, key: &str) {
        if let Some(index) = self
            .settings
            .fields(model)
            .iter()
            .position(|field| field.key == key)
        {
            self.settings.selected = index;
            self.focused = Some(ElementId::Setting(key.into()));
            if !self.settings.editing.has_entry() {
                self.caret.stop();
            }
            self.frame.dirty = true;
        }
    }

    pub(crate) fn settings_scroll_by(&mut self, rows: i32) {
        self.settings.scroll =
            list::scroll_by(self.settings.scroll, rows, self.settings.max_scroll);
        self.frame.dirty = true;
    }

    pub(crate) fn toggle_settings_recording(&mut self, intents: &mut Vec<UiIntent>) {
        self.commit_entry(intents);
        self.settings.editing.toggle_recording();
        self.focused = Some(ElementId::SettingsRecord);
        self.caret.stop();
        self.frame.dirty = true;
    }

    /// A chord recorded from the search field lists the actions bound to it.
    pub(crate) fn settings_recorded(&mut self, chord: &str) {
        self.frame.dirty = true;
        if chord.is_empty() {
            self.settings.editing.stop_recording();
            return;
        }
        self.settings.chord = Some(chord.into());
        self.settings.query = TextEditor::new((self.settings.chord_label)(chord));
        self.settings.selected = 0;
        self.settings.scroll = 0;
    }

    pub(crate) fn start_entry(&mut self, model: &Model, key: &str, typed: Option<char>) {
        if managed_in_lua(model, key) {
            return;
        }
        self.settings_focus_setting(model, key);
        if self.settings.editing.begin_entry(model, key, typed) {
            self.reset_caret();
            self.frame.dirty = true;
        }
    }

    pub(crate) fn sanitize_entry(&mut self) {
        self.settings.editing.sanitize_entry();
    }

    pub(crate) fn commit_entry(&mut self, intents: &mut Vec<UiIntent>) {
        if !self.settings.editing.has_entry() {
            return;
        }
        if let Some(intent) = self.settings.editing.commit_entry() {
            intents.push(UiIntent::Domain(intent));
        }
        self.caret.stop();
        self.frame.dirty = true;
    }

    /// Steps the value being typed, like a number field; Enter still applies it.
    fn step_entry(&mut self, model: &Model, steps: i32) {
        if self.settings.editing.step_entry(model, steps) {
            self.reset_caret();
            self.frame.dirty = true;
        }
    }

    /// Switches like the tab row; focus stays in the list when it was there.
    fn settings_switch_category(&mut self, model: &Model, delta: isize) {
        let in_list = !matches!(self.focused, Some(ElementId::SettingsCategory(_)));
        let at = CATEGORIES
            .iter()
            .position(|(id, ..)| *id == self.settings.category)
            .unwrap_or(0) as isize;
        let next = (at + delta).rem_euclid(CATEGORIES.len() as isize) as usize;
        self.settings_select_category(CATEGORIES[next].0.into());
        if in_list && let Some(field) = self.settings.fields(model).first() {
            self.settings_focus_setting(model, field.key);
        }
    }

    pub(crate) fn settings_key(
        &mut self,
        model: &Model,
        key: Key,
        mods: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) {
        if self.settings.editing.has_entry() {
            match key {
                Key::Escape => {
                    self.settings.editing.cancel_entry();
                    self.caret.stop();
                    self.frame.dirty = true;
                    return;
                }
                Key::Enter => {
                    self.commit_entry(intents);
                    return;
                }
                Key::Up | Key::Down => {
                    let steps = if mods.shift { 10 } else { 1 };
                    self.step_entry(model, if key == Key::Up { steps } else { -steps });
                    return;
                }
                Key::Tab | Key::PageUp | Key::PageDown => self.commit_entry(intents),
                _ => {
                    self.editor_key(EditorSlot::Entry, &key, mods, intents);
                    return;
                }
            }
        }
        if mods.command() && matches!(key, Key::Character('f' | 'F')) {
            self.focused = Some(ElementId::SettingsSearch);
            self.settings.query.select_all();
            self.reset_caret();
            self.frame.dirty = true;
            return;
        }
        if key == Key::Tab {
            let ids: Vec<_> = self
                .paint
                .hits
                .iter()
                .filter(|hit| {
                    self.settings.rect.contains(hit.rect.as_position())
                        && matches!(
                            hit.id,
                            ElementId::SettingsSearch
                                | ElementId::SettingsRecord
                                | ElementId::SettingsCategory(_)
                                | ElementId::Setting(_)
                                | ElementId::ResetSettings
                        )
                })
                .map(|hit| hit.id.clone())
                .collect();
            if let Some(id) = list::cycle(&ids, self.focused.as_ref(), mods.shift) {
                let search = id == ElementId::SettingsSearch;
                if let ElementId::Setting(key) = &id {
                    self.settings_focus_setting(model, key);
                }
                self.focused = Some(id);
                if search {
                    self.reset_caret();
                } else {
                    self.caret.stop();
                }
                self.frame.dirty = true;
            }
            return;
        }
        let search = self.focused == Some(ElementId::SettingsSearch);
        if search && !matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
            match self.editor_key(EditorSlot::SettingsSearch, &key, mods, intents) {
                EditResult::Submit => {
                    if let Some(field) = self.settings.fields(model).first() {
                        self.settings_focus_setting(model, field.key);
                    }
                }
                EditResult::Cancel => {
                    self.focused = None;
                    self.caret.stop();
                    self.frame.dirty = true;
                }
                _ => {}
            }
            return;
        }
        if matches!(key, Key::Left | Key::Right) && !mods.command() && !mods.alt {
            self.settings_switch_category(model, if key == Key::Left { -1 } else { 1 });
            return;
        }
        let fields = self.settings.fields(model);
        let last = fields.len().saturating_sub(1);
        if let Some(ElementId::SettingsCategory(category)) = &self.focused {
            match key {
                Key::Down | Key::Up if !fields.is_empty() => {
                    let at = if key == Key::Down { 0 } else { last };
                    self.settings_focus_setting(model, fields[at].key);
                }
                Key::Enter | Key::Character(' ') => self.activate_element(
                    model,
                    ElementId::SettingsCategory(category.clone()),
                    intents,
                ),
                Key::Character(character) if !mods.command() && !mods.alt => {
                    self.settings_input_text(&character.to_string())
                }
                _ => {}
            }
            return;
        }
        let field = fields.get(self.settings.selected).copied().filter(|field| {
            self.focused
                .as_ref()
                .is_none_or(|focused| focused.setting() == Some(field.key))
        });
        let ranged = field.is_some_and(|field| Range::of(&field.kind).is_some());
        match key {
            Key::Escape => {
                self.focused = None;
                self.frame.dirty = true;
            }
            Key::Up if search => {}
            Key::Up if field.is_some() && self.settings.selected == 0 => {
                self.settings_focus_setting(model, fields[last].key)
            }
            Key::Down if field.is_some() && self.settings.selected == last => {
                self.settings_focus_setting(model, fields[0].key)
            }
            Key::Down | Key::Up | Key::PageDown | Key::PageUp | Key::Home | Key::End => {
                self.settings.selected = match key {
                    Key::Home => 0,
                    Key::End => last,
                    Key::Down if search => 0,
                    Key::Down => (self.settings.selected + 1).min(last),
                    Key::Up => self.settings.selected.saturating_sub(1),
                    Key::PageDown => (self.settings.selected + 5).min(last),
                    Key::PageUp => self.settings.selected.saturating_sub(5),
                    _ => self.settings.selected,
                };
                if let Some(field) = fields.get(self.settings.selected) {
                    self.settings_focus_setting(model, field.key);
                }
                self.frame.dirty = true;
            }
            Key::Delete | Key::Backspace => {
                if let Some(field) = field {
                    self.reset_setting(model, field.key, intents);
                }
            }
            Key::Enter | Key::Character(' ') => {
                if let Some(
                    id @ (ElementId::ResetSettings
                    | ElementId::SettingsRecord
                    | ElementId::SettingsSearch),
                ) = self.focused.clone()
                {
                    self.activate_element(model, id, intents);
                } else if let Some(field) = field {
                    self.edit_setting(model, field.key, intents);
                }
            }
            Key::Character(digit) if ranged && digit.is_ascii_digit() && !mods.command() => {
                self.start_entry(model, field.unwrap().key, Some(digit));
            }
            Key::Character(character) if !mods.command() && !mods.alt => {
                self.settings_input_text(&character.to_string())
            }
            _ => {}
        }
    }

    pub(crate) fn edit_setting(&mut self, model: &Model, key: &str, intents: &mut Vec<UiIntent>) {
        if self.overlays.is_open() {
            self.dismiss();
        }
        let target = SettingTarget::of(key);
        if let SettingTarget::Shortcut(action) = target {
            self.open_keybinds(model, action);
            return;
        }
        if managed_in_lua(model, key) {
            return;
        }
        let Some(descriptor) = target.descriptor() else {
            return;
        };
        let value = target.value(&model.settings);
        match descriptor.kind {
            SettingKind::Bool => {
                intents.push(UiIntent::Domain(Intent::SetSetting {
                    key: key.into(),
                    value: Value::Bool(!value.as_bool().unwrap_or(false)),
                }));
                self.frame.dirty = true;
            }
            SettingKind::Choice(choices) => {
                let items = choices
                    .iter()
                    .map(|choice| {
                        MenuItem::new(
                            *choice,
                            choice_label(choice),
                            Action::Domain(Intent::SetSetting {
                                key: key.into(),
                                value: Value::String((*choice).into()),
                            }),
                        )
                    })
                    .collect();
                let anchor = self
                    .settings
                    .anchors
                    .iter()
                    .find(|(anchored, _)| anchored == key)
                    .map(|(_, rect)| *rect);
                self.push_menu(descriptor.label, items);
                if let Some(Overlay::Menu(menu)) = self.overlays.current_mut() {
                    menu.checked = choices
                        .iter()
                        .position(|choice| value.as_str() == Some(*choice));
                    menu.selected = menu.checked.unwrap_or(0);
                    menu.dropdown = anchor;
                }
            }
            SettingKind::Number { .. }
            | SettingKind::Fraction
            | SettingKind::Opacity
            | SettingKind::Scale => {
                self.start_entry(model, key, None);
            }
            SettingKind::Color | SettingKind::OptionalColor | SettingKind::Border => {
                let default = target.value(&Settings::default());
                let fallback = match key {
                    "quick_terminal_background_color" => {
                        self.host.page_background.unwrap_or(self.theme.background)
                    }
                    "quick_terminal_border_color" => self.theme.foreground,
                    "hover_background" => self.theme.hovered(),
                    "tab_foreground" | "local_glyph_color" => self.theme.tab_text(),
                    "selected_foreground" => self.theme.selected_text(),
                    _ => self.theme.accent,
                };
                self.open_color_form(
                    self.settings.label(descriptor),
                    FormKind::Setting(key.into()),
                    value.as_str().unwrap_or(""),
                    default.as_str(),
                    fallback,
                );
                if matches!(descriptor.kind, SettingKind::Border) {
                    self.allow_no_color();
                }
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
}

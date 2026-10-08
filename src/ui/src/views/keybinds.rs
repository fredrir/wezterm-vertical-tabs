use crate::SidebarUi;
use crate::actions::Action;
use crate::input::TextEditor;
use crate::intent::UiIntent;
use crate::overlays::{MenuItem, Overlay};
use crate::views::launcher::LauncherKind;
use std::collections::BTreeMap;
use vtabs_core::{Intent, Model, keybinds};

impl SidebarUi {
    pub(crate) fn edit_keybind(&mut self, model: &Model, action: &str) {
        if self.settings.managed_in_lua(model, "keybinds") {
            return;
        }
        let Some(field) = keybinds::DESCRIPTORS
            .iter()
            .find(|f| f.key.strip_prefix("shortcut.") == Some(action))
        else {
            return;
        };
        let mut items = Vec::new();
        for (index, chord) in keybinds::bindings(&model.settings, action)
            .iter()
            .enumerate()
        {
            items.push(MenuItem::new(
                format!("binding/{index}"),
                crate::keybinds::display_chord(chord),
                Action::Submenu {
                    title: crate::keybinds::display_chord(chord),
                    items: vec![
                        MenuItem::new(
                            "change",
                            "Record replacement",
                            Action::RecordKeybind(action.into(), Some(index)),
                        ),
                        MenuItem::new(
                            "remove",
                            "Remove shortcut",
                            Action::RemoveKeybind(action.into(), chord.clone()),
                        ),
                    ],
                },
            ));
        }
        items.push(MenuItem::new(
            "add",
            "Add shortcut",
            Action::RecordKeybind(action.into(), None),
        ));
        items.push(MenuItem::new(
            "reset",
            "Restore default shortcuts",
            Action::ResetSetting(field.key.into()),
        ));
        self.push_menu(field.label, items);
    }

    pub(crate) fn remove_keybind(
        &mut self,
        model: &Model,
        action: &str,
        chord: &str,
        intents: &mut Vec<UiIntent>,
    ) {
        if self.settings.managed_in_lua(model, "keybinds") {
            return;
        }
        if let Some(index) = keybinds::bindings(&model.settings, action)
            .iter()
            .position(|key| key == chord)
        {
            intents.push(UiIntent::Domain(set_keybinds(changed_keybinds(
                model,
                action,
                Some(index),
                None,
            ))));
        }
        self.dismiss();
        self.frame.dirty = true;
    }

    pub(crate) fn reset_setting(&mut self, model: &Model, key: &str, intents: &mut Vec<UiIntent>) {
        if self.settings.managed_in_lua(model, key) {
            return;
        }
        if let Some(action) = key.strip_prefix("shortcut.") {
            let mut bindings = model.settings.keybinds.clone();
            bindings.remove(action);
            if let Err(error) = keybinds::validate(&bindings) {
                self.push_menu(error, vec![MenuItem::new("back", "Back", Action::Close)]);
                return;
            }
            intents.push(UiIntent::Domain(set_keybinds(bindings)));
            self.dismiss();
        } else {
            intents.push(UiIntent::Domain(Intent::ResetSetting(key.into())));
        }
        self.frame.dirty = true;
    }

    pub(crate) fn open_keybind_recorder(&mut self, action: &str, index: Option<usize>) {
        self.open_launcher(
            LauncherKind::Keybind,
            "Record shortcut",
            "",
            vec![
                MenuItem::new(
                    "save-keybind",
                    "Save shortcut",
                    Action::SaveKeybind(action.into(), index),
                ),
                MenuItem::new("cancel-keybind", "Cancel", Action::Close),
            ],
            0,
        );
        self.toggle_shortcut_recording();
    }

    pub(crate) fn save_keybind(
        &mut self,
        model: &Model,
        action: &str,
        index: Option<usize>,
        intents: &mut Vec<UiIntent>,
    ) {
        if self.settings.managed_in_lua(model, "keybinds") {
            return;
        }
        let Some(Overlay::Menu(menu)) = &mut self.overlays.current else {
            return;
        };
        let Some(search) = &mut menu.search else {
            return;
        };
        let chord = search.editor.text().to_owned();
        let bindings = changed_keybinds(model, action, index, Some(chord));
        if let Err(error) = keybinds::validate(&bindings) {
            menu.title = error;
            search.editor = TextEditor::default();
            search.recording = false;
            crate::views::launcher::filter_menu(menu);
            self.frame.dirty = true;
            return;
        }
        intents.push(UiIntent::Domain(set_keybinds(bindings)));
        self.dismiss();
        self.frame.dirty = true;
    }
}

fn changed_keybinds(
    model: &Model,
    action: &str,
    index: Option<usize>,
    chord: Option<String>,
) -> BTreeMap<String, Vec<String>> {
    let mut overrides = model.settings.keybinds.clone();
    let mut keys = keybinds::bindings(&model.settings, action);
    if let Some(index) = index.filter(|index| *index < keys.len()) {
        keys.remove(index);
    }
    if let Some(chord) = chord {
        keys.insert(index.unwrap_or(keys.len()).min(keys.len()), chord);
    }
    overrides.insert(action.into(), keys);
    overrides
}

fn set_keybinds(bindings: BTreeMap<String, Vec<String>>) -> Intent {
    Intent::SetSetting {
        key: "keybinds".into(),
        value: serde_json::json!(bindings),
    }
}

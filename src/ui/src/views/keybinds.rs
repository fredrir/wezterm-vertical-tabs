use crate::SidebarUi;
use crate::actions::Action;
use crate::intent::UiIntent;
use crate::overlays::MenuItem;
use std::collections::BTreeMap;
use vtabs_core::{Intent, Model, keybinds};

impl SidebarUi {
    /// Removes by chord, since another window may have reordered the bindings.
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
        let keys = keybinds::bindings(&model.settings, action);
        if let Some(index) = keys.iter().position(|key| key == chord) {
            intents.push(UiIntent::Domain(set_keybinds(changed_keybinds(
                model,
                action,
                Some(index),
                None,
            ))));
            self.settings.chip = Some(index.min(keys.len() - 1));
        }
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
}

pub(crate) fn changed_keybinds(
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

pub(crate) fn set_keybinds(bindings: BTreeMap<String, Vec<String>>) -> Intent {
    Intent::SetSetting {
        key: "keybinds".into(),
        value: serde_json::json!(bindings),
    }
}

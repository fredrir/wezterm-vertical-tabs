use crate::SidebarUi;
use crate::actions::Action;
use crate::icons;
use crate::intent::UiIntent;
use crate::overlays::{MenuItem, Overlay, next_enabled};
use crate::views::launcher::{LauncherKind, filter_menu};
use crate::views::settings_page::{set_distro_color, set_padding_side};
use std::collections::BTreeMap;
use vtabs_core::{Intent, Model, distros, keybinds, padding};

/// The plugin action a keybind launcher edits.
#[derive(Clone, Debug)]
pub(crate) struct Binding {
    pub action: String,
    pub replacing: Option<usize>,
    pub rejected: Option<(String, String)>,
    pub select: Option<String>,
}

fn label(action: &str) -> &'static str {
    keybinds::DESCRIPTORS
        .iter()
        .find(|field| field.key.strip_prefix("shortcut.") == Some(action))
        .map_or("", |field| field.label)
}

fn owner(model: &Model, chord: &str) -> Option<&'static str> {
    keybinds::DESCRIPTORS.iter().find_map(|field| {
        let action = field.key.strip_prefix("shortcut.")?;
        keybinds::bindings(&model.settings, action)
            .iter()
            .any(|key| key == chord)
            .then_some(field.label)
    })
}

impl SidebarUi {
    pub(crate) fn binding(&self) -> Option<&Binding> {
        match &self.overlays.current {
            Some(Overlay::Menu(menu)) => menu.search.as_ref()?.binding.as_deref(),
            _ => None,
        }
    }

    fn binding_mut(&mut self) -> Option<&mut Binding> {
        match &mut self.overlays.current {
            Some(Overlay::Menu(menu)) => menu.search.as_mut()?.binding.as_deref_mut(),
            _ => None,
        }
    }

    pub(crate) fn open_keybinds(&mut self, model: &Model, action: &str) {
        if self.settings.managed_in_lua(model, "keybinds") {
            return;
        }
        let row = self.focused.clone();
        self.open_launcher(LauncherKind::Keybinds, label(action), "", Vec::new(), 0);
        self.overlays.restore_focus = row;
        if let Some(Overlay::Menu(menu)) = &mut self.overlays.current
            && let Some(search) = &mut menu.search
        {
            search.recording = true;
            search.binding = Some(Box::new(Binding {
                action: action.into(),
                replacing: None,
                rejected: None,
                select: None,
            }));
        }
        self.refresh_keybinds(model);
    }

    /// Rebuilds the launcher rows from the current bindings, keeping the selection.
    pub(crate) fn refresh_keybinds(&mut self, model: &Model) {
        let label = self.settings.chord_label;
        let danger = self.theme.danger;
        let accent = self.theme.accent;
        let Some(Overlay::Menu(menu)) = &mut self.overlays.current else {
            return;
        };
        let Some(search) = &mut menu.search else {
            return;
        };
        let Some(binding) = &mut search.binding else {
            return;
        };
        let chords = keybinds::bindings(&model.settings, &binding.action);
        let mut items = Vec::new();
        if let Some((chord, reason)) = &binding.rejected {
            let mut item = MenuItem::new("rejected", label(chord), Action::Close);
            item.icon = icons::ALERT;
            item.icon_color = Some(danger);
            item.hint = reason.clone();
            item.enabled = false;
            items.push(item);
        }
        for (index, chord) in chords.iter().enumerate() {
            let mut item = MenuItem::new(
                format!("chord/{chord}"),
                label(chord),
                Action::ReplaceKeybind(index),
            );
            if binding.replacing == Some(index) {
                item.icon = "●";
                item.icon_color = Some(accent);
            }
            item.remove = Some(Action::RemoveKeybind(binding.action.clone(), chord.clone()));
            items.push(item);
        }
        let mut reset = MenuItem::new(
            "reset",
            "Reset to default",
            Action::ResetSetting(format!("shortcut.{}", binding.action)),
        );
        reset.icon = icons::RESTORE;
        reset.enabled = chords != keybinds::defaults(&binding.action);
        items.push(reset);
        // A recorded chord is selected once the saved bindings list it.
        let recorded = binding.select.as_ref().and_then(|chord| {
            let id = format!("chord/{chord}");
            items.iter().position(|item| item.id == id)
        });
        if recorded.is_some() {
            binding.select = None;
        }
        let previous = menu.items.get(menu.selected).map(|item| item.id.clone());
        search.all_items = items;
        filter_menu(menu);
        menu.selected = recorded
            .or_else(|| {
                let previous = previous?;
                menu.items
                    .iter()
                    .position(|item| item.id == previous && item.enabled)
            })
            .unwrap_or_else(|| next_enabled(&menu.items, menu.items.len().saturating_sub(1), 1));
        self.frame.dirty = true;
    }

    /// Plain navigation keys can never be bindings, so they steer the list while recording.
    pub(crate) fn keybind_recorded(
        &mut self,
        model: &Model,
        chord: &str,
        intents: &mut Vec<UiIntent>,
    ) {
        let Some(binding) = self.binding().cloned() else {
            return;
        };
        let step = |ui: &mut Self, delta| {
            if let Some(Overlay::Menu(menu)) = &mut ui.overlays.current {
                menu.selected = next_enabled(&menu.items, menu.selected, delta);
            }
            ui.frame.dirty = true;
        };
        match chord {
            "" if binding.replacing.is_some() || binding.rejected.is_some() => {
                if let Some(binding) = self.binding_mut() {
                    binding.replacing = None;
                    binding.rejected = None;
                }
                self.refresh_keybinds(model);
            }
            "" => self.back(),
            "Up" | "Shift+Tab" => step(self, -1),
            "Down" | "Tab" => step(self, 1),
            "Enter" | "Space" => {
                let action = match &self.overlays.current {
                    Some(Overlay::Menu(menu)) => menu
                        .items
                        .get(menu.selected)
                        .filter(|item| item.enabled)
                        .map(|item| item.action.clone()),
                    _ => None,
                };
                if let Some(action) = action {
                    self.run_action(model, action, intents);
                }
            }
            "Delete" | "Backspace" => {
                let remove = match &self.overlays.current {
                    Some(Overlay::Menu(menu)) => menu
                        .items
                        .get(menu.selected)
                        .and_then(|item| item.remove.clone()),
                    _ => None,
                };
                if let Some(remove) = remove {
                    self.run_action(model, remove, intents);
                }
            }
            chord => {
                let bindings = changed_keybinds(
                    model,
                    &binding.action,
                    binding.replacing,
                    Some(chord.into()),
                );
                let rejected = match keybinds::validate(&bindings) {
                    Ok(()) => {
                        intents.push(UiIntent::Domain(set_keybinds(bindings)));
                        None
                    }
                    Err(_) => Some((
                        chord.to_owned(),
                        owner(model, chord).unwrap_or("Needs a modifier").to_owned(),
                    )),
                };
                if let Some(binding) = self.binding_mut() {
                    binding.select = rejected.is_none().then(|| chord.to_owned());
                    binding.replacing = None;
                    binding.rejected = rejected;
                }
                self.refresh_keybinds(model);
            }
        }
    }

    pub(crate) fn replace_keybind(&mut self, model: &Model, index: usize) {
        if let Some(binding) = self.binding_mut() {
            binding.replacing = Some(index);
            binding.rejected = None;
        }
        self.refresh_keybinds(model);
    }

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
        }
        if let Some(binding) = self.binding_mut() {
            binding.replacing = None;
            binding.rejected = None;
        }
        self.frame.dirty = true;
    }

    pub(crate) fn reset_setting(&mut self, model: &Model, key: &str, intents: &mut Vec<UiIntent>) {
        if self.settings.managed_in_lua(model, key) {
            return;
        }
        let editing = self.binding().is_some();
        if let Some(action) = key.strip_prefix("shortcut.") {
            let mut bindings = model.settings.keybinds.clone();
            bindings.remove(action);
            if keybinds::validate(&bindings).is_err() {
                // The clash shows where it can be resolved, in this action's launcher.
                if !editing {
                    self.open_keybinds(model, action);
                }
                let clash = keybinds::defaults(action)
                    .into_iter()
                    .find_map(|chord| Some((chord.clone(), owner(model, &chord)?.to_owned())));
                if let Some(binding) = self.binding_mut() {
                    binding.rejected = clash;
                }
                self.refresh_keybinds(model);
                return;
            }
            intents.push(UiIntent::Domain(set_keybinds(bindings)));
        } else if let Some(id) = distros::id(key) {
            let color = distros::defaults().remove(id).unwrap_or_default();
            intents.push(UiIntent::Domain(set_distro_color(model, id, color)));
        } else if let Some(side) = padding::side(key) {
            intents.push(UiIntent::Domain(set_padding_side(model, side, None)));
        } else {
            intents.push(UiIntent::Domain(Intent::ResetSetting(key.into())));
        }
        if !editing {
            self.dismiss();
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

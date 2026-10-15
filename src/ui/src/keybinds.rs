use crate::SidebarUi;
use crate::actions::{Action, reset_settings, toggle_rail};
use crate::element::ElementId;
use crate::input::{Key, Modifiers};
use crate::intent::{HostAction, UiIntent};
use crate::overlays::context_menus::{folder_actions, space_actions, tab_actions};
use vtabs_core::{Intent, Model};

pub(crate) fn display_chord(chord: &str) -> String {
    if cfg!(target_os = "macos") {
        chord.replace("Super+", "Cmd+")
    } else {
        chord.to_owned()
    }
}

pub(crate) fn update_shortcut_tooltips(
    model: &Model,
    label: fn(&str) -> String,
    hits: &mut [crate::HitRegion],
) {
    use crate::ElementId;
    for hit in hits {
        let action = match hit.id {
            ElementId::NewTab => "new_tab",
            ElementId::Settings | ElementId::SettingsTab => "settings",
            ElementId::CloseTab(_) | ElementId::CloseSettingsTab => "close",
            ElementId::Rail => "rail",
            ElementId::Refresh => "refresh",
            ElementId::Search => "tabs",
            _ => continue,
        };
        if model.settings.keyboard_shortcuts {
            let keys = vtabs_core::keybinds::bindings(&model.settings, action);
            if let Some(key) = keys.first() {
                hit.tooltip.push_str(&format!("  {}", label(key)));
            }
        }
    }
}

pub fn shortcut_chord(key: &Key, mods: Modifiers) -> String {
    let key = match key {
        Key::Character(' ') => "Space".into(),
        Key::Character(c) => c.to_ascii_lowercase().to_string(),
        Key::Function(n) => format!("F{n}"),
        other => format!("{other:?}"),
    };
    format!(
        "{}{}{}{}{key}",
        if mods.super_key { "Super+" } else { "" },
        if mods.control { "Ctrl+" } else { "" },
        if mods.alt { "Alt+" } else { "" },
        if mods.shift { "Shift+" } else { "" }
    )
}

pub fn matches_shortcut(settings: &vtabs_core::Settings, key: &Key, mods: Modifiers) -> bool {
    vtabs_core::keybinds::action(settings, &shortcut_chord(key, mods)).is_some()
}

impl SidebarUi {
    pub(crate) fn shortcut(
        &mut self,
        model: &Model,
        key: &Key,
        mods: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) -> bool {
        let Some(action) =
            vtabs_core::keybinds::action(&model.settings, &shortcut_chord(key, mods))
        else {
            return false;
        };
        match action {
            "settings" => {
                if self.settings.open {
                    self.close_settings();
                } else {
                    self.open_settings();
                }
            }
            "tabs" => intents.push(UiIntent::Host(HostAction::SearchTabs)),
            "commands" => intents.push(UiIntent::Host(HostAction::OpenCommands)),
            "tab_actions" => self.open_tab_actions(model),
            "space_actions" => self.open_space_actions(model),
            "create_space" => self.run_action(model, Action::NewSpace, intents),
            "new_folder" => self.run_action(model, Action::NewFolder, intents),
            "reset_settings" => self.run_action(model, reset_settings(), intents),
            "toggle_space" => self.run_action(
                model,
                Action::Domain(Intent::ToggleSpace(model.selected_space.clone())),
                intents,
            ),
            "return_tab_auto" => {
                if !self.settings.open
                    && let Some(id) = model.selected_tab
                {
                    self.run_action(model, Action::Domain(Intent::ReturnToAuto(id)), intents);
                }
            }
            "quick_terminal" => intents.push(UiIntent::Host(HostAction::QuickTerminal)),
            "rail" => intents.push(UiIntent::Domain(toggle_rail(model))),
            "new_tab" => {
                self.hide_settings();
                intents.push(UiIntent::Domain(Intent::NewTab));
            }
            "close" => {
                if self.settings.open {
                    self.close_settings();
                } else if let Some(id) = model.selected_tab {
                    self.close_tab(model, id, intents);
                }
            }
            "refresh" => intents.push(UiIntent::Refresh),
            action if numbered_action(action, "tab_").is_some() => {
                let number = numbered_action(action, "tab_").unwrap();
                let index = if number == 9 { -1 } else { number as isize - 1 };
                self.activate_index(model, index, intents);
            }
            action if numbered_action(action, "space_").is_some() => {
                let number = numbered_action(action, "space_").unwrap();
                if let Some(space) = model.spaces.get(number - 1) {
                    intents.push(UiIntent::Domain(Intent::SelectSpace(space.id.clone())));
                }
            }
            "next_tab" | "previous_tab" => {
                self.activate_relative(
                    model,
                    if action == "previous_tab" { -1 } else { 1 },
                    true,
                    intents,
                );
            }
            "previous_space" | "next_space" => {
                if model.spaces.is_empty() {
                    return true;
                }
                let current = model
                    .spaces
                    .iter()
                    .position(|s| s.id == model.selected_space)
                    .unwrap_or(0);
                let next = if action == "previous_space" {
                    (current + model.spaces.len() - 1) % model.spaces.len()
                } else {
                    (current + 1) % model.spaces.len()
                };
                intents.push(UiIntent::Domain(Intent::SelectSpace(
                    model.spaces[next].id.clone(),
                )));
            }
            _ => {
                // Resolve against current state so shortcuts share menu availability and policy.
                let group = vtabs_core::keybinds::DESCRIPTORS
                    .iter()
                    .find(|field| field.key.strip_prefix("shortcut.") == Some(action))
                    .map(|field| field.group);
                let items = match group {
                    Some("appearance") if !self.settings.open => model
                        .selected_tab
                        .map(|id| tab_actions(model, id))
                        .unwrap_or_default(),
                    Some("spaces") => space_actions(model, &model.selected_space),
                    Some("folders") if !self.settings.open => {
                        let folder = match &self.focused {
                            Some(ElementId::Folder(id)) => Some(id.as_str()),
                            _ => model
                                .selected_tab
                                .and_then(|id| model.tabs.get(&id))
                                .and_then(|tab| tab.folder_id.as_deref()),
                        };
                        folder
                            .map(|id| folder_actions(model, id))
                            .unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                if let Some(item) = items
                    .into_iter()
                    .find(|item| item.binding == Some(action) && item.enabled)
                {
                    self.run_action(model, item.action, intents);
                }
            }
        }
        true
    }
}

fn numbered_action(action: &str, prefix: &str) -> Option<usize> {
    action
        .strip_prefix(prefix)?
        .parse()
        .ok()
        .filter(|n| (1..=9).contains(n))
}

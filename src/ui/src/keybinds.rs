use crate::SidebarUi;
use crate::actions::toggle_rail;
use crate::input::{Key, Modifiers};
use crate::intent::{HostAction, UiIntent};
use vtabs_core::{Intent, Model};

pub(crate) fn display_chord(chord: &str) -> String {
    if cfg!(target_os = "macos") {
        chord.replace("Super+", "Cmd+")
    } else {
        chord.to_owned()
    }
}

pub(crate) fn update_shortcut_tooltips(model: &Model, hits: &mut [crate::HitRegion]) {
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
                hit.tooltip.push_str(&format!("  {}", display_chord(key)));
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
            "tabs" => self.open_tab_navigator(model),
            "commands" => intents.push(UiIntent::Host(HostAction::OpenCommands)),
            "jobs" => intents.push(UiIntent::Host(HostAction::OpenJobs)),
            "rail" => intents.push(UiIntent::Domain(toggle_rail(model))),
            "new_tab" | "reopen" => {
                self.hide_settings();
                intents.push(UiIntent::Domain(if action == "reopen" {
                    Intent::Reopen
                } else {
                    Intent::NewTab
                }));
            }
            "close" => {
                if self.settings.open {
                    self.close_settings();
                } else if let Some(id) = model.selected_tab {
                    self.close_tab(model, id, intents);
                }
            }
            "refresh" => intents.push(UiIntent::Refresh),
            action if action.starts_with("tab_") => {
                let c = action.as_bytes()[4] as char;
                let index = if c == '9' {
                    -1
                } else {
                    (c as u8 - b'1') as isize
                };
                self.activate_index(model, index, intents);
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
            _ => return false,
        }
        true
    }
}

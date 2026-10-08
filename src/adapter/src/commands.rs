use crate::commands::ExpandedCommand;
use crate::inputmap::InputMap;
use config::keyassignment::KeyAssignment;
use std::convert::TryFrom;
use vtabs_app::ui::CommandEntry;
use window::{KeyCode, Modifiers};

pub(super) fn entries(
    commands: &[ExpandedCommand],
    settings: &vtabs_app::core::Settings,
) -> Vec<CommandEntry> {
    let config = config::configuration();
    let input = InputMap::new(&config);
    commands
        .iter()
        .map(|command| {
            let mut keys: Vec<_> = input
                .keys
                .default
                .iter()
                .filter(|(_, entry)| entry.action == command.action)
                .map(|((key, mods), _)| (*mods, key.clone()))
                .collect();
            keys.sort_by_key(|(mods, _)| {
                (
                    mods.contains(Modifiers::SUPER) != cfg!(target_os = "macos"),
                    mods.bits().count_ones(),
                )
            });
            let mut shortcuts = Vec::new();
            for (mods, key) in keys {
                let shortcut = shortcut_label(&key, mods);
                if !shortcuts.contains(&shortcut) {
                    shortcuts.push(shortcut);
                }
            }
            if settings.keyboard_shortcuts
                && command.action == KeyAssignment::ActivateCommandPalette
            {
                let mut plugin = vtabs_app::core::keybinds::bindings(settings, "commands")
                    .iter()
                    .filter_map(|chord| binding_label(chord))
                    .collect::<Vec<_>>();
                for key in shortcuts {
                    if !plugin.contains(&key) {
                        plugin.push(key);
                    }
                }
                shortcuts = plugin;
            }
            CommandEntry {
                label: command.brief.to_string(),
                shortcuts,
                description: format!("{} {}", command.menubar.join(" "), command.doc),
            }
        })
        .collect()
}

pub(super) fn shortcut_label(key: &KeyCode, mods: Modifiers) -> String {
    let key = match key {
        KeyCode::Physical(physical) => physical.to_key_code(),
        key => key.clone(),
    };
    let mut mods = mods.remove_positional_mods();
    if matches!(key, KeyCode::Char(c) if c.is_ascii_uppercase()) {
        mods |= Modifiers::SHIFT;
    }
    let rendering = config::configuration().ui_key_cap_rendering;
    let key = match key {
        KeyCode::Char('\u{7f}') => "Delete".to_owned(),
        KeyCode::Char('\u{8}') => "Backspace".to_owned(),
        key => crate::inputmap::ui_key(&key, rendering),
    };
    let mods = mods.to_string_with_separator(window::ModifierToStringArgs {
        separator: "+",
        want_none: false,
        ui_key_cap_rendering: Some(rendering),
    });
    if mods.is_empty() {
        key
    } else {
        format!("{mods}+{key}")
    }
}

fn binding_label(chord: &str) -> Option<String> {
    let mut key = chord;
    let mut mods = Modifiers::NONE;
    for (prefix, modifier) in [
        ("Super+", Modifiers::SUPER),
        ("Ctrl+", Modifiers::CTRL),
        ("Alt+", Modifiers::ALT),
        ("Shift+", Modifiers::SHIFT),
    ] {
        if let Some(rest) = key.strip_prefix(prefix) {
            key = rest;
            mods |= modifier;
        }
    }
    let key = match key {
        "Space" => " ",
        "Left" => "LeftArrow",
        "Right" => "RightArrow",
        "Up" => "UpArrow",
        "Down" => "DownArrow",
        key => key,
    };
    Some(shortcut_label(&KeyCode::try_from(key).ok()?, mods))
}

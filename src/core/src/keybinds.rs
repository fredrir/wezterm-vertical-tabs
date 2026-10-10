use crate::settings::{SettingDescriptor, SettingKind, Settings};
use std::collections::{BTreeMap, BTreeSet};

macro_rules! shortcuts {
    ($(($action:literal, $label:literal, $description:literal, $mac:expr, $other:expr)),* $(,)?) => {
        pub const DESCRIPTORS: &[SettingDescriptor] = &[
            $(SettingDescriptor {
                key: concat!("shortcut.", $action),
                label: $label,
                group: "keybinds",
                kind: SettingKind::Keybinds,
                description: $description,
            }),*
        ];

        fn default_shortcuts(action: &str) -> &'static [&'static str] {
            match action {
                $($action => if cfg!(target_os = "macos") { $mac } else { $other }),*,
                _ => &[],
            }
        }
    };
}

shortcuts! {
    ("settings", "Open settings", "Open or close plugin settings",
        &["Super+,", "Super+Shift+,"], &["Ctrl+Shift+,", "Super+,", "Super+Shift+,"]),
    ("tabs", "Search tabs", "Find tabs across spaces and windows",
        &["Super+k", "Super+Shift+k"], &["Ctrl+Shift+k", "Super+k", "Super+Shift+k"]),
    ("commands", "Command palette", "Open the command palette",
        &["Super+p", "Super+Shift+p"], &["Ctrl+Shift+p", "Super+p", "Super+Shift+p"]),
    ("jobs", "Search jobs", "Find running and suspended jobs",
        &["Super+z", "Super+Shift+z"], &["Ctrl+Shift+z", "Super+z", "Super+Shift+z"]),
    ("quick_terminal", "Quick terminal", "Show or hide the floating terminal",
        &["Ctrl+`"], &["Ctrl+`"]),
    ("rail", "Toggle sidebar", "Show or hide the sidebar",
        &["Super+b", "Super+Shift+b"], &["Ctrl+Shift+b", "Super+b", "Super+Shift+b"]),
    ("new_tab", "New tab", "Create a tab in the current space",
        &["Super+t"], &["Ctrl+Shift+t", "Super+t"]),
    ("close", "Close tab or settings", "Close the current tab or settings page",
        &["Super+w", "Super+Shift+w"], &["Ctrl+Shift+w", "Super+w", "Super+Shift+w"]),
    ("refresh", "Refresh", "Refresh the configuration",
        &["Super+Shift+r", "Super+r"], &["Ctrl+Shift+r", "Super+r", "Super+Shift+r"]),
    ("tab_1", "Activate tab 1", "Activate visible tab 1",
        &["Super+1", "Super+Shift+1"], &["Ctrl+Shift+1", "Super+1", "Super+Shift+1"]),
    ("tab_2", "Activate tab 2", "Activate visible tab 2",
        &["Super+2", "Super+Shift+2"], &["Ctrl+Shift+2", "Super+2", "Super+Shift+2"]),
    ("tab_3", "Activate tab 3", "Activate visible tab 3",
        &["Super+3", "Super+Shift+3"], &["Ctrl+Shift+3", "Super+3", "Super+Shift+3"]),
    ("tab_4", "Activate tab 4", "Activate visible tab 4",
        &["Super+4", "Super+Shift+4"], &["Ctrl+Shift+4", "Super+4", "Super+Shift+4"]),
    ("tab_5", "Activate tab 5", "Activate visible tab 5",
        &["Super+5", "Super+Shift+5"], &["Ctrl+Shift+5", "Super+5", "Super+Shift+5"]),
    ("tab_6", "Activate tab 6", "Activate visible tab 6",
        &["Super+6", "Super+Shift+6"], &["Ctrl+Shift+6", "Super+6", "Super+Shift+6"]),
    ("tab_7", "Activate tab 7", "Activate visible tab 7",
        &["Super+7", "Super+Shift+7"], &["Ctrl+Shift+7", "Super+7", "Super+Shift+7"]),
    ("tab_8", "Activate tab 8", "Activate visible tab 8",
        &["Super+8", "Super+Shift+8"], &["Ctrl+Shift+8", "Super+8", "Super+Shift+8"]),
    ("tab_9", "Activate last tab", "Activate the last visible tab",
        &["Super+9", "Super+Shift+9"], &["Ctrl+Shift+9", "Super+9", "Super+Shift+9"]),
    ("next_tab", "Next tab", "Activate the next visible tab",
        &["Ctrl+Tab"], &["Ctrl+Tab"]),
    ("previous_tab", "Previous tab", "Activate the previous visible tab",
        &["Ctrl+Shift+Tab"], &["Ctrl+Shift+Tab"]),
    ("previous_space", "Previous space", "Switch to the previous space",
        &["Ctrl+Alt+Left"], &["Ctrl+Alt+Left"]),
    ("next_space", "Next space", "Switch to the next space",
        &["Ctrl+Alt+Right"], &["Ctrl+Alt+Right"]),
}

pub fn defaults(action: &str) -> Vec<String> {
    default_shortcuts(action)
        .iter()
        .map(|key| (*key).into())
        .collect()
}

pub fn bindings(settings: &Settings, action: &str) -> Vec<String> {
    settings
        .keybinds
        .get(action)
        .cloned()
        .unwrap_or_else(|| defaults(action))
}

pub fn action(settings: &Settings, chord: &str) -> Option<&'static str> {
    if !settings.keyboard_shortcuts {
        return None;
    }
    DESCRIPTORS.iter().find_map(|field| {
        let action = field.key.strip_prefix("shortcut.")?;
        let matches = settings.keybinds.get(action).map_or_else(
            || default_shortcuts(action).contains(&chord),
            |keys| keys.iter().any(|key| key == chord),
        );
        matches.then_some(action)
    })
}

fn valid_chord(chord: &str) -> bool {
    let mut key = chord;
    for modifier in ["Super+", "Ctrl+", "Alt+", "Shift+"] {
        if let Some(rest) = key.strip_prefix(modifier) {
            key = rest;
        }
    }
    let named = matches!(
        key,
        "Enter"
            | "Tab"
            | "Backspace"
            | "Delete"
            | "Left"
            | "Right"
            | "Up"
            | "Down"
            | "Home"
            | "End"
            | "PageUp"
            | "PageDown"
            | "Space"
    ) || key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n) && key == format!("F{n}"));
    // Bare text keys must remain available to terminal applications and editors.
    let modified = chord != key;
    (named
        || (key.chars().count() == 1
            && key
                .chars()
                .all(|c| !c.is_control() && !c.is_whitespace() && !c.is_ascii_uppercase())))
        && (modified || key.starts_with('F'))
}

pub fn validate(overrides: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    if overrides.keys().any(|key| {
        !DESCRIPTORS
            .iter()
            .any(|d| d.key.strip_prefix("shortcut.") == Some(key.as_str()))
    }) {
        return Err("Unknown plugin action".into());
    }
    let mut used = BTreeSet::new();
    for field in DESCRIPTORS {
        let action = field.key.strip_prefix("shortcut.").unwrap();
        let keys = overrides
            .get(action)
            .cloned()
            .unwrap_or_else(|| defaults(action));
        if keys.len() > 16 {
            return Err("At most 16 shortcuts per action".into());
        }
        for key in keys {
            if !valid_chord(&key) {
                return Err(format!(
                    "Unsupported shortcut: {key}. Use modifiers or a function key."
                ));
            }
            if !used.insert(key.clone()) {
                return Err(format!(
                    "{key} is already assigned. Remove its existing binding first."
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn deserialize<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    parse(&serde_json::Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

pub(crate) fn parse(value: &serde_json::Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut value = value.clone();
    let entries = value
        .as_object_mut()
        .ok_or("Keybinds must be a table of plugin actions")?;
    for keys in entries.values_mut() {
        // Lua uses the same empty table for maps and lists.
        if keys.as_object().is_some_and(|table| table.is_empty()) {
            *keys = serde_json::json!([]);
        }
    }
    serde_json::from_value(value)
        .map_err(|_| "Each plugin action must contain a list of shortcuts".into())
}

//! The command palette and launcher menu, labeled with host key assignments.
use super::*;
use crate::commands::ExpandedCommand;
use crate::inputmap::InputMap;
use config::keyassignment::KeyAssignment;
use std::convert::TryFrom;
use vtabs_app::ui::CommandEntry;
use window::{KeyCode, Modifiers};

impl Adapter {
    pub(super) fn show_palette(&mut self, commands: Vec<ExpandedCommand>) {
        self.cancel_paste();
        self.launcher_entries.clear();
        let entries = entries(&commands, &self.app.model().settings);
        self.palette_commands = commands;
        self.app.ui_mut().open_commands(entries);
    }
    pub(super) fn show_launcher(
        &mut self,
        args: config::keyassignment::LauncherActionArgs,
        launcher: crate::termwindow::ui_host::launcher::LauncherEntries,
    ) {
        self.cancel_paste();
        self.palette_commands.clear();
        let entries = launcher
            .entries
            .iter()
            .map(|entry| CommandEntry {
                label: entry.label.clone(),
                shortcuts: Vec::new(),
                description: String::new(),
            })
            .collect();
        self.launcher_entries = launcher.entries;
        self.app.ui_mut().open_launch_menu(
            ui::LaunchMenuConfig {
                title: args.title.unwrap_or_else(|| "Launcher".into()),
                help_text: args
                    .help_text
                    .unwrap_or_else(|| "Enter=launch  Esc=cancel  /=filter".into()),
                fuzzy_help_text: args
                    .fuzzy_help_text
                    .unwrap_or_else(|| "Fuzzy matching: ".into()),
                alphabet: args
                    .alphabet
                    .unwrap_or_else(|| config::configuration().launcher_alphabet.clone()),
                fuzzy: args
                    .flags
                    .contains(config::keyassignment::LauncherFlags::FUZZY),
                labels:
                    crate::overlay::quickselect::compute_labels_for_alphabet_with_preserved_case,
                matches: |query, label| {
                    crate::overlay::selector::matcher_score(
                        &crate::overlay::selector::matcher_pattern(query),
                        label,
                    )
                },
            },
            entries,
            launcher.selected,
        );
    }
}

fn entries(commands: &[ExpandedCommand], settings: &core::Settings) -> Vec<CommandEntry> {
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
        // These arrows come from the UI's bundled Symbols Nerd Font Mono;
        // ordinary Unicode arrows depend on the user's text-font fallback.
        KeyCode::LeftArrow => "\u{f177}".to_owned(),
        KeyCode::UpArrow => "\u{f176}".to_owned(),
        KeyCode::RightArrow => "\u{f178}".to_owned(),
        KeyCode::DownArrow => "\u{f175}".to_owned(),
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

/// Plugin chords drawn like host key assignments, honoring `ui_key_cap_rendering`.
pub(super) fn chord_label(chord: &str) -> String {
    binding_label(chord).unwrap_or_else(|| chord.to_owned())
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

#[cfg(test)]
#[path = "../tests/palette.rs"]
mod tests;

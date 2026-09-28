use crate::commands::ExpandedCommand;
use crate::inputmap::InputMap;
use config::keyassignment::KeyAssignment;
use vtabs_app::ui::CommandEntry;
use window::{KeyCode, Modifiers};

pub(super) fn entries(commands: &[ExpandedCommand], shortcuts: bool) -> Vec<CommandEntry> {
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
            if shortcuts && command.action == KeyAssignment::ActivateCommandPalette {
                keys.insert(
                    0,
                    (
                        if cfg!(target_os = "macos") {
                            Modifiers::SUPER
                        } else {
                            Modifiers::CTRL | Modifiers::SHIFT
                        },
                        KeyCode::Char('p'),
                    ),
                );
            }
            let shortcut = keys
                .first()
                .map(|(mods, key)| {
                    let key = crate::inputmap::ui_key(key, config.ui_key_cap_rendering);
                    let mods = mods.to_string_with_separator(window::ModifierToStringArgs {
                        separator: "+",
                        want_none: false,
                        ui_key_cap_rendering: Some(config.ui_key_cap_rendering),
                    });
                    if mods.is_empty() {
                        key
                    } else {
                        format!("{mods}+{key}")
                    }
                })
                .unwrap_or_default();
            CommandEntry {
                label: command.brief.to_string(),
                shortcut,
                description: format!("{} {}", command.menubar.join(" "), command.doc),
            }
        })
        .collect()
}

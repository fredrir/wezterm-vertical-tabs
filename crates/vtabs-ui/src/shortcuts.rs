use crate::*;
use vtabs_core::{Intent, Model, RailMode};

/// Platform shortcuts avoid intercepting ordinary terminal Control combinations.
pub fn is_shortcut(key: &Key, mods: Modifiers) -> bool {
    let command = if cfg!(target_os = "macos") {
        mods.super_key
    } else {
        mods.super_key || (mods.control && mods.shift)
    };
    (command
        && !mods.alt
        && matches!(
            key,
            Key::Character(
                ',' | 't' | 'T' | 'k' | 'K' | 'b' | 'B' | 'w' | 'W' | 'g' | 'G' | 'r' | 'R' | '1'
                    ..='9'
            )
        ))
        || (mods.control && !mods.super_key && !mods.alt && *key == Key::Tab)
        || (mods.control && mods.alt && matches!(key, Key::Left | Key::Right))
}

impl SidebarUi {
    pub(crate) fn shortcut(
        &mut self,
        model: &Model,
        key: &Key,
        mods: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) -> bool {
        if !is_shortcut(key, mods) {
            return false;
        }
        match key {
            Key::Character(',') => {
                if self.settings_page {
                    self.close_settings();
                } else {
                    self.open_settings();
                }
            }
            Key::Character('k' | 'K') => self.open_tab_navigator(model),
            Key::Character('b' | 'B') => {
                self.close_settings();
                intents.push(UiIntent::Domain(Intent::SetRail(
                    if model.settings.rail == RailMode::Expanded {
                        RailMode::Collapsed
                    } else {
                        RailMode::Expanded
                    },
                )));
            }
            Key::Character('t' | 'T') => {
                self.close_settings();
                intents.push(UiIntent::Domain(if mods.shift && mods.super_key {
                    Intent::Reopen
                } else {
                    Intent::NewTab
                }));
            }
            Key::Character('w' | 'W') => {
                if self.settings_page {
                    self.close_settings();
                } else if let Some(id) = model.selected_tab {
                    self.close_tab(model, id, intents);
                }
            }
            Key::Character('g' | 'G') => self.open_create_folder(),
            Key::Character('r' | 'R') => intents.push(UiIntent::Refresh),
            Key::Character(c @ '1'..='9') => {
                self.close_settings();
                intents.push(UiIntent::Domain(Intent::ActivateIndex(if *c == '9' {
                    -1
                } else {
                    (*c as u8 - b'1') as isize
                })));
            }
            Key::Tab => {
                self.close_settings();
                intents.push(UiIntent::Domain(Intent::ActivateRelative {
                    delta: if mods.shift { -1 } else { 1 },
                    wrap: true,
                }));
            }
            Key::Left | Key::Right => {
                let current = model
                    .spaces
                    .iter()
                    .position(|s| s.id == model.selected_space)
                    .unwrap_or(0);
                let next = if *key == Key::Left {
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

//! Operations shared by settings rows, editors and reset actions.
use crate::{SidebarUi, intent::UiIntent};
use serde_json::{Value, json};
use vtabs_core::{
    Dimension, Intent, Model, SettingDescriptor, SettingKind, Settings, distros, keybinds, padding,
    settings,
};

#[derive(Clone, Copy)]
pub(crate) enum SettingTarget<'a> {
    Setting(&'a str),
    Shortcut(&'a str),
    DistroColor(&'a str),
    PaddingSide(&'a str),
}

impl<'a> SettingTarget<'a> {
    pub(crate) fn of(key: &'a str) -> Self {
        if let Some(action) = key.strip_prefix("shortcut.") {
            Self::Shortcut(action)
        } else if let Some(id) = distros::id(key) {
            Self::DistroColor(id)
        } else if let Some(side) = padding::side(key) {
            Self::PaddingSide(side)
        } else {
            Self::Setting(key)
        }
    }

    pub(crate) fn setting_key(self) -> &'a str {
        match self {
            Self::Setting(key) => key,
            Self::Shortcut(_) => "keybinds",
            Self::DistroColor(_) => "distro_colors",
            Self::PaddingSide(_) => "quick_terminal_window_padding",
        }
    }

    pub(crate) fn descriptor(self) -> Option<&'static SettingDescriptor> {
        let (fields, suffix) = match self {
            Self::Setting(key) => return settings::descriptor(key),
            Self::Shortcut(action) => (keybinds::DESCRIPTORS, action),
            Self::DistroColor(id) => (distros::DESCRIPTORS, id),
            Self::PaddingSide(side) => (padding::DESCRIPTORS, side),
        };
        fields.iter().find(|field| {
            field
                .key
                .split_once('.')
                .is_some_and(|(_, key)| key == suffix)
        })
    }

    pub(crate) fn value(self, settings: &Settings) -> Value {
        match self {
            Self::Setting(key) => settings.get(key).unwrap_or_default(),
            Self::Shortcut(action) => json!(keybinds::bindings(settings, action)),
            Self::DistroColor(id) => json!(settings.distro_colors.get(id)),
            Self::PaddingSide(side) => json!(settings.quick_terminal_window_padding.side(side)),
        }
    }

    pub(crate) fn changed(self, current: &Value, defaults: &Value) -> bool {
        let key = self.setting_key();
        match self {
            Self::Shortcut(action) => current[key]
                .get(action)
                .is_some_and(|keys| *keys != json!(keybinds::defaults(action))),
            Self::DistroColor(id) => current[key][id] != defaults[key][id],
            Self::PaddingSide(side) => current[key][side] != defaults[key][side],
            Self::Setting(_) => current.get(key) != defaults.get(key),
        }
    }

    pub(crate) fn submit(self, settings: &Settings, text: &str) -> Result<Intent, String> {
        let descriptor = self.descriptor().ok_or("This setting no longer exists")?;
        let value = match self {
            Self::Shortcut(_) => return Err("Use the keybind editor".into()),
            Self::PaddingSide(_) => json!(
                (!text.is_empty())
                    .then(|| Dimension::from_text(text))
                    .transpose()?
            ),
            _ => match descriptor.kind {
                SettingKind::Number { .. }
                | SettingKind::Fraction
                | SettingKind::Opacity
                | SettingKind::Scale
                | SettingKind::Colors
                | SettingKind::List => {
                    serde_json::from_str(text).map_err(|error| format!("Invalid value: {error}"))?
                }
                SettingKind::Text | SettingKind::OptionalColor | SettingKind::Border
                    if text.is_empty() =>
                {
                    Value::Null
                }
                _ => Value::String(text.into()),
            },
        };
        self.set(settings, value)
    }

    fn set(self, settings: &Settings, value: Value) -> Result<Intent, String> {
        let key = self.setting_key();
        let value = match self {
            Self::DistroColor(child) | Self::PaddingSide(child) => {
                let mut parent = settings.get(key).unwrap_or_default();
                if value.is_null() {
                    if let Some(parent) = parent.as_object_mut() {
                        parent.remove(child);
                    }
                } else {
                    parent[child] = value;
                }
                parent
            }
            _ => value,
        };
        settings::validate_value(key, &value)?;
        Ok(Intent::SetSetting {
            key: key.into(),
            value,
        })
    }

    pub(crate) fn reset(self, settings: &Settings) -> Result<Intent, String> {
        match self {
            Self::Setting(key) => Ok(Intent::ResetSetting(key.into())),
            Self::Shortcut(action) => {
                let mut bindings = settings.keybinds.clone();
                bindings.remove(action);
                self.set(settings, json!(bindings))
            }
            Self::DistroColor(_) => self.set(settings, self.value(&Settings::default())),
            Self::PaddingSide(_) => self.set(settings, Value::Null),
        }
    }
}

impl SidebarUi {
    pub(crate) fn reset_setting(&mut self, model: &Model, key: &str, intents: &mut Vec<UiIntent>) {
        if self.settings.managed_in_lua(model, key) {
            return;
        }
        let target = SettingTarget::of(key);
        match target.reset(&model.settings) {
            Ok(intent) => intents.push(UiIntent::Domain(intent)),
            Err(_) => {
                if let SettingTarget::Shortcut(action) = target {
                    self.show_keybind_reset_conflict(model, action);
                }
                return;
            }
        }
        if self.binding().is_none() {
            self.dismiss();
        }
        self.frame.dirty = true;
    }
}

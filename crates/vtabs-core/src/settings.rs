use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RailMode {
    #[default]
    Expanded,
    Collapsed,
    Hidden,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub width: u16,
    pub rail_width: u16,
    pub side: Side,
    pub rail: RailMode,
    pub animations: bool,
    pub reduced_motion: bool,
    pub animation_ms: u16,
    pub cards: bool,
    pub show_indices: bool,
    pub show_metadata: bool,
    pub show_close: bool,
    pub confirm_close: bool,
    pub keyboard_shortcuts: bool,
    pub accent: String,
    pub background: String,
    pub foreground: String,
    pub muted: String,
    pub selected_background: String,
    pub private_accent: String,
    pub reopen_limit: u16,
    pub default_domain: Option<String>,
    pub private_env: BTreeMap<String, String>,
    pub menus: Vec<MenuEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub children: Vec<MenuEntry>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub confirm: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            width: 256,
            rail_width: 40,
            side: Side::Left,
            rail: RailMode::Expanded,
            animations: true,
            reduced_motion: false,
            animation_ms: 140,
            cards: true,
            show_indices: false,
            show_metadata: false,
            show_close: true,
            confirm_close: true,
            keyboard_shortcuts: true,
            accent: "#a7c5b5".into(),
            background: "#1d2423".into(),
            foreground: "#e2e8e4".into(),
            muted: "#a1afa8".into(),
            selected_background: "#34483f".into(),
            private_accent: "#cba6f7".into(),
            reopen_limit: 20,
            default_domain: None,
            private_env: BTreeMap::from([
                ("HISTFILE".into(), "".into()),
                ("VTABS_PRIVATE".into(), "1".into()),
                ("fish_private_mode".into(), "1".into()),
            ]),
            menus: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    Bool,
    Number { min: u16, max: u16 },
    Text,
    Color,
    Choice(&'static [&'static str]),
    Object,
    List,
}

#[derive(Clone, Debug, Serialize)]
pub struct SettingDescriptor {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub kind: SettingKind,
    pub description: &'static str,
}

pub const DESCRIPTORS: &[SettingDescriptor] = &[
    SettingDescriptor {
        key: "width",
        label: "Sidebar width",
        group: "layout",
        kind: SettingKind::Number { min: 32, max: 1024 },
        description: "Expanded width in logical pixels",
    },
    SettingDescriptor {
        key: "rail_width",
        label: "Rail width",
        group: "layout",
        kind: SettingKind::Number { min: 16, max: 128 },
        description: "Collapsed width in logical pixels",
    },
    SettingDescriptor {
        key: "side",
        label: "Side",
        group: "layout",
        kind: SettingKind::Choice(&["left", "right"]),
        description: "Native sidebar edge",
    },
    SettingDescriptor {
        key: "rail",
        label: "Rail",
        group: "layout",
        kind: SettingKind::Choice(&["expanded", "collapsed", "hidden"]),
        description: "Sidebar visibility",
    },
    SettingDescriptor {
        key: "animations",
        label: "Animations",
        group: "motion",
        kind: SettingKind::Bool,
        description: "Enable finite visual transitions",
    },
    SettingDescriptor {
        key: "reduced_motion",
        label: "Reduced motion",
        group: "motion",
        kind: SettingKind::Bool,
        description: "Suppress transitions",
    },
    SettingDescriptor {
        key: "animation_ms",
        label: "Transition duration",
        group: "motion",
        kind: SettingKind::Number { min: 0, max: 1000 },
        description: "Transition duration in milliseconds",
    },
    SettingDescriptor {
        key: "cards",
        label: "Cards",
        group: "appearance",
        kind: SettingKind::Bool,
        description: "Use cards instead of compact rows",
    },
    SettingDescriptor {
        key: "show_indices",
        label: "Tab numbers",
        group: "appearance",
        kind: SettingKind::Bool,
        description: "Visible native tab indices",
    },
    SettingDescriptor {
        key: "show_metadata",
        label: "Metadata",
        group: "appearance",
        kind: SettingKind::Bool,
        description: "Show current directory and domain",
    },
    SettingDescriptor {
        key: "show_close",
        label: "Close controls",
        group: "appearance",
        kind: SettingKind::Bool,
        description: "Show close controls",
    },
    SettingDescriptor {
        key: "confirm_close",
        label: "Confirm close",
        group: "behavior",
        kind: SettingKind::Bool,
        description: "Confirm destructive UI close actions",
    },
    SettingDescriptor {
        key: "accent",
        label: "Accent",
        group: "theme",
        kind: SettingKind::Color,
        description: "Default accent color",
    },
    SettingDescriptor {
        key: "background",
        label: "Background",
        group: "theme",
        kind: SettingKind::Color,
        description: "Surface background",
    },
    SettingDescriptor {
        key: "foreground",
        label: "Foreground",
        group: "theme",
        kind: SettingKind::Color,
        description: "Text foreground",
    },
    SettingDescriptor {
        key: "muted",
        label: "Muted",
        group: "theme",
        kind: SettingKind::Color,
        description: "Secondary text foreground",
    },
    SettingDescriptor {
        key: "selected_background",
        label: "Selection",
        group: "theme",
        kind: SettingKind::Color,
        description: "Selected card background",
    },
    SettingDescriptor {
        key: "private_accent",
        label: "Private accent",
        group: "theme",
        kind: SettingKind::Color,
        description: "Private window accent",
    },
    SettingDescriptor {
        key: "keyboard_shortcuts",
        label: "Keyboard shortcuts",
        group: "behavior",
        kind: SettingKind::Bool,
        description: "Enable native tab, search, settings and sidebar shortcuts",
    },
    SettingDescriptor {
        key: "reopen_limit",
        label: "Reopen history",
        group: "behavior",
        kind: SettingKind::Number { min: 0, max: 100 },
        description: "Maximum in-memory launch intents",
    },
    SettingDescriptor {
        key: "default_domain",
        label: "Default domain",
        group: "behavior",
        kind: SettingKind::Text,
        description: "Spawn domain used in an empty space; null retains the host default",
    },
    SettingDescriptor {
        key: "private_env",
        label: "Private environment",
        group: "behavior",
        kind: SettingKind::Object,
        description: "Environment additions for explicitly created private windows",
    },
    SettingDescriptor {
        key: "menus",
        label: "Custom menus",
        group: "behavior",
        kind: SettingKind::List,
        description: "Nested semantic action menus",
    },
];

pub fn descriptors() -> &'static [SettingDescriptor] {
    DESCRIPTORS
}
pub fn descriptor(key: &str) -> Option<&'static SettingDescriptor> {
    DESCRIPTORS.iter().find(|d| d.key == key)
}
pub fn valid_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn validate_value(key: &str, value: &Value) -> Result<(), String> {
    let d = descriptor(key).ok_or_else(|| format!("Unknown setting: {key}"))?;
    let valid = match d.kind {
        SettingKind::Bool => value.is_boolean(),
        SettingKind::Number { min, max } => value
            .as_u64()
            .is_some_and(|n| n >= u64::from(min) && n <= u64::from(max)),
        SettingKind::Text => value.is_null() || value.as_str().is_some_and(|s| s.len() <= 4096),
        SettingKind::Color => value.as_str().is_some_and(valid_color),
        SettingKind::Choice(choices) => value.as_str().is_some_and(|s| choices.contains(&s)),
        SettingKind::Object => value.as_object().is_some_and(|o| {
            o.len() <= 128
                && o.iter()
                    .all(|(k, v)| k.len() <= 256 && v.as_str().is_some_and(|s| s.len() <= 4096))
        }),
        SettingKind::List => serde_json::from_value::<Vec<MenuEntry>>(value.clone())
            .is_ok_and(|v| validate_menus(&v, 0)),
    };
    if valid {
        Ok(())
    } else {
        Err(format!("Invalid value for {key}"))
    }
}

fn validate_menus(entries: &[MenuEntry], depth: usize) -> bool {
    entries.len() <= 64
        && depth <= 8
        && entries.iter().all(|e| {
            !e.id.is_empty()
                && e.id.len() <= 128
                && !e.label.is_empty()
                && e.label.len() <= 512
                && validate_menus(&e.children, depth + 1)
        })
}

impl Settings {
    pub fn get(&self, key: &str) -> Option<Value> {
        serde_json::to_value(self).ok()?.get(key).cloned()
    }
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        validate_value(key, &value)?;
        let mut all = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        all[key] = value;
        *self = serde_json::from_value(all).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn logical_width(&self) -> u16 {
        match self.rail {
            RailMode::Expanded => self.width,
            RailMode::Collapsed => self.rail_width,
            RailMode::Hidden => 0,
        }
    }
    pub fn schema() -> Value {
        json!({"version":1,"defaults":Self::default(),"options":DESCRIPTORS})
    }
}

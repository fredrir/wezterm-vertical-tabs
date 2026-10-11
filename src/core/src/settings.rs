use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RailMode {
    #[default]
    Expanded,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HideOnEscape {
    Never,
    Always,
    /// Only while nothing runs in the shell, judged like closing a tab.
    #[default]
    Auto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub width: u16,
    pub side: Side,
    pub rail: RailMode,
    pub quick_terminal_width: f64,
    pub quick_terminal_height: f64,
    pub quick_terminal_blur: u16,
    pub quick_terminal_opacity: f64,
    pub quick_terminal_border_color: Option<String>,
    pub quick_terminal_background_color: Option<String>,
    pub quick_terminal_auto_hide: bool,
    pub quick_terminal_preserve_shell: bool,
    pub quick_terminal_hide_on_escape: HideOnEscape,
    pub animations: bool,
    pub reduced_motion: bool,
    pub animation_ms: u16,
    pub cards: bool,
    pub show_indexes: bool,
    pub show_metadata: bool,
    pub show_close: bool,
    pub confirm_close: bool,
    pub keyboard_shortcuts: bool,
    #[serde(deserialize_with = "crate::keybinds::deserialize")]
    pub keybinds: BTreeMap<String, Vec<String>>,
    pub accent: String,
    pub background: String,
    pub foreground: String,
    pub muted: String,
    pub selected_background: String,
    pub distro_colors: BTreeMap<String, String>,
    pub default_domain: Option<String>,
    pub menus: Vec<MenuEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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
            side: Side::Left,
            rail: RailMode::Expanded,
            quick_terminal_width: 0.75,
            quick_terminal_height: 0.75,
            quick_terminal_blur: 0,
            quick_terminal_opacity: 1.,
            quick_terminal_border_color: None,
            quick_terminal_background_color: None,
            quick_terminal_auto_hide: false,
            quick_terminal_preserve_shell: true,
            quick_terminal_hide_on_escape: HideOnEscape::Auto,
            animations: true,
            reduced_motion: false,
            animation_ms: 140,
            cards: true,
            show_indexes: true,
            show_metadata: false,
            show_close: true,
            confirm_close: true,
            keyboard_shortcuts: true,
            keybinds: BTreeMap::new(),
            accent: "#a9c7f5".into(),
            background: "#192231".into(),
            foreground: "#e1e7f0".into(),
            muted: "#98a8be".into(),
            selected_background: "#34485f".into(),
            distro_colors: BTreeMap::new(),
            default_domain: None,
            menus: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    Bool,
    Number {
        min: u16,
        max: u16,
    },
    /// Above 0, at most 1.
    Fraction,
    /// From 0 (invisible) through 1 (opaque).
    Opacity,
    Text,
    Color,
    /// Null follows the terminal palette.
    OptionalColor,
    /// Keyed hex colors.
    Colors,
    Choice(&'static [&'static str]),
    List,
    Keybinds,
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
        key: "keybinds",
        label: "Plugin keybinds",
        group: "keybinds",
        kind: SettingKind::Keybinds,
        description: "Shortcuts supplied by this plugin",
    },
    SettingDescriptor {
        key: "width",
        label: "Sidebar width",
        group: "layout",
        kind: SettingKind::Number { min: 32, max: 1024 },
        description: "Width in logical pixels",
    },
    SettingDescriptor {
        key: "side",
        label: "Side",
        group: "layout",
        kind: SettingKind::Choice(&["left", "right"]),
        description: "Sidebar edge",
    },
    SettingDescriptor {
        key: "rail",
        label: "Rail",
        group: "layout",
        kind: SettingKind::Choice(&["expanded", "hidden"]),
        description: "Sidebar visibility",
    },
    SettingDescriptor {
        key: "quick_terminal_width",
        label: "Quick terminal width",
        group: "quick_terminal",
        kind: SettingKind::Fraction,
        description: "Fraction of the window width",
    },
    SettingDescriptor {
        key: "quick_terminal_height",
        label: "Quick terminal height",
        group: "quick_terminal",
        kind: SettingKind::Fraction,
        description: "Fraction of the window height",
    },
    SettingDescriptor {
        key: "quick_terminal_blur",
        label: "Blur",
        group: "quick_terminal",
        kind: SettingKind::Number { min: 0, max: 64 },
        description: "Backdrop blur radius in logical pixels behind a translucent background",
    },
    SettingDescriptor {
        key: "quick_terminal_opacity",
        label: "Opacity",
        group: "quick_terminal",
        kind: SettingKind::Opacity,
        description: "Background opacity; 1 is opaque and text stays opaque",
    },
    SettingDescriptor {
        key: "quick_terminal_border_color",
        label: "Border color",
        group: "quick_terminal",
        kind: SettingKind::OptionalColor,
        description: "Null follows the terminal foreground at 22% opacity",
    },
    SettingDescriptor {
        key: "quick_terminal_background_color",
        label: "Background color",
        group: "quick_terminal",
        kind: SettingKind::OptionalColor,
        description: "Null follows the quick terminal palette background",
    },
    SettingDescriptor {
        key: "quick_terminal_auto_hide",
        label: "Auto hide",
        group: "quick_terminal",
        kind: SettingKind::Bool,
        description: "Hide the quick terminal when the window loses focus",
    },
    SettingDescriptor {
        key: "quick_terminal_preserve_shell",
        label: "Preserve shell",
        group: "quick_terminal",
        kind: SettingKind::Bool,
        description: "Keep the shell when hidden; disable to start a fresh shell each time",
    },
    SettingDescriptor {
        key: "quick_terminal_hide_on_escape",
        label: "Hide on Escape",
        group: "quick_terminal",
        kind: SettingKind::Choice(&["never", "always", "auto"]),
        description: "Auto hides only while nothing runs in the shell",
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
        key: "show_indexes",
        label: "Tab numbers",
        group: "appearance",
        kind: SettingKind::Bool,
        description: "Visible tab indices",
    },
    SettingDescriptor {
        key: "show_metadata",
        label: "Metadata",
        group: "appearance",
        kind: SettingKind::Bool,
        description: "Show the session domain",
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
        description: "Confirm closing a tab with a running process",
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
        key: "distro_colors",
        label: "Remote glyphs",
        group: "theme",
        kind: SettingKind::Colors,
        description: "Remote glyph colors by os-release ID, `remote` when unknown",
    },
    SettingDescriptor {
        key: "keyboard_shortcuts",
        label: "Keyboard shortcuts",
        group: "behavior",
        kind: SettingKind::Bool,
        description: "Enable tab, search, settings and sidebar shortcuts",
    },
    SettingDescriptor {
        key: "default_domain",
        label: "Default domain",
        group: "behavior",
        kind: SettingKind::Text,
        description: "Spawn domain used in an empty space; null retains the host default",
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

pub fn valid_fraction(value: f64) -> bool {
    value > 0. && value <= 1.
}

pub fn validate_value(key: &str, value: &Value) -> Result<(), String> {
    let d = descriptor(key).ok_or_else(|| format!("Unknown setting: {key}"))?;
    let valid = match d.kind {
        SettingKind::Keybinds => return crate::keybinds::validate(&crate::keybinds::parse(value)?),
        SettingKind::Bool => value.is_boolean(),
        SettingKind::Number { min, max } => value
            .as_u64()
            .is_some_and(|n| n >= u64::from(min) && n <= u64::from(max)),
        SettingKind::Fraction => value.as_f64().is_some_and(valid_fraction),
        SettingKind::Opacity => value.as_f64().is_some_and(|v| (0. ..=1.).contains(&v)),
        SettingKind::Text => value.is_null() || value.as_str().is_some_and(|s| s.len() <= 4096),
        SettingKind::Color => value.as_str().is_some_and(valid_color),
        SettingKind::OptionalColor => value.is_null() || value.as_str().is_some_and(valid_color),
        SettingKind::Colors => value.as_object().is_some_and(|o| {
            o.len() <= 128
                && o.iter()
                    .all(|(k, v)| k.len() <= 64 && v.as_str().is_some_and(valid_color))
        }),
        SettingKind::Choice(choices) => value.as_str().is_some_and(|s| choices.contains(&s)),
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
            RailMode::Hidden => 0,
        }
    }
    pub fn schema() -> Value {
        json!({"version":1,"defaults":Self::default(),"options":DESCRIPTORS})
    }
}

/// Schema marker for a partial settings table; Lua types come from `DESCRIPTORS`.
pub struct Overrides;
impl JsonSchema for Overrides {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Settings".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "object"})
    }
}

/// Schema for a setting name, enumerated from `DESCRIPTORS`.
pub struct Key;
impl JsonSchema for Key {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SettingKey".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let keys: Vec<_> = DESCRIPTORS.iter().map(|d| d.key).collect();
        schemars::json_schema!({"type": "string", "enum": keys})
    }
}

/// Theme hooks may override these keys only.
pub fn is_theme(key: &str) -> bool {
    descriptor(key).is_some_and(|d| d.group == "theme")
}

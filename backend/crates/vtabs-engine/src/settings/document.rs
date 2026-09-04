use std::collections::{BTreeMap, BTreeSet};

use unicode_segmentation::UnicodeSegmentation;

use super::normalize::normalize_cross_fields;
use super::persistence;
pub use super::schema::ApplyMode;
use super::schema::{Descriptor, Kind, by_key, canonical_value, defaults, options, policy_for};
use super::value::{SettingPath, Value, get_at, set_at};

#[derive(Debug, Clone, PartialEq)]
pub struct RawSettings {
    /// Fully resolved serializable values. Callbacks travel separately in `opaque`.
    pub values: Value,
    /// Paths supplied by config-as-code. They are read-only and omitted from persistence.
    pub explicit: BTreeSet<SettingPath>,
    /// WezTerm config keys that were already owned before the plugin ran.
    pub host_values: BTreeSet<String>,
    /// Nearest setting paths whose values contain callbacks/functions. Their bodies never cross
    /// into Rust, and the containing value is therefore read-only and never persisted.
    pub opaque: BTreeSet<SettingPath>,
    /// Effective shipped bindings, before the user's `keys` overrides.
    pub key_defaults: BTreeMap<String, Value>,
    pub is_macos: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub path: SettingPath,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widget {
    Toggle,
    Picker,
    Stepper,
    Colour,
    Text,
    Recorder,
    Variant,
    Entries,
    Locked,
}

impl Widget {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Picker => "picker",
            Self::Stepper => "stepper",
            Self::Colour => "colour",
            Self::Text => "text",
            Self::Recorder => "recorder",
            Self::Variant => "variant",
            Self::Entries => "entries",
            Self::Locked => "locked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockReason {
    Explicit,
    Host,
    Opaque,
}

impl LockReason {
    pub const fn text(self) -> &'static str {
        match self {
            Self::Explicit => "wezterm.lua",
            Self::Host => "wezterm.lua (host)",
            Self::Opaque => "not editable",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub path: SettingPath,
    pub label: String,
    pub group: String,
    pub widget: Widget,
    pub value: Option<Value>,
    pub default: Option<Value>,
    pub value_text: String,
    pub changed: bool,
    pub locked: Option<LockReason>,
    pub depth: usize,
    pub help: String,
    pub entries: Option<usize>,
    pub editing: Option<String>,
    pub armed: bool,
    pub apply_mode: ApplyMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentState {
    pub editing: Option<Editing>,
    pub armed: Option<SettingPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    pub path: SettingPath,
    pub buffer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentAction {
    Activate(SettingPath),
    Step { path: SettingPath, delta: i64 },
    Reset(SettingPath),
    EditKey(String),
    RecordChord { key: String, mods: Vec<String> },
    Copy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommitEffect {
    pub path: SettingPath,
    /// `None` removes a dynamic key whose default is absent.
    pub value: Option<Value>,
    /// Secondary paths changed by the canonical cross-field policy.
    pub derived: Vec<DocumentChange>,
    pub mode: ApplyMode,
    pub persistence_json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentChange {
    pub path: SettingPath,
    pub value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DocumentEffect {
    None,
    StateChanged,
    Rejected { path: SettingPath },
    Commit(CommitEffect),
    Copy { lua: String },
}

#[derive(Debug, Clone)]
pub struct SettingsDocument {
    values: Value,
    explicit: BTreeSet<SettingPath>,
    host_values: BTreeSet<String>,
    opaque: BTreeSet<SettingPath>,
    key_defaults: BTreeMap<String, Value>,
    is_macos: bool,
    state: DocumentState,
    issues: Vec<ValidationIssue>,
}

impl SettingsDocument {
    pub fn new(raw: RawSettings) -> Self {
        let mut document = Self {
            values: raw.values,
            explicit: raw.explicit,
            host_values: raw.host_values,
            opaque: raw.opaque,
            key_defaults: raw.key_defaults,
            is_macos: raw.is_macos,
            state: DocumentState::default(),
            issues: Vec::new(),
        };
        document.validate();
        normalize_cross_fields(&mut document.values);
        document
    }

    pub fn values(&self) -> &Value {
        &self.values
    }

    pub fn state(&self) -> &DocumentState {
        &self.state
    }

    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    /// Resolves the display key carried by the existing settings interaction model back to its
    /// segmented path. Open-map children containing dots therefore remain one path segment.
    pub fn path_for_key(&self, key: &str) -> Option<SettingPath> {
        self.fields()
            .into_iter()
            .find_map(|field| (field.path.dotted() == key).then_some(field.path))
    }

    fn validate(&mut self) {
        let schema_defaults = defaults();
        for option in options() {
            let path = SettingPath::from_dotted(option.key);
            if self.opaque.contains(&path) {
                continue;
            }
            let Some(value) = get_at(&self.values, &path).cloned() else {
                continue;
            };
            match canonical_value(option, &value) {
                Some(canonical) => set_at(&mut self.values, &path, Some(canonical)),
                None => {
                    self.issues.push(ValidationIssue {
                        path: path.clone(),
                        reason: "invalid value; reset to the schema default",
                    });
                    set_at(
                        &mut self.values,
                        &path,
                        get_at(&schema_defaults, &path).cloned(),
                    );
                }
            }
        }
    }

    pub fn fields(&self) -> Vec<Field> {
        let defaults = defaults();
        let mut fields = Vec::new();
        for option in options() {
            // Known children of an open descriptor are rendered by that descriptor's expansion;
            // walking them again would duplicate rows such as `theme.elevation`.
            if super::schema::is_open(option.key) {
                continue;
            }
            let path = SettingPath::from_dotted(option.key);
            let value = get_at(&self.values, &path).cloned();
            let default = get_at(&defaults, &path).cloned();
            if !option.container {
                let mut field = self.field(path.clone(), Some(option), value.clone(), default, 0);
                if option.open
                    && let Some(Value::Table(table)) = value
                {
                    let (live, shipped) = self.effective_open(option, &table);
                    field.entries = Some(live.len());
                    if field.widget != Widget::Variant {
                        field.widget = Widget::Entries;
                        field.value_text = entries_text(live.len());
                    }
                    fields.push(field);
                    self.expand(option, live, shipped.as_ref(), &mut fields);
                    continue;
                }
                fields.push(field);
            } else if option.open
                && let Some(Value::Table(table)) = value
            {
                self.expand(option, table, None, &mut fields);
            }
        }
        fields
    }

    fn effective_open(
        &self,
        option: &Descriptor,
        value: &BTreeMap<String, Value>,
    ) -> (BTreeMap<String, Value>, Option<BTreeMap<String, Value>>) {
        if option.key != "keys" {
            return (value.clone(), None);
        }
        let mut effective = self.key_defaults.clone();
        effective.extend(value.clone());
        (effective, Some(self.key_defaults.clone()))
    }

    fn expand(
        &self,
        parent: &Descriptor,
        values: BTreeMap<String, Value>,
        defaults: Option<&BTreeMap<String, Value>>,
        fields: &mut Vec<Field>,
    ) {
        let parent_path = SettingPath::from_dotted(parent.key);
        for (key, value) in values {
            let path = parent_path.child(&key);
            let option = descriptor_for_path(&path);
            let default = defaults
                .and_then(|values| values.get(&key))
                .cloned()
                .or_else(|| get_at(&super::schema::defaults(), &path).cloned());
            fields.push(self.field(path, option, Some(value), default, 1));
        }
    }

    fn field(
        &self,
        path: SettingPath,
        option: Option<&'static Descriptor>,
        value: Option<Value>,
        default: Option<Value>,
        depth: usize,
    ) -> Field {
        let label = if depth == 0 {
            option
                .and_then(|option| option.label)
                .unwrap_or_else(|| path.0.last().map_or("", String::as_str))
                .to_owned()
        } else {
            format!("  {}", path.0.last().map_or("", String::as_str))
        };
        let group = option
            .and_then(|option| option.group)
            .unwrap_or_else(|| group_for_path(&path))
            .to_owned();
        let locked = self.lock_for(&path, option);
        let entries = value.as_ref().and_then(count_entries);
        let widget = widget_for(option, &path, value.as_ref(), locked);
        let armed = self.state.armed.as_ref() == Some(&path);
        let editing = self
            .state
            .editing
            .as_ref()
            .filter(|editing| editing.path == path)
            .map(|editing| editing.buffer.clone());
        let value_text = value_text(widget, value.as_ref(), option, entries, armed, locked);
        let words = option
            .and_then(|option| option.label)
            .unwrap_or_else(|| label.trim_start());
        let help = option
            .and_then(|option| option.help)
            .map_or_else(|| words.to_owned(), |help| format!("{words} — {help}"));
        Field {
            path: path.clone(),
            label,
            group,
            widget,
            value_text,
            changed: value != default,
            value,
            default,
            locked,
            depth,
            help,
            entries,
            editing,
            armed,
            apply_mode: apply_mode(&path, &self.host_values),
        }
    }

    fn lock_for(&self, path: &SettingPath, option: Option<&Descriptor>) -> Option<LockReason> {
        if self.explicit.contains(path) {
            return Some(LockReason::Explicit);
        }
        if host_key(path).is_some_and(|key| self.host_values.contains(key)) {
            return Some(LockReason::Host);
        }
        if self.opaque.iter().any(|opaque| path.starts_with(opaque))
            || option.is_some_and(|option| option.kind == Kind::Function)
        {
            return Some(LockReason::Opaque);
        }
        None
    }

    pub fn groups(&self) -> Vec<Group> {
        let fields = self.fields();
        GROUPS
            .iter()
            .filter(|(id, _)| fields.iter().any(|field| field.group == *id))
            .map(|(id, label)| Group {
                id: (*id).to_owned(),
                label: (*label).to_owned(),
            })
            .collect()
    }

    pub fn caveat(&self) -> Option<&'static [&'static str]> {
        (self.is_macos && self.state.armed.is_some()).then_some(&CAVEAT)
    }

    pub fn changed(&self, include_explicit: bool) -> BTreeMap<String, Value> {
        let empty = BTreeSet::new();
        persistence::changed_values(
            &self.values,
            if include_explicit {
                &empty
            } else {
                &self.explicit
            },
            &self.opaque,
        )
    }

    pub fn clipboard_lua(&self) -> String {
        let changed = Value::Table(self.changed(true));
        let Value::Table(changed) = changed else {
            unreachable!()
        };
        if changed.is_empty() {
            return "vtabs.apply_to_config(config, {})".to_owned();
        }
        format!(
            "vtabs.apply_to_config(config, {})",
            render_lua(&Value::Table(changed), 0)
        )
    }

    pub fn persistence_json(&self) -> Result<String, serde_json::Error> {
        persistence::json_body(&self.values, &self.explicit, &self.opaque)
    }

    pub fn apply(&mut self, action: DocumentAction) -> Result<DocumentEffect, serde_json::Error> {
        match action {
            DocumentAction::Activate(path) => self.activate(&path),
            DocumentAction::Step { path, delta } => self.step(&path, delta),
            DocumentAction::Reset(path) => {
                self.state.armed = None;
                let Some(field) = self.row(&path) else {
                    return Ok(DocumentEffect::None);
                };
                if field.locked.is_some() {
                    return Ok(DocumentEffect::None);
                }
                self.commit(path, field.default)
            }
            DocumentAction::EditKey(key) => self.edit_key(&key),
            DocumentAction::RecordChord { key, mods } => self.record_chord(&key, &mods),
            DocumentAction::Copy => Ok(DocumentEffect::Copy {
                lua: self.clipboard_lua(),
            }),
        }
    }

    fn activate(&mut self, path: &SettingPath) -> Result<DocumentEffect, serde_json::Error> {
        let Some(field) = self.row(path) else {
            return Ok(DocumentEffect::None);
        };
        if field.locked.is_some() {
            return Ok(DocumentEffect::None);
        }
        match (field.widget, field.value) {
            (Widget::Toggle, Some(Value::Bool(value))) => {
                self.commit(path.clone(), Some(Value::Bool(!value)))
            }
            (Widget::Variant, Some(Value::Bool(value))) => {
                self.commit(path.clone(), Some(Value::Bool(!value)))
            }
            (Widget::Text | Widget::Colour, Some(value)) => {
                self.state.editing = Some(Editing {
                    path: path.clone(),
                    buffer: display_scalar(&value),
                });
                Ok(DocumentEffect::StateChanged)
            }
            (Widget::Recorder, _) => {
                self.state.armed = Some(path.clone());
                Ok(DocumentEffect::StateChanged)
            }
            _ => Ok(DocumentEffect::None),
        }
    }

    fn step(
        &mut self,
        path: &SettingPath,
        delta: i64,
    ) -> Result<DocumentEffect, serde_json::Error> {
        let Some(field) = self.row(path) else {
            return Ok(DocumentEffect::None);
        };
        if field.locked.is_some() {
            return Ok(DocumentEffect::None);
        }
        let next = match (field.widget, field.value.as_ref()) {
            (Widget::Toggle, Some(Value::Bool(value))) => Some(Value::Bool(!value)),
            (Widget::Picker, Some(value)) => field
                .option()
                .and_then(|option| cycle(&option.allowed, value, delta)),
            (Widget::Stepper, Some(Value::Number(value))) => {
                let option = field.option();
                let mut next = *value + delta as f64;
                if option.is_some_and(|option| option.integer) {
                    next = (next + 0.5).floor();
                }
                if let Some(min) = option.and_then(|option| option.min) {
                    next = next.max(min);
                }
                if let Some(max) = option.and_then(|option| option.max) {
                    next = next.min(max);
                }
                Some(Value::Number(next))
            }
            (Widget::Variant, Some(value @ Value::Bool(_))) => {
                cycle(&[Value::Bool(false), Value::Bool(true)], value, delta)
            }
            _ => None,
        };
        match next {
            Some(value) if Some(&value) != field.value.as_ref() => {
                self.commit(path.clone(), Some(value))
            }
            _ => Ok(DocumentEffect::None),
        }
    }

    fn edit_key(&mut self, key: &str) -> Result<DocumentEffect, serde_json::Error> {
        let Some(mut editing) = self.state.editing.take() else {
            return Ok(DocumentEffect::None);
        };
        match key {
            "escape" => Ok(DocumentEffect::StateChanged),
            "enter" => self.commit(editing.path, Some(Value::String(editing.buffer))),
            "backspace" => {
                if let Some((at, _)) = editing.buffer.grapheme_indices(true).next_back() {
                    editing.buffer.truncate(at);
                }
                self.state.editing = Some(editing);
                Ok(DocumentEffect::StateChanged)
            }
            _ if key.graphemes(true).count() == 1 => {
                editing.buffer.push_str(key);
                self.state.editing = Some(editing);
                Ok(DocumentEffect::StateChanged)
            }
            _ => {
                self.state.editing = Some(editing);
                Ok(DocumentEffect::None)
            }
        }
    }

    fn record_chord(
        &mut self,
        key: &str,
        mods: &[String],
    ) -> Result<DocumentEffect, serde_json::Error> {
        let Some(path) = self.state.armed.take() else {
            return Ok(DocumentEffect::None);
        };
        if key == "escape" {
            return Ok(DocumentEffect::StateChanged);
        }
        let mut binding = BTreeMap::from([("key".to_owned(), Value::String(key.to_owned()))]);
        if !mods.is_empty() {
            binding.insert("mods".to_owned(), Value::String(mods.join("|")));
        }
        self.commit(path, Some(Value::Table(binding)))
    }

    fn commit(
        &mut self,
        path: SettingPath,
        value: Option<Value>,
    ) -> Result<DocumentEffect, serde_json::Error> {
        let value = match (descriptor_for_path(&path), value) {
            (Some(option), Some(value)) => match canonical_value(option, &value) {
                Some(value) => Some(value),
                None => return Ok(DocumentEffect::Rejected { path }),
            },
            (_, value) => value,
        };
        set_at(&mut self.values, &path, value.clone());
        let changed = normalize_cross_fields(&mut self.values);
        let value = get_at(&self.values, &path).cloned();
        let derived = changed
            .into_iter()
            .filter(|changed| changed != &path)
            .map(|changed| DocumentChange {
                value: get_at(&self.values, &changed).cloned(),
                path: changed,
            })
            .collect();
        let effect = CommitEffect {
            mode: apply_mode(&path, &self.host_values),
            path,
            value,
            derived,
            persistence_json: self.persistence_json()?,
        };
        Ok(DocumentEffect::Commit(effect))
    }

    fn row(&self, path: &SettingPath) -> Option<Field> {
        self.fields().into_iter().find(|field| field.path == *path)
    }
}

impl Default for RawSettings {
    fn default() -> Self {
        Self {
            values: defaults(),
            explicit: BTreeSet::new(),
            host_values: BTreeSet::new(),
            opaque: BTreeSet::new(),
            key_defaults: BTreeMap::new(),
            is_macos: false,
        }
    }
}

impl Field {
    fn option(&self) -> Option<&'static Descriptor> {
        descriptor_for_path(&self.path)
    }
}

fn descriptor_for_path(path: &SettingPath) -> Option<&'static Descriptor> {
    let option = by_key(&path.dotted())?;
    (SettingPath::from_dotted(option.key) == *path).then_some(option)
}

fn group_for_path(path: &SettingPath) -> &'static str {
    let Some(parent) = path.0.first() else {
        return "behaviour";
    };
    by_key(parent)
        .and_then(|option| option.group)
        .unwrap_or("behaviour")
}

fn count_entries(value: &Value) -> Option<usize> {
    match value {
        Value::List(values) => Some(values.len()),
        Value::Table(values) => Some(values.len()),
        _ => None,
    }
}

fn entries_text(entries: usize) -> String {
    if entries == 1 {
        "1 entry".to_owned()
    } else {
        format!("{entries} entries")
    }
}

fn looks_like_colour(path: &SettingPath, value: Option<&Value>) -> bool {
    let key = path.0.last().map_or("", String::as_str);
    matches!(value, Some(Value::String(value)) if value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit()))
        || [
            "_fg",
            "_bg",
            "accent",
            "border",
            "border_idle",
            "separator",
            "split",
        ]
        .iter()
        .any(|suffix| key.ends_with(suffix))
}

fn widget_for(
    option: Option<&Descriptor>,
    path: &SettingPath,
    value: Option<&Value>,
    locked: Option<LockReason>,
) -> Widget {
    if option.is_some_and(|option| option.kind == Kind::Function)
        || locked == Some(LockReason::Opaque)
    {
        return Widget::Locked;
    }
    match option.map(|option| option.kind) {
        Some(Kind::Boolean) => return Widget::Toggle,
        Some(Kind::Enum) => return Widget::Picker,
        Some(Kind::Number) => return Widget::Stepper,
        Some(Kind::Any) => return Widget::Variant,
        _ => {}
    }
    if path.0.first().is_some_and(|part| part == "keys") && path.0.len() > 1 {
        return Widget::Recorder;
    }
    match value {
        Some(Value::Bool(_)) => Widget::Toggle,
        Some(Value::Number(_)) => Widget::Stepper,
        Some(Value::List(_) | Value::Table(_)) => Widget::Entries,
        Some(Value::String(_)) if looks_like_colour(path, value) => Widget::Colour,
        Some(Value::String(_)) => Widget::Text,
        _ => Widget::Locked,
    }
}

fn value_text(
    widget: Widget,
    value: Option<&Value>,
    _option: Option<&Descriptor>,
    entries: Option<usize>,
    armed: bool,
    locked: Option<LockReason>,
) -> String {
    match widget {
        Widget::Toggle => match value {
            Some(Value::Bool(true)) => "[ on ]".to_owned(),
            _ => "[ off ]".to_owned(),
        },
        Widget::Picker | Widget::Stepper => {
            format!(
                "‹ {} ›",
                value.map_or_else(|| "nil".to_owned(), display_scalar)
            )
        }
        Widget::Colour => format!(
            "██ {}",
            value.map_or_else(|| "nil".to_owned(), display_scalar)
        ),
        Widget::Text => value.map_or_else(
            || "\"nil\"".to_owned(),
            |value| lua_string(&display_scalar(value)),
        ),
        Widget::Recorder => {
            if armed {
                return "press a key…   [ ARMED  ]".to_owned();
            }
            let binding = match value {
                Some(Value::Table(binding)) => {
                    let key = binding
                        .get("key")
                        .map_or_else(|| "nil".to_owned(), display_scalar);
                    let mods = binding.get("mods").map(display_scalar);
                    mods.map_or(key.clone(), |mods| format!("{mods}+{key}"))
                }
                Some(Value::Bool(false)) | None => "off".to_owned(),
                Some(value) => display_scalar(value),
            };
            format!("{binding}   [ record ]")
        }
        Widget::Variant => {
            let name = match value {
                Some(Value::Bool(false)) => "off",
                Some(Value::Bool(true)) => "on",
                _ => "custom",
            };
            format!("‹ {name} ›")
        }
        Widget::Entries => entries_text(entries.unwrap_or(0)),
        Widget::Locked if locked == Some(LockReason::Opaque) => "fun()".to_owned(),
        Widget::Locked => value.map_or_else(|| "nil".to_owned(), display_scalar),
    }
}

fn cycle(values: &[Value], current: &Value, delta: i64) -> Option<Value> {
    if values.is_empty() {
        return None;
    }
    let at = values
        .iter()
        .position(|value| value == current)
        .unwrap_or(0) as i64;
    Some(values[(at + delta).rem_euclid(values.len() as i64) as usize].clone())
}

fn host_key(path: &SettingPath) -> Option<&'static str> {
    policy_for(&path.dotted()).and_then(|option| option.host_key)
}

fn apply_mode(path: &SettingPath, host_values: &BTreeSet<String>) -> ApplyMode {
    let Some(option) = policy_for(&path.dotted()) else {
        return ApplyMode::Instant;
    };
    if option.host_key.is_some_and(|key| host_values.contains(key)) {
        return ApplyMode::Instant;
    }
    option.apply_mode
}

fn display_scalar(value: &Value) -> String {
    match value {
        Value::Null => "nil".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) if value.fract() == 0.0 => format!("{}", *value as i64),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::List(_) | Value::Table(_) => "table".to_owned(),
    }
}

fn lua_string(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\x{:02x}", character as u32))
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn render_lua(value: &Value, indent: usize) -> String {
    match value {
        Value::Null => "nil".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) if value.fract() == 0.0 => format!("{}", *value as i64),
        Value::Number(value) => value.to_string(),
        Value::String(value) => lua_string(value),
        Value::List(values) => render_lua_entries(
            values
                .iter()
                .map(|value| render_lua(value, indent + 2))
                .collect(),
            indent,
        ),
        Value::Table(values) => render_lua_entries(
            values
                .iter()
                .map(|(key, value)| {
                    format!("[{}] = {}", lua_string(key), render_lua(value, indent + 2))
                })
                .collect(),
            indent,
        ),
    }
}

fn render_lua_entries(entries: Vec<String>, indent: usize) -> String {
    if entries.is_empty() {
        return "{}".to_owned();
    }
    let pad = " ".repeat(indent + 2);
    let close = " ".repeat(indent);
    format!(
        "{{\n{pad}{},\n{close}}}",
        entries.join(&format!(",\n{pad}"))
    )
}

const GROUPS: [(&str, &str); 9] = [
    ("layout", "Layout"),
    ("cards", "Cards"),
    ("chrome", "Chrome"),
    ("behaviour", "Behaviour"),
    ("theme", "Theme"),
    ("identity", "Identity"),
    ("hooks", "Hooks"),
    ("backend", "Backend"),
    ("spaces", "Spaces"),
];

const CAVEAT: [&str; 3] = [
    "⚠ macOS does not deliver CMD to the pty.",
    "  Type it into wezterm.lua, or avoid CMD.",
    "  enable_kitty_keyboard widens what arrives",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::value::{get_at, set_at, set_path, table};

    fn path(key: &str) -> SettingPath {
        SettingPath::from_dotted(key)
    }

    fn field<'a>(fields: &'a [Field], key: &str) -> &'a Field {
        fields
            .iter()
            .find(|field| field.path == path(key))
            .unwrap_or_else(|| panic!("missing field {key}"))
    }

    fn committed(effect: DocumentEffect) -> CommitEffect {
        match effect {
            DocumentEffect::Commit(effect) => effect,
            effect => panic!("expected commit, got {effect:?}"),
        }
    }

    #[test]
    fn builds_schema_ordered_groups_fields_widgets_and_opaque_hooks() {
        let document = SettingsDocument::new(RawSettings::default());
        let fields = document.fields();
        assert_eq!(fields[0].path, path("width"));
        assert_eq!(field(&fields, "width").widget, Widget::Stepper);
        assert_eq!(field(&fields, "width").value_text, "‹ 28 ›");
        assert_eq!(field(&fields, "position").widget, Widget::Picker);
        assert_eq!(field(&fields, "debug").widget, Widget::Toggle);
        assert_eq!(
            fields
                .iter()
                .filter(|field| field.path == path("theme.elevation"))
                .count(),
            1
        );

        let hook = field(&fields, "hooks.filter");
        assert_eq!(hook.widget, Widget::Locked);
        assert_eq!(hook.locked, Some(LockReason::Opaque));
        assert_eq!(hook.value_text, "fun()");

        assert_eq!(
            document
                .groups()
                .into_iter()
                .map(|group| group.id)
                .collect::<Vec<_>>(),
            [
                "layout",
                "cards",
                "chrome",
                "behaviour",
                "theme",
                "identity",
                "hooks",
                "backend",
                "spaces",
            ]
        );
    }

    #[test]
    fn resets_invalid_known_values() {
        let mut raw = RawSettings::default();
        set_path(&mut raw.values, "tab_height", 3.into());
        set_path(&mut raw.values, "width", 4.into());
        let document = SettingsDocument::new(raw);
        assert_eq!(
            get_at(document.values(), &path("tab_height")),
            Some(&Value::String("card".to_owned()))
        );
        assert_eq!(
            get_at(document.values(), &path("width")),
            Some(&Value::Number(28.0))
        );
        assert_eq!(document.issues().len(), 2);
        assert!(
            document
                .issues()
                .iter()
                .any(|issue| issue.path == path("tab_height"))
        );
        assert!(
            document
                .issues()
                .iter()
                .any(|issue| issue.path == path("width"))
        );
    }

    #[test]
    fn actions_own_activation_stepping_reset_and_commit_effects() {
        let mut document = SettingsDocument::new(RawSettings::default());
        let effect = committed(
            document
                .apply(DocumentAction::Step {
                    path: path("width"),
                    delta: 1,
                })
                .unwrap(),
        );
        assert_eq!(effect.path, path("width"));
        assert_eq!(effect.value, Some(Value::Number(29.0)));
        assert_eq!(effect.mode, ApplyMode::Instant);
        assert!(effect.persistence_json.contains(r#""width":29.0"#));
        assert!(effect.derived.is_empty());

        let effect = committed(
            document
                .apply(DocumentAction::Activate(path("debug")))
                .unwrap(),
        );
        assert_eq!(effect.value, Some(Value::Bool(true)));

        let effect = committed(
            document
                .apply(DocumentAction::Step {
                    path: path("edge_to_edge"),
                    delta: 1,
                })
                .unwrap(),
        );
        assert_eq!(effect.value, Some(Value::Bool(false)));
        assert_eq!(effect.mode, ApplyMode::Reload);

        let effect = committed(
            document
                .apply(DocumentAction::Reset(path("width")))
                .unwrap(),
        );
        assert_eq!(effect.value, Some(Value::Number(28.0)));
    }

    #[test]
    fn cross_field_changes_stay_in_the_document_persistence_and_commit_effect() {
        let mut document = SettingsDocument::new(RawSettings::default());
        let effect = committed(
            document
                .apply(DocumentAction::Step {
                    path: path("hover"),
                    delta: 1,
                })
                .unwrap(),
        );
        assert_eq!(effect.value, Some(Value::String("press".into())));
        assert_eq!(
            effect.derived,
            vec![DocumentChange {
                path: path("close_button"),
                value: Some(Value::String("always".into())),
            }]
        );
        assert_eq!(
            get_at(document.values(), &path("close_button")),
            Some(&Value::String("always".into()))
        );
        assert!(
            effect
                .persistence_json
                .contains(r#""close_button":"always""#)
        );
    }

    #[test]
    fn descriptor_policies_preserve_live_modes_and_host_ownership() {
        let host_values = BTreeSet::new();
        for (key, mode) in [
            ("width", ApplyMode::Instant),
            ("frame.margin", ApplyMode::Override),
            ("frame.border", ApplyMode::Instant),
            ("theme.split", ApplyMode::Override),
            ("edge_to_edge", ApplyMode::Reload),
            ("titlebar", ApplyMode::Reload),
            ("backend.uservar", ApplyMode::Reload),
        ] {
            assert_eq!(apply_mode(&path(key), &host_values), mode, "{key}");
        }

        let owned = BTreeSet::from(["window_padding".to_owned()]);
        assert_eq!(apply_mode(&path("frame.inset"), &owned), ApplyMode::Instant);
        assert_eq!(
            apply_mode(&path("edge_to_edge"), &owned),
            ApplyMode::Instant
        );
        assert_eq!(host_key(&path("frame.border")), None);
    }

    #[test]
    fn editing_backspace_removes_a_complete_grapheme_and_enter_commits() {
        let mut document = SettingsDocument::new(RawSettings::default());
        assert_eq!(
            document
                .apply(DocumentAction::Activate(path("meta_sep")))
                .unwrap(),
            DocumentEffect::StateChanged
        );
        document
            .apply(DocumentAction::EditKey("👩‍👩‍👧‍👧".to_owned()))
            .unwrap();
        assert_eq!(document.state().editing.as_ref().unwrap().buffer, "  👩‍👩‍👧‍👧");
        document
            .apply(DocumentAction::EditKey("backspace".to_owned()))
            .unwrap();
        assert_eq!(document.state().editing.as_ref().unwrap().buffer, "  ");
        let effect = committed(
            document
                .apply(DocumentAction::EditKey("enter".to_owned()))
                .unwrap(),
        );
        assert_eq!(effect.value, Some(Value::String("  ".to_owned())));
        assert!(document.state().editing.is_none());
    }

    #[test]
    fn recorder_uses_segmented_dynamic_key_paths() {
        let binding = table([("key", "o".into()), ("mods", "CTRL".into())]);
        let mut raw = RawSettings::default();
        raw.key_defaults
            .insert("open.file".to_owned(), binding.clone());
        let dynamic = SettingPath(vec!["keys".to_owned(), "open.file".to_owned()]);
        let mut document = SettingsDocument::new(raw);
        let row = document
            .fields()
            .into_iter()
            .find(|field| field.path == dynamic)
            .expect("dynamic binding row");
        assert_eq!(row.widget, Widget::Recorder);
        assert_eq!(row.default, Some(binding));

        assert_eq!(
            document
                .apply(DocumentAction::Activate(dynamic.clone()))
                .unwrap(),
            DocumentEffect::StateChanged
        );
        assert_eq!(document.state().armed, Some(dynamic.clone()));
        let effect = committed(
            document
                .apply(DocumentAction::RecordChord {
                    key: "p".to_owned(),
                    mods: vec!["CTRL".to_owned(), "SHIFT".to_owned()],
                })
                .unwrap(),
        );
        assert_eq!(effect.path, dynamic.clone());
        assert_eq!(
            get_at(document.values(), &dynamic),
            Some(&table([("key", "p".into()), ("mods", "CTRL|SHIFT".into())]))
        );
        assert!(document.state().armed.is_none());
    }

    #[test]
    fn clipboard_is_valid_lua_shape_for_lists_empty_tables_and_arbitrary_keys() {
        let mut raw = RawSettings::default();
        set_path(
            &mut raw.values,
            "strip_actions",
            Value::List(vec!["search".into()]),
        );
        set_path(&mut raw.values, "frame", Value::Table(BTreeMap::new()));
        set_at(
            &mut raw.values,
            &SettingPath(vec![
                "backend".to_owned(),
                "env".to_owned(),
                "A.B-with dash".to_owned(),
            ]),
            Some("one\ntwo".into()),
        );
        let lua = SettingsDocument::new(raw).clipboard_lua();
        assert!(lua.starts_with("vtabs.apply_to_config(config, {"));
        assert!(lua.contains(r#"["A.B-with dash"] = "one\ntwo""#));
        assert!(lua.contains(r#"["frame"] = {}"#));
        assert!(lua.contains("\"search\","));
        assert!(!lua.contains("1 = \"search\""));
    }

    #[test]
    fn persistence_omits_explicit_values_while_clipboard_includes_them() {
        let mut raw = RawSettings::default();
        set_path(&mut raw.values, "width", 42.into());
        raw.explicit.insert(path("width"));
        let document = SettingsDocument::new(raw);
        assert!(!document.persistence_json().unwrap().contains("width"));
        assert!(document.clipboard_lua().contains(r#"["width"] = 42"#));
        assert_eq!(
            field(&document.fields(), "width").locked,
            Some(LockReason::Explicit)
        );
    }

    #[test]
    fn explicit_leaf_locks_do_not_lock_structural_siblings() {
        let mut raw = RawSettings::default();
        raw.explicit.insert(path("padding.top"));
        let document = SettingsDocument::new(raw);
        let fields = document.fields();
        assert_eq!(
            field(&fields, "padding.top").locked,
            Some(LockReason::Explicit)
        );
        assert_eq!(field(&fields, "padding.left").locked, None);
    }

    #[test]
    fn opaque_callback_values_never_enter_persistence_or_clipboard() {
        let mut raw = RawSettings::default();
        set_path(
            &mut raw.values,
            "backend.path",
            "/serializable-projection".into(),
        );
        raw.opaque.insert(path("backend.path"));
        let document = SettingsDocument::new(raw);
        let fields = document.fields();
        let row = field(&fields, "backend.path");
        assert_eq!(row.locked, Some(LockReason::Opaque));
        assert_eq!(row.widget, Widget::Locked);
        assert_eq!(row.value_text, "fun()");
        assert!(!document.persistence_json().unwrap().contains("backend"));
        assert!(!document.clipboard_lua().contains("backend"));
    }

    #[test]
    fn macos_recorder_caveat_only_exists_while_armed() {
        let mut raw = RawSettings {
            is_macos: true,
            ..RawSettings::default()
        };
        raw.key_defaults
            .insert("open".to_owned(), table([("key", "o".into())]));
        let mut document = SettingsDocument::new(raw);
        assert_eq!(document.caveat(), None);
        document
            .apply(DocumentAction::Activate(SettingPath(vec![
                "keys".to_owned(),
                "open".to_owned(),
            ])))
            .unwrap();
        assert_eq!(document.caveat(), Some(CAVEAT.as_slice()));
    }
}

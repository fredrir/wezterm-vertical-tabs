use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::schema::{Kind, by_key, canonical_value, defaults, options};
use super::value::{SettingPath, Value, get_path, set_at};

pub const VERSION: u32 = 1;

#[derive(Serialize)]
struct Output<'a> {
    version: u32,
    options: &'a BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct Input {
    version: u32,
    options: Value,
}

/// The usable part of a persistence-v1 document plus non-fatal diagnostics for the host.
#[derive(Debug, PartialEq)]
pub struct Loaded {
    pub values: Value,
    pub warnings: Vec<String>,
}

impl Default for Loaded {
    fn default() -> Self {
        Self {
            values: Value::Table(BTreeMap::new()),
            warnings: Vec::new(),
        }
    }
}

/// Parses, filters, and migrates a persistence-v1 body. A broken file is an empty overlay rather
/// than a startup error: user configuration and defaults must still be usable.
pub fn parse_v1(body: &str) -> Loaded {
    let mut loaded = Loaded::default();
    let input: Input = match serde_json::from_str(body) {
        Ok(input) => input,
        Err(_) => {
            loaded
                .warnings
                .push("settings file unreadable, starting from the defaults".into());
            return loaded;
        }
    };
    if input.version != VERSION {
        loaded.warnings.push(format!(
            "settings file version {} ignored (expected {VERSION})",
            input.version
        ));
        return loaded;
    }
    let Value::Table(options) = input.options else {
        loaded
            .warnings
            .push("settings file options must be an object, ignored".into());
        return loaded;
    };
    loaded.values = Value::Table(filter_table(
        &options,
        &SettingPath(Vec::new()),
        &mut loaded.warnings,
    ));
    migrate_aliases(&mut loaded.values);
    loaded
}

fn filter_table(
    table: &BTreeMap<String, Value>,
    prefix: &SettingPath,
    warnings: &mut Vec<String>,
) -> BTreeMap<String, Value> {
    let mut kept = BTreeMap::new();
    for (key, value) in table {
        let path = prefix.child(key);
        let dotted = path.dotted();
        let filtered = match by_key(&dotted) {
            Some(option) if option.open || option.kind == Kind::List => Some(value.clone()),
            Some(_) => match value {
                Value::Table(children) => {
                    Some(Value::Table(filter_table(children, &path, warnings)))
                }
                value => Some(value.clone()),
            },
            None if super::schema::is_open(&dotted) => Some(value.clone()),
            None => {
                warnings.push(format!("settings file: dropped unknown {dotted}"));
                None
            }
        };
        if let Some(value) = filtered {
            kept.insert(key.clone(), value);
        }
    }
    kept
}

fn migrate_aliases(values: &mut Value) {
    for option in options() {
        let Some(value) = get_path(values, option.key).cloned() else {
            continue;
        };
        let Some((_, replacement)) = option.aliases.iter().find(|(alias, _)| *alias == value)
        else {
            continue;
        };
        set_at(
            values,
            &SettingPath::from_dotted(option.key),
            Some(replacement.clone()),
        );
    }
}

/// Returns only serializable, changed values, excluding config-as-code keys and their children.
pub fn changed_values(
    resolved: &Value,
    explicit: &BTreeSet<SettingPath>,
    opaque: &BTreeSet<SettingPath>,
) -> BTreeMap<String, Value> {
    let defaults = defaults();
    let mut changed = Value::Table(BTreeMap::new());
    for option in options() {
        if option.kind == Kind::Function || has_open_ancestor(option.key) {
            continue;
        }
        let Some(value) = get_path(resolved, option.key) else {
            continue;
        };
        let default = get_path(&defaults, option.key);
        let difference = if option.open {
            diff_open(
                &SettingPath::from_dotted(option.key),
                value,
                default,
                explicit,
                opaque,
            )
        } else if option.container
            || explicit.contains(&SettingPath::from_dotted(option.key))
            || opaque_locked(&SettingPath::from_dotted(option.key), opaque)
            || default == Some(value)
        {
            None
        } else {
            canonical_value(option, value)
        };
        if let Some(value) = difference {
            set_at(
                &mut changed,
                &SettingPath::from_dotted(option.key),
                Some(value),
            );
        }
    }
    match changed {
        Value::Table(table) => table,
        _ => unreachable!("changed values always have a table root"),
    }
}

fn has_open_ancestor(key: &str) -> bool {
    let mut prefix = String::new();
    for part in key.split('.') {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(part);
        if prefix != key && by_key(&prefix).is_some_and(|option| option.open) {
            return true;
        }
    }
    false
}

fn opaque_locked(path: &SettingPath, opaque: &BTreeSet<SettingPath>) -> bool {
    opaque.iter().any(|lock| path.starts_with(lock))
}

fn diff_open(
    path: &SettingPath,
    value: &Value,
    default: Option<&Value>,
    explicit: &BTreeSet<SettingPath>,
    opaque: &BTreeSet<SettingPath>,
) -> Option<Value> {
    if opaque_locked(path, opaque) || default == Some(value) {
        return None;
    }
    let explicitly_locked = explicit.contains(path);
    match value {
        Value::Table(table) => {
            let descriptor_is_open = by_key(&path.dotted())
                .is_some_and(|option| option.open && SettingPath::from_dotted(option.key) == *path);
            if explicitly_locked && !descriptor_is_open {
                return None;
            }
            let defaults = default.and_then(Value::as_table);
            let mut changed = BTreeMap::new();
            for (key, value) in table {
                let child = path.child(key);
                let default = defaults.and_then(|table| table.get(key));
                if let Some(value) = diff_open(&child, value, default, explicit, opaque) {
                    changed.insert(key.clone(), value);
                }
            }
            (!changed.is_empty() || (table.is_empty() && !explicitly_locked))
                .then_some(Value::Table(changed))
        }
        value if !explicitly_locked => Some(value.clone()),
        _ => None,
    }
}

/// Produces the complete deterministic version-1 JSON body for Lua's later atomic write.
pub fn json_body(
    resolved: &Value,
    explicit: &BTreeSet<SettingPath>,
    opaque: &BTreeSet<SettingPath>,
) -> Result<String, serde_json::Error> {
    let options = changed_values(resolved, explicit, opaque);
    serde_json::to_string(&Output {
        version: VERSION,
        options: &options,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::value::{Value, set_path, table};

    #[test]
    fn changed_values_omit_defaults_and_explicit_keys() {
        let mut resolved = defaults();
        set_path(&mut resolved, "width", 42.into());
        set_path(&mut resolved, "row_gap", 2.into());
        set_path(&mut resolved, "theme.accent", "#ff0000".into());
        set_path(&mut resolved, "frame", Value::Table(BTreeMap::new()));
        let explicit = BTreeSet::from([SettingPath::from_dotted("width")]);
        let changed = changed_values(&resolved, &explicit, &BTreeSet::new());
        let changed = Value::Table(changed);
        assert_eq!(get_path(&changed, "width"), None);
        assert_eq!(get_path(&changed, "row_gap"), Some(&Value::Number(2.0)));
        assert_eq!(
            get_path(&changed, "theme.accent"),
            Some(&Value::String("#ff0000".into()))
        );
        assert_eq!(get_path(&changed, "theme.elevation"), None);
        assert_eq!(
            get_path(&changed, "frame"),
            Some(&Value::Table(BTreeMap::new())),
            "an empty frame table is observably different from false"
        );
    }

    #[test]
    fn final_body_is_compact_deterministic_and_versioned() {
        let resolved = table([("width", 40.into()), ("row_gap", 1.into())]);
        assert_eq!(
            json_body(&resolved, &BTreeSet::new(), &BTreeSet::new()).unwrap(),
            r#"{"version":1,"options":{"row_gap":1.0,"width":40.0}}"#
        );
    }

    #[test]
    fn exact_leaf_locks_do_not_hide_editable_siblings_inside_tables() {
        let mut resolved = defaults();
        set_path(&mut resolved, "padding.top", 2.into());
        set_path(&mut resolved, "padding.left", 4.into());
        set_path(
            &mut resolved,
            "keys",
            table([
                ("new_tab", table([("key", "x".into())])),
                ("other", false.into()),
            ]),
        );
        set_path(
            &mut resolved,
            "frame",
            table([("zen", true.into()), ("margin", 9.into())]),
        );
        let explicit = BTreeSet::from([
            SettingPath::from_dotted("padding"),
            SettingPath::from_dotted("padding.top"),
            SettingPath::from_dotted("keys"),
            SettingPath(vec!["keys".into(), "new_tab".into()]),
            SettingPath::from_dotted("frame"),
            SettingPath(vec!["frame".into(), "zen".into()]),
        ]);
        let changed = Value::Table(changed_values(&resolved, &explicit, &BTreeSet::new()));
        assert_eq!(get_path(&changed, "padding.top"), None);
        assert_eq!(
            get_path(&changed, "padding.left"),
            Some(&Value::Number(4.0))
        );
        assert_eq!(get_path(&changed, "keys.other"), Some(&Value::Bool(false)));
        assert_eq!(get_path(&changed, "keys.new_tab"), None);
        assert_eq!(get_path(&changed, "frame.zen"), None);
        assert_eq!(
            get_path(&changed, "frame.margin"),
            Some(&Value::Number(9.0))
        );
    }

    #[test]
    fn persistence_v1_parses_filters_closed_keys_and_preserves_open_maps_and_lists() {
        let loaded = parse_v1(
            r#"{"version":1,"options":{"bogus":1,"padding":{"top":3,"bogus":4},"backend":{"env":{"A.B":"one"}},"strip_actions":[],"spaces":[{"id":"work"}]}}"#,
        );
        assert_eq!(get_path(&loaded.values, "bogus"), None);
        assert_eq!(get_path(&loaded.values, "padding.bogus"), None);
        assert_eq!(get_path(&loaded.values, "padding.top"), Some(&3.into()));
        assert_eq!(
            get_path(&loaded.values, "backend.env.A.B"),
            None,
            "a dotted open-map key is one opaque segment, not a dotted descriptor path"
        );
        assert_eq!(
            get_path(&loaded.values, "backend.env")
                .and_then(Value::as_table)
                .and_then(|table| table.get("A.B")),
            Some(&Value::String("one".into()))
        );
        assert_eq!(
            get_path(&loaded.values, "strip_actions"),
            Some(&Value::List(vec![]))
        );
        assert!(matches!(
            get_path(&loaded.values, "spaces"),
            Some(Value::List(_))
        ));
        assert_eq!(loaded.warnings.len(), 2);
    }

    #[test]
    fn persistence_v1_migrates_aliases() {
        let loaded = parse_v1(r#"{"version":1,"options":{"tab_height":2,"tooltip":true}}"#);
        assert_eq!(get_path(&loaded.values, "tab_height"), Some(&"card".into()));
        assert_eq!(get_path(&loaded.values, "tooltip"), Some(&"on".into()));
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn persistence_v1_rejects_corrupt_versions_and_non_object_options() {
        for body in [
            "not json",
            r#"{"version":2,"options":{}}"#,
            r#"{"version":1,"options":[]}"#,
        ] {
            let loaded = parse_v1(body);
            assert_eq!(loaded.values, Value::Table(BTreeMap::new()));
            assert_eq!(loaded.warnings.len(), 1);
        }
    }
}

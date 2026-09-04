use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::schema::{Kind, by_key, canonical_value, defaults, options};
use super::value::{SettingPath, Value, get_path, set_at};

#[derive(Serialize)]
struct Output<'a> {
    options: &'a BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    options: Value,
}

/// The usable part of a persisted settings document plus non-fatal diagnostics for the host.
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

/// Parses and filters a persisted body. A broken file is an empty overlay rather than a startup
/// error: user configuration and defaults must still be usable.
pub fn parse(body: &str) -> Loaded {
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

/// Produces the complete deterministic JSON body for Lua's later atomic write.
pub fn json_body(
    resolved: &Value,
    explicit: &BTreeSet<SettingPath>,
    opaque: &BTreeSet<SettingPath>,
) -> Result<String, serde_json::Error> {
    let options = changed_values(resolved, explicit, opaque);
    serde_json::to_string(&Output { options: &options })
}

#[cfg(test)]
#[path = "../../tests/unit/settings/persistence.rs"]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::persistence;
use super::schema::{by_key, canonical_value, defaults, is_open, options};
use super::value::{SettingPath, Value, get_path, set_at, set_path};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizeRequest {
    pub plugin_version: String,
    pub schema_id: String,
    #[serde(default)]
    pub persisted: Option<String>,
    pub opts: Value,
    #[serde(default)]
    pub explicit: Vec<SettingPath>,
    #[serde(default)]
    pub invalid: Vec<SettingPath>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct NormalizeResponse {
    pub plugin_version: &'static str,
    pub schema_id: String,
    pub values: Value,
    pub warnings: Vec<String>,
}

/// Resolves the serializable boot configuration as defaults < persisted < wezterm.lua options.
/// Opaque Lua values are intentionally absent and are restored by the host after this returns.
pub fn normalize(request: NormalizeRequest) -> Result<NormalizeResponse, &'static str> {
    if request.plugin_version != env!("CARGO_PKG_VERSION")
        || request.schema_id != super::schema::identity()
    {
        return Err("normalizer contract mismatch");
    }
    let Value::Table(_) = request.opts else {
        return Err("opts must be an object");
    };
    let mut warnings = Vec::new();
    let stored = request.persisted.map_or_else(
        || Value::Table(BTreeMap::new()),
        |body| {
            let loaded = persistence::parse(&body);
            warnings.extend(loaded.warnings);
            loaded.values
        },
    );
    for path in &request.explicit {
        let dotted = path.dotted();
        if by_key(&dotted).is_none()
            && !is_open(&dotted)
            && !has_unknown_ancestor(path, &request.explicit)
        {
            warnings.push(format!("unknown option {dotted}"));
        }
    }

    let mut values = merge(defaults(), stored);
    values = merge(values, request.opts);
    prune_unknown(&mut values, &SettingPath(vec![]), &mut warnings);
    let schema_defaults = defaults();
    for path in &request.invalid {
        warnings.push(format!("invalid {}, using default", path.dotted()));
        set_at(&mut values, path, get_path_at(&schema_defaults, path));
    }
    validate(&mut values, &mut warnings);
    let cross_changes = normalize_cross_fields(&mut values);
    if cross_changes
        .iter()
        .any(|path| path == &SettingPath::from_dotted("popover.width"))
    {
        warnings.push("popover.width must be \"auto\" or a whole number, using auto".into());
    }
    Ok(NormalizeResponse {
        plugin_version: env!("CARGO_PKG_VERSION"),
        schema_id: super::schema::identity(),
        values,
        warnings,
    })
}

fn get_path_at(values: &Value, path: &SettingPath) -> Option<Value> {
    super::value::get_at(values, path).cloned()
}

fn has_unknown_ancestor(path: &SettingPath, explicit: &[SettingPath]) -> bool {
    (1..path.0.len()).any(|len| {
        let ancestor = SettingPath(path.0[..len].to_vec());
        explicit.contains(&ancestor)
            && by_key(&ancestor.dotted()).is_none()
            && !is_open(&ancestor.dotted())
    })
}

fn merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Table(mut base), Value::Table(overlay)) => {
            for (key, value) in overlay {
                let old = base.remove(&key).unwrap_or(Value::Null);
                base.insert(key, merge(old, value));
            }
            Value::Table(base)
        }
        (_, overlay) => overlay,
    }
}

fn current_theme_key(key: &str) -> bool {
    key == "split"
        || vtabs_protocol::payload::THEME_COLOR_FIELDS.contains(&key)
        || vtabs_protocol::payload::THEME_FRACTION_FIELDS.contains(&key)
}

fn warn_unknown(warnings: &mut Vec<String>, path: &SettingPath) {
    let warning = format!("unknown option {}", path.dotted());
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

/// Removes values outside the current schema before returning the effective configuration. Open
/// maps remain opaque, except for `theme`, whose accepted keys come from the protocol manifest.
fn prune_unknown(values: &mut Value, path: &SettingPath, warnings: &mut Vec<String>) {
    let Some(table) = values.as_table_mut() else {
        return;
    };
    let keys: Vec<String> = table.keys().cloned().collect();
    for key in keys {
        let child_path = path.child(&key);
        let dotted = child_path.dotted();
        let theme_child = path.0.len() == 1 && path.0[0] == "theme";
        if theme_child && !current_theme_key(&key) {
            table.remove(&key);
            warn_unknown(warnings, &child_path);
            continue;
        }
        match by_key(&dotted) {
            Some(option) if option.container || dotted == "theme" => {
                if let Some(child) = table.get_mut(&key) {
                    prune_unknown(child, &child_path, warnings);
                }
            }
            Some(_) => {}
            None if is_open(&dotted) => {}
            None => {
                table.remove(&key);
                warn_unknown(warnings, &child_path);
            }
        }
    }
}

fn validate(values: &mut Value, warnings: &mut Vec<String>) {
    let schema_defaults = defaults();
    for option in options() {
        let Some(value) = get_path(values, option.key).cloned() else {
            continue;
        };
        if let Some(canonical) = canonical_value(option, &value) {
            if canonical != value {
                set_path(values, option.key, canonical);
            }
            continue;
        }
        warnings.push(format!("invalid {}, using default", option.key));
        let fallback = get_path(&schema_defaults, option.key).cloned();
        set_at(values, &SettingPath::from_dotted(option.key), fallback);
    }
}

/// Applies relationships that span descriptors and returns every path it changed. Both boot
/// normalization and the live settings document use this one policy function.
pub fn normalize_cross_fields(values: &mut Value) -> Vec<SettingPath> {
    let mut changed = Vec::new();
    let popover_width_ok = match get_path(values, "popover.width") {
        Some(Value::String(value)) => value == "auto",
        Some(Value::Number(value)) => value.is_finite() && value.fract() == 0.0,
        _ => false,
    };
    if !popover_width_ok {
        set_path(values, "popover.width", "auto".into());
        changed.push(SettingPath::from_dotted("popover.width"));
    }

    let hover_press = matches!(get_path(values, "hover"), Some(Value::String(v)) if v == "press");
    let no_highlight = get_path(values, "hover_highlight") == Some(&Value::Bool(false));
    let hover_close =
        matches!(get_path(values, "close_button"), Some(Value::String(v)) if v == "hover");
    if (hover_press || no_highlight) && hover_close {
        set_path(values, "close_button", "always".into());
        changed.push(SettingPath::from_dotted("close_button"));
    }
    changed
}

#[cfg(test)]
#[path = "../../tests/unit/settings/normalize.rs"]
mod tests;

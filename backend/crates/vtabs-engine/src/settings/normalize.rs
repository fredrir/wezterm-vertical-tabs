use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::persistence;
use super::schema::{by_key, canonical_value, defaults, is_open, options};
use super::value::{SettingPath, Value, get_path, set_at, set_path};

pub const NORMALIZER_VERSION: u8 = 1;

#[derive(Debug, Deserialize)]
pub struct NormalizeRequest {
    pub normalizer_v: u8,
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
    pub normalizer_v: u8,
    pub plugin_version: &'static str,
    pub schema_id: String,
    pub values: Value,
    pub warnings: Vec<String>,
}

/// Resolves the serializable boot configuration as defaults < persisted < wezterm.lua options.
/// Opaque Lua values are intentionally absent and are restored by the host after this returns.
pub fn normalize(request: NormalizeRequest) -> Result<NormalizeResponse, &'static str> {
    if request.normalizer_v != NORMALIZER_VERSION
        || request.plugin_version != env!("CARGO_PKG_VERSION")
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
            let loaded = persistence::parse_v1(&body);
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
        warnings.push("popover.width must be \"auto\" or a number, using auto".into());
    }
    Ok(NormalizeResponse {
        normalizer_v: NORMALIZER_VERSION,
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
        Some(Value::Number(_)) => true,
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
mod tests {
    use super::*;

    fn request(opts: Value) -> NormalizeRequest {
        NormalizeRequest {
            normalizer_v: NORMALIZER_VERSION,
            plugin_version: env!("CARGO_PKG_VERSION").into(),
            schema_id: super::super::schema::identity(),
            persisted: None,
            opts,
            explicit: vec![],
            invalid: vec![],
        }
    }

    #[test]
    fn resolves_precedence_aliases_bounds_and_cross_rules() {
        let mut request = request(Value::Table(BTreeMap::from([
            ("width".into(), 44.into()),
            ("row_gap".into(), (-1).into()),
            ("hover".into(), "press".into()),
        ])));
        request.persisted =
            Some(r#"{"version":1,"options":{"width":31,"tab_height":2,"row_gap":4}}"#.into());
        request.explicit = vec![SettingPath::from_dotted("width")];
        let response = normalize(request).unwrap();
        assert_eq!(get_path(&response.values, "width"), Some(&44.into()));
        assert_eq!(get_path(&response.values, "row_gap"), Some(&0.into()));
        assert_eq!(
            get_path(&response.values, "tab_height"),
            Some(&"card".into())
        );
        assert_eq!(
            get_path(&response.values, "close_button"),
            Some(&"always".into())
        );
    }

    #[test]
    fn unknown_explicit_segmented_paths_warn_without_splitting_open_keys() {
        let mut request = request(Value::Table(BTreeMap::new()));
        request.explicit = vec![
            SettingPath(vec!["backend".into(), "env".into(), "A.B".into()]),
            SettingPath::from_dotted("padding.typo"),
            SettingPath::from_dotted("typo"),
            SettingPath::from_dotted("typo.child"),
        ];
        let response = normalize(request).unwrap();
        assert_eq!(
            response.warnings,
            vec!["unknown option padding.typo", "unknown option typo"]
        );
    }

    #[test]
    fn lists_replace_instead_of_deep_merging() {
        let mut request = request(Value::Table(BTreeMap::from([(
            "strip_actions".into(),
            Value::List(vec![]),
        )])));
        request.persisted = Some(r#"{"version":1,"options":{"strip_actions":["search"]}}"#.into());
        let response = normalize(request).unwrap();
        assert_eq!(
            get_path(&response.values, "strip_actions"),
            Some(&Value::List(vec![]))
        );
    }

    #[test]
    fn invalid_typed_opaque_paths_override_stored_values_with_defaults() {
        let mut request = request(Value::Table(BTreeMap::new()));
        request.persisted = Some(r#"{"version":1,"options":{"width":45}}"#.into());
        request.explicit = vec![SettingPath::from_dotted("width")];
        request.invalid = vec![SettingPath::from_dotted("width")];
        let response = normalize(request).unwrap();
        assert_eq!(get_path(&response.values, "width"), Some(&28.into()));
        assert!(
            response
                .warnings
                .iter()
                .any(|warning| warning.contains("width"))
        );
    }

    #[test]
    fn mismatched_normalizer_contract_is_rejected() {
        let mut request = request(Value::Table(BTreeMap::new()));
        request.normalizer_v = 99;
        assert_eq!(
            normalize(request).unwrap_err(),
            "normalizer contract mismatch"
        );
    }
}

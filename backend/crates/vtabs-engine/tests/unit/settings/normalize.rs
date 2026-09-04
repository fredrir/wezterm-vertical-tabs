use super::*;
use crate::settings::value::table;

fn request(opts: Value) -> NormalizeRequest {
    NormalizeRequest {
        plugin_version: env!("CARGO_PKG_VERSION").into(),
        schema_id: super::super::schema::identity(),
        persisted: None,
        opts,
        explicit: vec![],
        invalid: vec![],
    }
}

#[test]
fn resolves_precedence_bounds_and_cross_rules() {
    let mut request = request(Value::Table(BTreeMap::from([
        ("width".into(), 44.into()),
        ("row_gap".into(), (-1).into()),
        ("hover".into(), "press".into()),
    ])));
    request.persisted = Some(r#"{"options":{"width":31,"tab_height":2,"row_gap":4}}"#.into());
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
fn unknown_closed_and_theme_keys_are_removed() {
    let removed_theme = ["active", "_title", "_fg"].concat();
    let removed_backend = ["ver", "sion"].concat();
    let response = normalize(request(table([
        (
            "theme",
            Value::Table(BTreeMap::from([
                ("accent".into(), "#ff0000".into()),
                ("split".into(), "auto".into()),
                (removed_theme.clone(), "#00ff00".into()),
            ])),
        ),
        (
            "backend",
            Value::Table(BTreeMap::from([
                ("path".into(), "/bin/wez-vtabs".into()),
                (removed_backend.clone(), "unused".into()),
            ])),
        ),
    ])))
    .unwrap();

    assert_eq!(
        get_path(&response.values, "theme.accent"),
        Some(&"#ff0000".into())
    );
    assert_eq!(
        get_path(&response.values, "theme.split"),
        Some(&"auto".into())
    );
    assert_eq!(
        get_path(&response.values, &format!("theme.{removed_theme}")),
        None
    );
    assert_eq!(
        get_path(&response.values, &format!("backend.{removed_backend}")),
        None
    );
    assert_eq!(response.warnings.len(), 2);
}

#[test]
fn lists_replace_instead_of_deep_merging() {
    let mut request = request(Value::Table(BTreeMap::from([(
        "strip_actions".into(),
        Value::List(vec![]),
    )])));
    request.persisted = Some(r#"{"options":{"strip_actions":["search"]}}"#.into());
    let response = normalize(request).unwrap();
    assert_eq!(
        get_path(&response.values, "strip_actions"),
        Some(&Value::List(vec![]))
    );
}

#[test]
fn invalid_typed_opaque_paths_override_stored_values_with_defaults() {
    let mut request = request(Value::Table(BTreeMap::new()));
    request.persisted = Some(r#"{"options":{"width":45}}"#.into());
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
fn popover_width_is_a_whole_cell_count() {
    let response = normalize(request(table([(
        "popover",
        table([("width", Value::from(12.5))]),
    )])))
    .unwrap();
    assert_eq!(
        get_path(&response.values, "popover.width"),
        Some(&"auto".into())
    );
    assert_eq!(
        response.warnings,
        vec!["popover.width must be \"auto\" or a whole number, using auto"]
    );
}

#[test]
fn mismatched_normalizer_contract_is_rejected() {
    let mut request = request(Value::Table(BTreeMap::new()));
    request.plugin_version = "0.0.0".into();
    assert_eq!(
        normalize(request).unwrap_err(),
        "normalizer contract mismatch"
    );
}

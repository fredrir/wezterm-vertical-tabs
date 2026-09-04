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
fn final_body_is_compact_and_deterministic() {
    let resolved = table([("width", 40.into()), ("row_gap", 1.into())]);
    assert_eq!(
        json_body(&resolved, &BTreeSet::new(), &BTreeSet::new()).unwrap(),
        r#"{"options":{"row_gap":1.0,"width":40.0}}"#
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
fn persistence_parses_filters_closed_keys_and_preserves_open_maps_and_lists() {
    let loaded = parse(
        r#"{"options":{"bogus":1,"padding":{"top":3,"bogus":4},"backend":{"env":{"A.B":"one"}},"strip_actions":[],"spaces":[{"id":"work"}]}}"#,
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
fn persistence_keeps_values_for_canonical_validation() {
    let loaded = parse(r#"{"options":{"tab_height":2,"tooltip":true}}"#);
    assert_eq!(get_path(&loaded.values, "tab_height"), Some(&2.into()));
    assert_eq!(get_path(&loaded.values, "tooltip"), Some(&true.into()));
    assert!(loaded.warnings.is_empty());
}

#[test]
fn persistence_rejects_invalid_document_shapes() {
    for body in [
        "not json",
        r#"{}"#,
        r#"{"options":[]}"#,
        r#"{"options":{},"marker":1}"#,
    ] {
        let loaded = parse(body);
        assert_eq!(loaded.values, Value::Table(BTreeMap::new()));
        assert_eq!(loaded.warnings.len(), 1);
    }
}

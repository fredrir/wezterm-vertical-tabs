use super::*;

#[test]
fn descriptors_are_self_consistent() {
    validate_schema().unwrap();
    assert_eq!(options().len(), 80);
    assert_eq!(identity().len(), 16);
}

#[test]
fn defaults_are_fresh_and_nested() {
    let mut first = defaults();
    set_path(&mut first, "padding.top", 99.into());
    assert_eq!(
        get_path(&defaults(), "padding.top"),
        Some(&Value::Number(1.0))
    );
}

#[test]
fn validation_rejects_retired_values_bounds_and_bad_list_shapes() {
    for (key, value) in [
        ("tab_height", Value::from(3)),
        ("meta", Value::from(true)),
        ("new_tab_button", Value::from(true)),
        ("scroll_indicator", Value::from(false)),
        ("tear_off", Value::from("edge")),
        ("tooltip", Value::from(true)),
        ("animations", Value::from(false)),
    ] {
        assert_eq!(canonical_value(by_key(key).unwrap(), &value), None, "{key}");
    }
    assert_eq!(canonical_value(by_key("width").unwrap(), &4.into()), None);
    let strip = by_key("strip_actions").unwrap();
    assert!(canonical_value(strip, &values!["toggle_sidebar", "search"].into()).is_some());
    assert!(canonical_value(strip, &values!["toggle"].into()).is_none());
    assert!(canonical_value(strip, &values!["not-an-action"].into()).is_none());
    assert!(canonical_value(strip, &Value::List(vec![table([("id", "custom".into())])])).is_some());
}

#[test]
fn value_model_can_represent_explicit_null_and_nested_defaults() {
    let option = Descriptor::new("example", Kind::Any).with_default(Value::Null);
    assert_eq!(option.default, Some(Value::Null));
    assert!(matches!(
        get_path(&defaults(), "private.env"),
        Some(Value::Table(_))
    ));
}

#[test]
fn host_and_apply_policies_are_descriptor_owned_and_exact() {
    let cases = [
        ("frame", Some("window_padding"), ApplyMode::Override),
        ("frame.margin", Some("window_padding"), ApplyMode::Override),
        ("edge_to_edge", Some("window_padding"), ApplyMode::Reload),
        ("theme.split", Some("colors_split"), ApplyMode::Override),
        ("backend.path", None, ApplyMode::Reload),
        ("width", None, ApplyMode::Instant),
    ];
    for (key, host_key, apply_mode) in cases {
        let option = policy_for(key).expect("known policy path");
        assert_eq!(option.host_key, host_key, "{key}");
        assert_eq!(option.apply_mode, apply_mode, "{key}");
    }
    assert!(policy_for("frame.border").is_none());
}

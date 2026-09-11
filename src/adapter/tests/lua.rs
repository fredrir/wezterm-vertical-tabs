use super::*;

#[test]
fn template_configuration_survives_without_declared_spaces() {
    let mut configuration = Configuration::default();
    configuration.templates.push(core::SpaceTemplate {
        id: "host-$host".into(),
        name: "$host".into(),
        icon: "".into(),
        accent: None,
        rules: Vec::new(),
    });
    let value = configuration.value();
    assert_eq!(value["profile"], "default");
    assert!(value.get("spaces").is_none());
    assert_eq!(value["templates"].as_array().unwrap().len(), 1);
}
#[test]
fn routing_type_errors_cannot_become_route_reset() {
    assert!(decode_result("routing", json!(false)).is_err());
    assert!(matches!(
        decode_result("routing", Value::Null).unwrap(),
        Some(HookResult::Route(None))
    ));
}
#[test]
fn footer_rows_are_bounded_and_normalized() {
    assert!(
        matches!(decode_result("footer", json!(["One", "Two"])).unwrap(), Some(HookResult::Footer(text)) if text == "One\nTwo")
    );
    assert!(decode_result("footer", json!(["One", 2])).is_err());
    assert!(decode_result("footer", json!(vec!["row"; 17])).is_err());
}
#[test]
fn batch_capacity_includes_window_hooks() {
    assert!(MAX_HOOK_TABS * TAB_HOOKS.len() + WINDOW_HOOKS.len() <= MAX_HOOK_RESULTS);
    assert!((MAX_HOOK_TABS + 1) * TAB_HOOKS.len() + WINDOW_HOOKS.len() > MAX_HOOK_RESULTS);
}
#[test]
fn each_lua_configuration_keeps_its_own_validated_value() {
    let a = Lua::new();
    let b = Lua::new();
    let configuration = Configuration {
        profile: "kept".into(),
        ..Configuration::default()
    };
    a.set_named_registry_value(CONFIG_REGISTRY, a.to_value(&configuration).unwrap())
        .unwrap();
    assert_eq!(configuration_from_lua(&a).unwrap().profile, "kept");
    assert_eq!(configuration_from_lua(&b).unwrap().profile, "default");
}

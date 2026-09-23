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
    assert_eq!(value["spaces"], json!([]));
    assert!(value["managed"].is_null());
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
#[test]
fn lua_actions_are_typed_before_they_reach_a_window() {
    let lua = Lua::new();
    let action = |source: &str| -> mlua::Result<core::Action> {
        lua.from_value(lua.load(source).eval::<mlua::Value>()?)
    };
    assert_eq!(
        action("return 'navigator'").unwrap(),
        core::Action::Ui(core::UiAction::Navigator)
    );
    assert_eq!(
        action("return { SelectSpace = 'work' }").unwrap(),
        core::Action::Intent(core::Intent::SelectSpace("work".into()))
    );
    assert!(action("return 'navigate'").is_err());
}
#[test]
fn managed_file_tables_round_trip_through_the_registry() {
    let lua = Lua::new();
    let table = lua
        .load(
            "return { managed = { settings = { width = 300 }, spaces = {} }, \
             managed_path = '/tmp/vtabs_settings.lua' }",
        )
        .eval::<mlua::Value>()
        .unwrap();
    let configuration: Configuration = lua.from_value(table).unwrap();
    lua.set_named_registry_value(CONFIG_REGISTRY, lua.to_value(&configuration).unwrap())
        .unwrap();
    let restored = configuration_from_lua(&lua).unwrap();
    assert_eq!(restored.managed.unwrap().settings["width"], json!(300));
    assert_eq!(
        restored.managed_path.unwrap(),
        std::path::Path::new("/tmp/vtabs_settings.lua")
    );
}

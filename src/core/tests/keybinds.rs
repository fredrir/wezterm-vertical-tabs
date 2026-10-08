use serde_json::json;
use vtabs_core::{Settings, keybinds};

#[test]
fn overrides_replace_defaults_and_allow_multiple_or_no_bindings() {
    let mut settings: Settings = serde_json::from_value(json!({"width": 300})).unwrap();
    let original = keybinds::defaults("commands")[0].clone();
    assert_eq!(keybinds::action(&settings, &original), Some("commands"));
    settings
        .set(
            "keybinds",
            json!({"commands": ["Ctrl+Alt+p", "F12"], "refresh": []}),
        )
        .unwrap();
    assert_eq!(keybinds::action(&settings, &original), None);
    for key in ["Ctrl+Alt+p", "F12"] {
        assert_eq!(keybinds::action(&settings, key), Some("commands"));
    }
    assert!(keybinds::bindings(&settings, "refresh").is_empty());
    settings.keyboard_shortcuts = false;
    assert_eq!(keybinds::action(&settings, "F12"), None);
}

#[test]
fn invalid_or_conflicting_bindings_do_not_change_settings() {
    let mut settings = Settings::default();
    let original = settings.clone();
    for bindings in [
        json!({"not_a_plugin_action": ["F12"]}),
        json!({"commands": ["Ctrl+Alt+p", "Ctrl+Alt+p"]}),
        json!({"commands": ["F12"], "settings": ["F12"]}),
        json!({"commands": keybinds::defaults("settings")}),
        json!({"commands": ["p"]}),
        json!({"commands": ["Escape"]}),
        json!({"commands": ["Ctrl+unknown"]}),
        json!({"commands": ["Shift+Ctrl+p"]}),
    ] {
        assert!(
            settings.set("keybinds", bindings.clone()).is_err(),
            "accepted invalid bindings: {bindings}"
        );
        assert_eq!(settings, original);
    }
}

#[test]
fn empty_lua_tables_disable_an_action_without_restoring_its_defaults() {
    let mut settings = Settings::default();
    settings.set("keybinds", json!({"commands": {}})).unwrap();
    assert!(keybinds::bindings(&settings, "commands").is_empty());
    assert_eq!(settings.get("keybinds"), Some(json!({"commands": []})));
}

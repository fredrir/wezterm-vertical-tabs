use super::*;

fn config(json: &str) -> Result<EngineConfig, ConfigError> {
    EngineConfig::try_from(serde_json::from_str::<ConfigMsg>(json).unwrap())
}

#[test]
fn false_disables_the_lua_context_action() {
    let cfg = config(r#"{"context":false}"#).unwrap();
    assert_eq!(cfg.context, ContextMode::Disabled);
}

#[test]
fn wire_modes_and_values_are_validated_once() {
    assert!(config(r#"{"wheel":"teleport"}"#).is_err());
    assert!(config(r#"{"popover":{"width":0}}"#).is_err());
    assert!(config(r#"{"render":{"padding":{"left":-1,"right":0,"top":0,"bottom":0}}}"#).is_err());
}

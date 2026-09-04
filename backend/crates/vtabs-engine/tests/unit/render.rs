use super::*;
use crate::config::EngineConfig;
use vtabs_protocol::payload::ConfigMsg;

fn config(position: &str) -> RenderCfg {
    let message: ConfigMsg = serde_json::from_value(serde_json::json!({
        "position": position,
    }))
    .unwrap();
    EngineConfig::try_from(message).unwrap().render
}

fn action(id: &str) -> layout::Action {
    layout::Action {
        id: id.into(),
        icon: None,
        x: 1,
        x1: 1,
        x2: 1,
    }
}

#[test]
fn canonical_strip_action_ids_select_their_glyphs() {
    let glyphs = Glyphs::from([
        ("toggle_left".into(), "L".into()),
        ("toggle_right".into(), "R".into()),
        ("settings".into(), "S".into()),
        ("new_tab".into(), "+".into()),
    ]);
    assert_eq!(
        action_glyph(&action("toggle_sidebar"), &config("left"), &glyphs),
        "L"
    );
    assert_eq!(
        action_glyph(&action("toggle_sidebar"), &config("right"), &glyphs),
        "R"
    );
    assert_eq!(
        action_glyph(&action("open_settings"), &config("left"), &glyphs),
        "S"
    );
}

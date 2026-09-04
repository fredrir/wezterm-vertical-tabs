//! The Lua encoder and these types must agree; retired shapes fail closed.

use vtabs_protocol::{Command, payload};

fn parse(line: &str) -> Command {
    serde_json::from_str(line).expect(line)
}

#[test]
fn theme_field_manifest_covers_every_wire_override() {
    use std::collections::BTreeSet;

    let serialized = serde_json::to_value(payload::ThemeOverrides::default()).unwrap();
    let actual = serialized
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = payload::THEME_COLOR_FIELDS
        .iter()
        .chain(payload::THEME_FRACTION_FIELDS)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn control_and_maintenance_commands_use_one_exact_shape() {
    assert_eq!(
        parse(r#"{"t":"auth","token":"t","keys":"server"}"#),
        Command::Auth {
            token: "t".into(),
            keys: Some("server".into()),
        }
    );
    assert_eq!(parse(r#"{"t":"kill","pane":7}"#), Command::Kill { pane: 7 });
    assert_eq!(
        parse(r#"{"t":"rescue","band":5,"position":"left"}"#),
        Command::Rescue {
            band: 5,
            position: "left".into(),
        }
    );
    assert_eq!(
        parse(r#"{"t":"adjust","target":28,"min_content":20}"#),
        Command::Adjust {
            target: 28,
            min_content: 20,
        }
    );
    let removed_capabilities = ["ca", "ps"].concat();
    let mut auth_with_removed_field = serde_json::json!({"t":"auth","token":"t"});
    auth_with_removed_field[removed_capabilities] = serde_json::json!([]);
    assert!(serde_json::from_value::<Command>(auth_with_removed_field).is_err());
    for retired in [
        r#"{"t":"kill","title":"pane-title"}"#,
        r#"{"t":"kill"}"#,
        r#"{"t":"rescue","band":5}"#,
        r#"{"t":"adjust","direction":"Left","amount":3,"park":false}"#,
    ] {
        assert!(
            serde_json::from_str::<Command>(retired).is_err(),
            "{retired}"
        );
    }
}

#[test]
fn atomic_sections_parse_without_section_markers() {
    assert_eq!(parse(r#"{"t":"begin"}"#), Command::Begin);
    assert_eq!(parse(r#"{"t":"commit"}"#), Command::Commit);

    let config = r##"{"t":"config","rail_width":5,"position":"left",
        "icons":true,"icon_map":{"nvim":"N"},"meta":"cwd",
        "glyphs":{"custom_block":true,"east_asian_wide":false},
        "double_click_ms":300,"tear_off":true,"wheel":"scroll","context":"popover",
        "hover_timeout_ms":1200,
        "render":{"padding":{"left":1,"right":1,"top":0,"bottom":0},"frame":false,
            "tab_height":"card","row_gap":0,"separator":"gap","pinned_style":"dense",
            "close_button":"hover","show_index":false,"scroll_indicator":"auto",
            "new_tab_button":true,"new_tab_label":"New tab","hover":"follow"}}"##;
    let Command::Config(config) = parse(config) else {
        panic!("not config")
    };
    assert_eq!(config.render.unwrap().padding.left, 1);

    let theme = r##"{"t":"theme","private":true,"scheme":{"background":"#1e1e2e",
        "foreground":"#cdd6f4","cursor_bg":"#f5e0dc","ansi":["#45475a","#f38ba8"]},
        "overrides":{"accent":"#89b4fa","elevation":0.12}}"##;
    let Command::Theme(theme) = parse(theme) else {
        panic!("not theme")
    };
    assert!(theme.private);
    assert_eq!(theme.overrides.accent.as_deref(), Some("#89b4fa"));

    let model = r##"{"t":"model","active":7,
        "focus":{"on":false,"index":1},"scroll":{"top":4,"user":true},
        "drag":{"id":7,"active":true,"slot":3,"outside":false,
            "origin":{"x":5,"y":6,"at":1712345678901}},
        "strip":{"dpi":144,"chrome":{"is_mac":true,"integrated_buttons":true,
            "native_button_style":true,"preview":false,"is_full_screen":false},
            "buttons":[{"id":"toggle_sidebar"},{"id":"open_settings"}]},
        "footer":[{"text":"main"}]}"##;
    let Command::Model(model) = parse(model) else {
        panic!("not model")
    };
    assert_eq!(model.drag.unwrap().origin.x, 5);
    assert_eq!(model.strip.as_ref().unwrap().buttons.len(), 2);

    let spaces = r##"{"t":"spaces","window_id":12,"enabled":true,"hook":true,
        "definitions":[{"id":"home"},{"id":"$host","match":{"remote":true}}],
        "tabs":[{"id":7,"index":1,"title":"shell","proc":"ssh","host":"pi",
            "remote":true,"space":"home","manual":false,"fingerprint":"0123abcd"}],
        "active_tab":7,"active_space":"home","follow":{"tab_id":7,"space":"home"},
        "last_tabs":[{"space_id":"home","tab_id":7}],
        "dynamics":[{"id":"pi","name":"pi","template":"$host","seq":1}]}"##;
    let Command::Spaces(spaces) = parse(spaces) else {
        panic!("not spaces")
    };
    assert_eq!(spaces.window_id, 12);
    assert!(spaces.hook && spaces.tabs[0].remote);

    let settings = r##"{"t":"settings","values":{"width":42,"frame":{},"spaces":[]},
        "explicit":[["width"]],"host_values":["window_padding"],
        "opaque":[["backend","path"]],"key_defaults":{"open.file":{"key":"o"}},
        "is_macos":true,"version":"9.9.9"}"##;
    let Command::Settings(settings) = parse(settings) else {
        panic!("not settings")
    };
    assert_eq!(settings.values["frame"], serde_json::json!({}));
    assert_eq!(settings.explicit[0], ["width"]);
}

#[test]
fn removed_section_fields_and_theme_alias_fail() {
    let old_title_colour = ["active_title_", "fg"].concat();
    let mut theme_colour = serde_json::json!({"t":"theme","private":true,"overrides":{}});
    theme_colour["overrides"][old_title_colour] = "#ffffff".into();
    let retired = [
        theme_colour,
        serde_json::json!({"t":"model","screen":"sidebar"}),
        serde_json::json!({"t":"model","tabs":[]}),
        serde_json::json!({"t":"model","strip":{"metrics":{"cols":28}}}),
        serde_json::json!({"t":"menu","header":{"title":"x"}}),
    ];
    for value in retired {
        assert!(
            serde_json::from_value::<Command>(value.clone()).is_err(),
            "{value}"
        );
    }
}

#[test]
fn typed_nested_payloads_reject_unknown_fields_recursively() {
    let model = [
        serde_json::json!({"t":"model","focus":{"extra":true}}),
        serde_json::json!({"t":"model","scroll":{"extra":true}}),
        serde_json::json!({
            "t":"model",
            "drag":{"id":7,"origin":{"x":1,"y":2,"at":3.0,"extra":true}}
        }),
        serde_json::json!({
            "t":"model",
            "strip":{"chrome":{"extra":true}}
        }),
        serde_json::json!({
            "t":"model",
            "strip":{"buttons":[{"id":"toggle_sidebar","extra":true}]}
        }),
        serde_json::json!({"t":"model","footer":[{"text":"main","extra":true}]}),
    ];

    let spaces = [
        serde_json::json!({
            "t":"spaces",
            "window_id":1,
            "tabs":[{"id":7,"index":0,"extra":true}]
        }),
        serde_json::json!({
            "t":"spaces",
            "window_id":1,
            "follow":{"tab_id":7,"extra":true}
        }),
        serde_json::json!({
            "t":"spaces",
            "window_id":1,
            "last_tabs":[{"space_id":"main","tab_id":7,"extra":true}]
        }),
        serde_json::json!({
            "t":"spaces",
            "window_id":1,
            "dynamics":[{"id":"host","name":"Host","seq":1,"extra":true}]
        }),
    ];

    let menu = [
        serde_json::json!({"t":"menu","anchor":{"row":1,"extra":true}}),
        serde_json::json!({
            "t":"menu",
            "items":[{"id":"close","label":"Close","extra":true}]
        }),
        serde_json::json!({
            "t":"menu",
            "items":[{
                "id":"close",
                "label":"Close",
                "confirm":{"q":"Close?","yes":"Yes","no":"No","extra":true}
            }]
        }),
    ];

    let hook = serde_json::json!({
        "t":"space_route_hook_result",
        "routes":[{"tab_id":7,"extra":true}]
    });

    for value in model.into_iter().chain(spaces).chain(menu).chain([hook]) {
        assert!(
            serde_json::from_value::<Command>(value.clone()).is_err(),
            "{value}"
        );
    }
}

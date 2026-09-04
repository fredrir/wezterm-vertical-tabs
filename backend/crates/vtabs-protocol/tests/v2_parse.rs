//! The wire.lua encoder and these types must agree; shapes here mirror wire.lua's builders.

use vtabs_protocol::Command;

fn parse(line: &str) -> Command {
    serde_json::from_str(line).expect(line)
}

#[test]
fn theme_field_manifest_covers_every_wire_override() {
    use std::collections::BTreeSet;

    let serialized = serde_json::to_value(vtabs_protocol::v2::ThemeOverrides::default()).unwrap();
    let actual = serialized
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = vtabs_protocol::v2::THEME_COLOR_FIELDS
        .iter()
        .chain(vtabs_protocol::v2::THEME_FRACTION_FIELDS)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn auth_keys_kill_and_transport_lines_parse_beside_their_old_shapes() {
    assert_eq!(
        parse(r#"{"t":"auth","token":"t","caps":["typed_intents"],"keys":"server"}"#),
        Command::Auth {
            token: "t".into(),
            caps: vec!["typed_intents".into()],
            keys: Some("server".into()),
        }
    );
    assert_eq!(
        parse(r#"{"t":"kill","title":"wez-vtabs:abcd"}"#),
        Command::Kill {
            title: Some("wez-vtabs:abcd".into()),
            pane: None,
        }
    );
    assert_eq!(
        parse(r#"{"t":"kill","pane":7}"#),
        Command::Kill {
            title: None,
            pane: Some(7),
        }
    );
    assert_eq!(
        parse(r#"{"t":"kill"}"#),
        Command::Kill {
            title: None,
            pane: None,
        },
        "an empty kill parses; the backend reports it as nothing to do"
    );
    for (line, expected) in [
        (
            r#"{"t":"transport_probe","session":"inbox-42-9f3a1b2c"}"#,
            Command::TransportProbe {
                session: "inbox-42-9f3a1b2c".into(),
            },
        ),
        (
            r#"{"t":"transport_barrier","session":"inbox-42-9f3a1b2c"}"#,
            Command::TransportBarrier {
                session: "inbox-42-9f3a1b2c".into(),
            },
        ),
        (
            r#"{"t":"transport_stop","session":"inbox-42-9f3a1b2c"}"#,
            Command::TransportStop {
                session: "inbox-42-9f3a1b2c".into(),
            },
        ),
    ] {
        assert_eq!(parse(line), expected);
    }
    assert!(
        serde_json::from_str::<Command>(r#"{"t":"transport_barrier"}"#).is_err(),
        "a barrier always names its session"
    );
}

#[test]
fn v2_lines_parse() {
    assert_eq!(
        parse(r#"{"t":"auth","token":"legacy"}"#),
        Command::Auth {
            token: "legacy".into(),
            caps: Vec::new(),
            keys: None,
        }
    );
    assert_eq!(
        parse(r#"{"t":"auth","token":"new","caps":["typed_intents"]}"#),
        Command::Auth {
            token: "new".into(),
            caps: vec!["typed_intents".into()],
            keys: None,
        }
    );
    assert_eq!(
        parse(r#"{"t":"begin","generation":42}"#),
        Command::Begin { generation: 42 }
    );
    assert_eq!(
        parse(r#"{"t":"commit","generation":42}"#),
        Command::Commit { generation: 42 }
    );

    let config = r##"{"t":"config","rev":3,"rail_width":5,"position":"left",
        "icons":true,"icon_map":{"nvim":"N"},"meta":"cwd",
        "glyphs":{"custom_block":true,"east_asian_wide":false},
        "double_click_ms":300,"tear_off":true,"wheel":"scroll","context":"popover",
        "hover_timeout_ms":1200,
        "render":{"padding":{"left":1,"right":1,"top":0,"bottom":0},"frame":false,
            "tab_height":"card","row_gap":0,"separator":"gap","pinned_style":"dense",
            "close_button":"hover","show_index":false,"scroll_indicator":"auto",
            "new_tab_button":true,"new_tab_label":"New tab","hover":"follow"}}"##;
    let Command::Config(c) = parse(config) else {
        panic!("not config")
    };
    assert_eq!(c.rev, 3);
    assert_eq!(c.render.unwrap().padding.left, 1);

    let theme = r##"{"t":"theme","rev":7,"private":true,"scheme":{"background":"#1e1e2e","foreground":"#cdd6f4",
        "cursor_bg":"#f5e0dc","ansi":["#45475a","#f38ba8"]},
        "overrides":{"accent":"#89b4fa","elevation":0.12}}"##;
    let Command::Theme(t) = parse(theme) else {
        panic!("not theme")
    };
    assert_eq!(t.scheme.ansi.len(), 2);
    assert_eq!(t.overrides.accent.as_deref(), Some("#89b4fa"));
    assert_eq!(t.overrides.elevation, Some(0.12));
    assert_eq!(t.private, Some(true));
    assert!(!t.hook, "the new hook handshake is opt-in");

    let Command::Theme(t) = parse(r#"{"t":"theme","rev":8,"hook":true}"#) else {
        panic!("not theme")
    };
    assert!(t.hook);
    assert_eq!(
        parse(
            r##"{"t":"theme_hook_result","generation":8,"overrides":{"accent":"#123456","scrim":0.4}}"##,
        ),
        Command::ThemeHookResult {
            generation: 8,
            overrides: Box::new(vtabs_protocol::v2::ThemeOverrides {
                accent: Some("#123456".into()),
                scrim: Some(0.4),
                ..Default::default()
            }),
        }
    );
    assert!(
        serde_json::from_str::<Command>(
            r#"{"t":"theme_hook_result","generation":8,"overrides":{"accent":false}}"#
        )
        .is_err()
    );

    let model = r##"{"t":"model","rev":142,"screen":"sidebar","active":7,
        "focus":{"on":false,"index":1},"scroll":{"top":4,"user":true},
        "drag":{"id":7,"active":true,"slot":3,"outside":false,
            "origin":{"x":5,"y":6,"at":1712345678901}},
        "strip":{"dpi":144,"metrics":{"cols":28,"viewport_rows":30,"pixel_width":235,"pixel_height":570,"dpi":144},
            "chrome":{"is_mac":true,"integrated_buttons":true,"native_button_style":true,
                "preview":false,"is_full_screen":false},
            "buttons":[{"id":"toggle"},{"id":"open_settings"}]},
        "footer":[{"text":"main"}],
        "space":"home",
        "spaces":[{"id":"home","name":"Home","icon":"H"},{"id":"work","unseen":true}],
        "tabs":[{"id":7,"index":1,"title":"nvim","pane_title":"nvim - x","override":null,
            "proc":"nvim","cwd":"~/p/x","host":null,"user":null,"domain":"local",
            "pinned":false,"unseen":false}],"private":false}"##;
    let Command::Model(m) = parse(model) else {
        panic!("not model")
    };
    let m = m.sidebar().expect("sidebar model");
    assert_eq!(m.tabs[0].proc.as_deref(), Some("nvim"));
    assert_eq!(m.drag.unwrap().origin.x, 5);
    assert_eq!(m.strip.as_ref().unwrap().buttons.len(), 2);
    assert_eq!(
        m.strip.as_ref().unwrap().metrics.unwrap().pixel_width,
        235.0
    );
    assert!(m.strip.as_ref().unwrap().chrome.unwrap().is_mac);
    assert_eq!(m.strip.as_ref().unwrap().dpi, Some(144.0));
    assert_eq!(m.space.as_deref(), Some("home"));
    assert_eq!(m.spaces.len(), 2);
    assert_eq!(m.spaces[0].icon.as_deref(), Some("H"));
    assert_eq!((m.spaces[1].name.as_str(), m.spaces[1].unseen), ("", true));

    // a model from a plugin without spaces parses with none, so the switcher never appears
    let bare = r##"{"t":"model","rev":1,"screen":"sidebar","tabs":[]}"##;
    let Command::Model(m) = parse(bare) else {
        panic!("not model")
    };
    let m = m.sidebar().expect("sidebar model");
    assert!(m.space.is_none() && m.spaces.is_empty());

    // Sidebar was the only model before screens were introduced, so an absent tag remains valid.
    let legacy = r##"{"t":"model","rev":1,"tabs":[]}"##;
    let Command::Model(m) = parse(legacy) else {
        panic!("not model")
    };
    assert!(m.sidebar().is_some());

    let spaces = r##"{"t":"spaces","rev":4,"window_id":12,"enabled":true,"hook":true,
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
    assert_eq!(spaces.tabs[0].space.as_deref(), Some("home"));
    assert_eq!(spaces.dynamics[0].template.as_deref(), Some("$host"));
    assert_eq!(
        parse(
            r#"{"t":"space_route_hook_result","generation":5,"routes":[{"tab_id":7,"space":"work"},{"tab_id":8}]}"#,
        ),
        Command::SpaceRouteHookResult {
            generation: 5,
            routes: vec![
                vtabs_protocol::v2::SpaceRouteHookAnswer {
                    tab_id: 7,
                    space: Some("work".into()),
                },
                vtabs_protocol::v2::SpaceRouteHookAnswer {
                    tab_id: 8,
                    space: None,
                },
            ],
        }
    );

    let settings = r##"{"t":"settings","rev":9,"values":{"width":42,"frame":{},"spaces":[]},
        "explicit":[["width"]],"host_values":["window_padding"],
        "opaque":[["backend","path"]],"key_defaults":{"open.file":{"key":"o"}},
        "is_macos":true,"version":"9.9.9"}"##;
    let Command::Settings(settings) = parse(settings) else {
        panic!("not settings")
    };
    assert_eq!(settings.rev, 9);
    assert_eq!(settings.values["frame"], serde_json::json!({}));
    assert_eq!(settings.values["spaces"], serde_json::json!([]));
    assert_eq!(settings.explicit[0], ["width"]);
    assert_eq!(settings.opaque[0], ["backend", "path"]);
    assert_eq!(settings.key_defaults["open.file"]["key"], "o");
    assert!(settings.is_macos);

    let Command::Fx(fx) = parse(r##"{"t":"fx","phase":"expand"}"##) else {
        panic!()
    };
    assert_eq!(fx.phase, "expand");
    let Command::Notice(n) = parse(r##"{"t":"notice","level":"warn","text":"hi"}"##) else {
        panic!()
    };
    assert_eq!(n.text, "hi");
}

#[test]
fn an_adjust_names_a_target_beside_the_delta_or_keeps_the_old_shape() {
    assert_eq!(
        parse(r#"{"t":"adjust","direction":"Left","amount":3,"park":false}"#),
        Command::Adjust {
            direction: "Left".into(),
            amount: 3,
            park: false,
            target: None,
            min_content: None,
        }
    );
    assert_eq!(
        parse(
            r#"{"t":"adjust","direction":"Left","amount":3,"park":false,"target":28,"min_content":20}"#
        ),
        Command::Adjust {
            direction: "Left".into(),
            amount: 3,
            park: false,
            target: Some(28),
            min_content: Some(20),
        }
    );
}

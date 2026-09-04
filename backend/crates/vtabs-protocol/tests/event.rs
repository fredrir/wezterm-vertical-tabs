use vtabs_protocol::{
    CardPart, Event, Intent, Modifier, Mods, SettingsApplyMode, SettingsChange, SettingsPatch,
    payload,
};

fn resolved_theme() -> payload::ResolvedTheme {
    let c = [1, 2, 3];
    payload::ResolvedTheme {
        bg: c,
        fg: c,
        dim: c,
        accent: c,
        title_idle: c,
        meta_fg: c,
        active_bg: c,
        active_fg: c,
        hover_bg: c,
        hover_fg: c,
        focus_bg: c,
        pinned_fg: c,
        separator: c,
        border: c,
        border_idle: c,
        ghost_border_hover: c,
        new_tab_fg: c,
        close_fg: c,
        close_hover_fg: c,
        unseen_fg: c,
        private_accent: c,
        drag_bg: c,
        drag_fg: c,
        scroll_fg: c,
        scroll_idle_fg: c,
        title_active: c,
        title_active_contrast: 4.5,
        content_bg: c,
        surface_raised: c,
        scrim: 0.4,
        disabled_fg: c,
        popover_sel_bg: c,
        popover_sel_fg: c,
        popover_sel_hint: c,
    }
}

#[test]
fn ready_and_resize() {
    assert_eq!(
        Event::ready_at(30, 40, 42, None).to_json(),
        r#"{"t":"ready","cols":30,"rows":40,"pane":42}"#
    );
    assert_eq!(
        Event::Resize { cols: 31, rows: 40 }.to_json(),
        r#"{"t":"resize","cols":31,"rows":40}"#
    );
}

#[test]
fn ready_names_the_server_pane_and_the_inbox_it_offers() {
    assert_eq!(
        Event::ready_at(30, 40, 42, Some("inbox-42-9f3a1b2c".into())).to_json(),
        r#"{"t":"ready","cols":30,"rows":40,"pane":42,"transport":{"inbox":"inbox-42-9f3a1b2c"}}"#
    );
    assert_eq!(
        Event::ready_at(30, 40, 42, None).to_json(),
        r#"{"t":"ready","cols":30,"rows":40,"pane":42}"#
    );
}

#[test]
fn transport_events_name_their_session_and_a_lost_message_its_seq() {
    assert_eq!(
        Event::TransportReady {
            session: "inbox-42-9f3a1b2c".into()
        }
        .to_json(),
        r#"{"t":"transport_ready","session":"inbox-42-9f3a1b2c"}"#
    );
    assert_eq!(
        Event::TransportRefused {
            session: "inbox-42-9f3a1b2c".into(),
            why: "probe",
        }
        .to_json(),
        r#"{"t":"transport_refused","session":"inbox-42-9f3a1b2c","why":"probe"}"#
    );
    assert_eq!(
        Event::dropped_message(7).to_json(),
        r#"{"t":"dropped","what":"message","reason":"gap","seq":7}"#
    );
    assert_eq!(
        Event::dropped("model", "bounds").to_json(),
        r#"{"t":"dropped","what":"model","reason":"bounds"}"#
    );
}

#[test]
fn a_delivered_key_says_so_and_an_ordinary_one_omits_the_flag() {
    let key = Event::key("x".into(), Mods::default(), b"x");
    assert_eq!(key.to_json(), r#"{"t":"key","key":"x","raw":"eA=="}"#);
    assert_eq!(
        key.delivered().to_json(),
        r#"{"t":"key","key":"x","raw":"eA==","delivered":true}"#
    );
    assert_eq!(
        Event::Pong { echo: None }.delivered().to_json(),
        r#"{"t":"pong"}"#,
        "only a key can be delivered"
    );
}

#[test]
fn settings_effects_distinguish_set_remove_and_copy() {
    assert_eq!(
        Event::SettingsCommit {
            path: vec!["backend".into(), "env".into(), "A.B".into()],
            change: SettingsChange::Set {
                value: serde_json::json!(["one", "two"]),
            },
            derived: vec![],
            mode: SettingsApplyMode::Instant,
            persistence_json: r#"{"options":{}}"#.into(),
        }
        .to_json(),
        r#"{"t":"settings_commit","path":["backend","env","A.B"],"change":{"op":"set","value":["one","two"]},"mode":"instant","persistence_json":"{\"options\":{}}"}"#
    );
    assert_eq!(
        Event::SettingsCommit {
            path: vec!["keys".into(), "open.file".into()],
            change: SettingsChange::Remove,
            derived: vec![],
            mode: SettingsApplyMode::Reload,
            persistence_json: "{}".into(),
        }
        .to_json(),
        r#"{"t":"settings_commit","path":["keys","open.file"],"change":{"op":"remove"},"mode":"reload","persistence_json":"{}"}"#
    );
    assert_eq!(
        Event::SettingsCopy { lua: "x".into() }.to_json(),
        r#"{"t":"settings_copy","lua":"x"}"#
    );
    assert_eq!(
        Event::SettingsCommit {
            path: vec!["hover".into()],
            change: SettingsChange::Set {
                value: serde_json::json!("press"),
            },
            derived: vec![SettingsPatch {
                path: vec!["close_button".into()],
                change: SettingsChange::Set {
                    value: serde_json::json!("always"),
                },
            }],
            mode: SettingsApplyMode::Instant,
            persistence_json: "{}".into(),
        }
        .to_json(),
        r#"{"t":"settings_commit","path":["hover"],"change":{"op":"set","value":"press"},"derived":[{"path":["close_button"],"op":"set","value":"always"}],"mode":"instant","persistence_json":"{}"}"#
    );
}

#[test]
fn theme_hook_events_keep_the_resolved_theme_together() {
    let theme = resolved_theme();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &Event::ThemeHookRequest {
                theme: theme.clone(),
            }
            .to_json(),
        )
        .unwrap(),
        serde_json::json!({"t":"theme_hook_request","theme":theme})
    );

    let json = Event::ThemeResolved {
        theme: resolved_theme(),
    }
    .to_json();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["t"], "theme_resolved");
    assert_eq!(value["theme"]["bg"], serde_json::json!([1, 2, 3]));
    assert_eq!(value["theme"]["title_active_contrast"], 4.5);
}

#[test]
fn spaces_events_are_flat_and_name_the_window() {
    let request = Event::SpaceRouteHookRequest {
        window_id: 4,
        tabs: vec![payload::SpaceRouteHookFact {
            tab_id: 7,
            window_id: 4,
            title: "shell".into(),
            proc: Some("ssh".into()),
            cwd: None,
            host: Some("pi".into()),
            user: None,
            domain: Some("ssh:pi".into()),
            remote: true,
            space: Some("home".into()),
            manual: false,
        }],
    };
    let value: serde_json::Value = serde_json::from_str(&request.to_json()).unwrap();
    assert_eq!(value["t"], "space_route_hook_request");
    assert_eq!(value["window_id"], 4);
    assert_eq!(value["tabs"][0]["host"], "pi");

    let resolved = Event::SpacesResolved {
        window_id: 4,
        resolution: Box::new(payload::SpaceResolution {
            active: Some("home".into()),
            assignments: vec![payload::SpaceAssignment {
                tab_id: 7,
                space: Some("home".into()),
                manual: false,
                fingerprint: Some("abcd".into()),
            }],
            dynamics: Vec::new(),
            follow: Some(payload::SpaceFollow {
                tab_id: 7,
                space: Some("home".into()),
            }),
            last_tabs: vec![payload::SpaceLastTab {
                space_id: "home".into(),
                tab_id: 7,
            }],
            summary: vec![payload::SpaceSummary {
                id: "home".into(),
                name: "Home".into(),
                icon: None,
                count: 1,
                unseen: false,
            }],
            visible_tab_ids: vec![7],
            theme_overrides: payload::ThemeOverrides {
                accent: Some("#123456".into()),
                ..Default::default()
            },
            warnings: Vec::new(),
        }),
    };
    let value: serde_json::Value = serde_json::from_str(&resolved.to_json()).unwrap();
    assert_eq!(value["t"], "spaces_resolved");
    assert_eq!(value["window_id"], 4);
    assert_eq!(value["active"], "home", "the resolution is flattened");
    assert!(value.get("resolution").is_none());
    assert_eq!(value["theme_overrides"]["accent"], "#123456");
}

#[test]
fn intent_events_flatten_their_variant_fields() {
    assert_eq!(
        Event::intent(Intent::PressCard {
            tab_id: 7,
            x: 5,
            y: 6,
            part: Some(CardPart::Title),
        })
        .to_json(),
        r#"{"t":"intent","a":"press_card","tab_id":7,"x":5,"y":6,"part":"title"}"#
    );
    assert_eq!(
        Event::intent(Intent::MoveTab {
            tab_id: 7,
            slot: 3,
            focus_index: 3,
        })
        .to_json(),
        r#"{"t":"intent","a":"move_tab","tab_id":7,"slot":3,"focus_index":3}"#
    );
    assert_eq!(
        Event::intent(Intent::RecordChord {
            key: "p".into(),
            mods: vec![Modifier::Shift, Modifier::Ctrl],
        })
        .to_json(),
        r#"{"t":"intent","a":"record_chord","key":"p","mods":["shift","ctrl"]}"#
    );
    assert_eq!(
        Event::intent(Intent::SetRailReserve { cols: 9 }).to_json(),
        r#"{"t":"intent","a":"set_rail_reserve","cols":9}"#
    );
}

#[test]
fn generated_intent_inventory_matches_every_serde_tag() {
    let intents = vec![
        Intent::PressCard {
            tab_id: 1,
            x: 2,
            y: 3,
            part: None,
        },
        Intent::DragTo {
            x: 1,
            y: 2,
            slot: 3,
            outside: false,
        },
        Intent::DragEnd {
            outside: false,
            slot: None,
        },
        Intent::RequestClose {
            tab_id: 1,
            row: 2,
            col: None,
            from_key: false,
        },
        Intent::TogglePin { tab_id: 1 },
        Intent::OpenMenu {
            tab_id: 1,
            row: 2,
            col: None,
        },
        Intent::NewTab,
        Intent::Strip {
            button_id: "toggle_sidebar".into(),
        },
        Intent::Footer { index: 1 },
        Intent::SwitchSpace {
            space_id: "work".into(),
        },
        Intent::WheelTab { dy: 1 },
        Intent::SetScroll {
            top: 1,
            user: false,
        },
        Intent::SetFocusIndex { index: 1 },
        Intent::ActivateTab { tab_id: 1 },
        Intent::BlurSidebar,
        Intent::MenuPick {
            item_id: "close".into(),
        },
        Intent::MenuBack,
        Intent::MenuClosed,
        Intent::RenameCommit { text: "tab".into() },
        Intent::RenameTab { tab_id: 1 },
        Intent::MoveTab {
            tab_id: 1,
            slot: 2,
            focus_index: 2,
        },
        Intent::SetRailReserve { cols: 4 },
        Intent::NudgeOption {
            key: "width".into(),
            delta: 1,
        },
        Intent::ActivateOption {
            key: "debug".into(),
        },
        Intent::ResetOption {
            key: "width".into(),
        },
        Intent::SettingsCopy,
        Intent::EditKey {
            key: "escape".into(),
        },
        Intent::RecordChord {
            key: "x".into(),
            mods: vec![Modifier::Ctrl],
        },
        Intent::CloseSettings,
    ];
    let tags = intents
        .iter()
        .map(|intent| {
            serde_json::to_value(intent).unwrap()["a"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tags,
        Intent::NAMES,
        "serde tags and the generated Lua inventory cannot drift"
    );
}

#[test]
fn key_events() {
    assert_eq!(
        Event::key("enter".into(), Mods::default(), b"\r").to_json(),
        r#"{"t":"key","key":"enter","raw":"DQ=="}"#
    );
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert_eq!(
        Event::key("c".into(), ctrl, b"\x03").to_json(),
        r#"{"t":"key","key":"c","mods":["ctrl"],"raw":"Aw=="}"#
    );
    assert_eq!(
        Event::key("escape".into(), Mods::default(), b"").to_json(),
        r#"{"t":"key","key":"escape"}"#
    );
}

#[test]
fn focus_and_pong() {
    assert_eq!(
        Event::Focus { focused: true }.to_json(),
        r#"{"t":"focus","in":true}"#
    );
    assert_eq!(
        Event::Focus { focused: false }.to_json(),
        r#"{"t":"focus","in":false}"#
    );
    assert_eq!(
        Event::paste(Some(b"hi".to_vec())).to_json(),
        r#"{"t":"paste","data":"aGk="}"#
    );
    assert_eq!(
        Event::paste(None).to_json(),
        r#"{"t":"paste","dropped":"size"}"#
    );
    assert_eq!(
        Event::Dropped {
            what: "model",
            reason: "bounds",
            seq: None,
        }
        .to_json(),
        r#"{"t":"dropped","what":"model","reason":"bounds"}"#
    );
    assert_eq!(Event::Pong { echo: None }.to_json(), r#"{"t":"pong"}"#);
    assert_eq!(
        Event::MenuRefused {
            why: Some("bounds"),
            id: Some(7),
            level: Some("confirm"),
        }
        .to_json(),
        r#"{"t":"menu_refused","why":"bounds","id":7,"level":"confirm"}"#
    );
    // `echo` is the ping's own number; App::emit appends the monotonic `n` on top of it
    assert_eq!(
        Event::Pong { echo: Some(5) }.to_json(),
        r#"{"t":"pong","echo":5}"#
    );
}

#[test]
fn a_cli_answer_carries_the_width_only_once_an_adjust_has_one() {
    assert_eq!(
        Event::Cli {
            op: "kill",
            ok: true,
            detail: String::new(),
            cols: None,
        }
        .to_json(),
        r#"{"t":"cli","op":"kill","ok":true}"#
    );
    assert_eq!(
        Event::Cli {
            op: "adjust",
            ok: true,
            detail: "3".into(),
            cols: Some(28),
        }
        .to_json(),
        r#"{"t":"cli","op":"adjust","ok":true,"detail":"3","cols":28}"#
    );
}

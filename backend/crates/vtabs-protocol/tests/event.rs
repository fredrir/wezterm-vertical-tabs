use vtabs_protocol::{
    CardPart, Event, Intent, Modifier, Mods, SettingsApplyMode, SettingsChange, SettingsPatch, v2,
};

fn resolved_theme() -> v2::ResolvedTheme {
    let c = [1, 2, 3];
    v2::ResolvedTheme {
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
        active_title_fg: c,
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
        Event::ready(30, 40).to_json(),
        r#"{"t":"ready","v":3,"cols":30,"rows":40,"paints":true,"caps":["atomic_sync","typed_intents","theme_hooks","settings_document","spaces_policy"]}"#
    );
    assert_eq!(
        Event::Resize { cols: 31, rows: 40 }.to_json(),
        r#"{"t":"resize","cols":31,"rows":40}"#
    );
}

#[test]
fn settings_effects_distinguish_set_remove_and_copy() {
    assert_eq!(
        Event::SettingsCommit {
            settings_rev: 7,
            path: vec!["backend".into(), "env".into(), "A.B".into()],
            change: SettingsChange::Set {
                value: serde_json::json!(["one", "two"]),
            },
            derived: vec![],
            mode: SettingsApplyMode::Instant,
            persistence_json: r#"{"version":1,"options":{}}"#.into(),
        }
        .to_json(),
        r#"{"t":"settings_commit","settings_rev":7,"path":["backend","env","A.B"],"change":{"op":"set","value":["one","two"]},"mode":"instant","persistence_json":"{\"version\":1,\"options\":{}}"}"#
    );
    assert_eq!(
        Event::SettingsCommit {
            settings_rev: 8,
            path: vec!["keys".into(), "open.file".into()],
            change: SettingsChange::Remove,
            derived: vec![],
            mode: SettingsApplyMode::Reload,
            persistence_json: "{}".into(),
        }
        .to_json(),
        r#"{"t":"settings_commit","settings_rev":8,"path":["keys","open.file"],"change":{"op":"remove"},"mode":"reload","persistence_json":"{}"}"#
    );
    assert_eq!(
        Event::SettingsCopy { lua: "x".into() }.to_json(),
        r#"{"t":"settings_copy","lua":"x"}"#
    );
    assert_eq!(
        Event::SettingsCommit {
            settings_rev: 9,
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
        r#"{"t":"settings_commit","settings_rev":9,"path":["hover"],"change":{"op":"set","value":"press"},"derived":[{"path":["close_button"],"op":"set","value":"always"}],"mode":"instant","persistence_json":"{}"}"#
    );
}

#[test]
fn theme_hook_events_keep_generation_and_resolved_theme_together() {
    let theme = resolved_theme();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &Event::ThemeHookRequest {
                generation: 12,
                theme: theme.clone(),
            }
            .to_json(),
        )
        .unwrap(),
        serde_json::json!({"t":"theme_hook_request","generation":12,"theme":theme})
    );

    let json = Event::ThemeResolved {
        generation: None,
        theme: resolved_theme(),
    }
    .to_json();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["t"], "theme_resolved");
    assert!(value.get("generation").is_none(), "legacy reports omit it");
    assert_eq!(value["theme"]["bg"], serde_json::json!([1, 2, 3]));
    assert_eq!(value["theme"]["title_active_contrast"], 4.5);
}

#[test]
fn spaces_events_are_flat_and_guarded_by_generation_and_window() {
    let request = Event::SpaceRouteHookRequest {
        generation: 21,
        window_id: 4,
        tabs: vec![v2::SpaceRouteHookFact {
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
    assert_eq!(value["generation"], 21);
    assert_eq!(value["window_id"], 4);
    assert_eq!(value["tabs"][0]["host"], "pi");

    let resolved = Event::SpacesResolved {
        generation: 21,
        window_id: 4,
        resolution: Box::new(v2::SpaceResolution {
            active: Some("home".into()),
            assignments: vec![v2::SpaceAssignment {
                tab_id: 7,
                space: Some("home".into()),
                manual: false,
                fingerprint: Some("abcd".into()),
            }],
            dynamics: Vec::new(),
            follow: Some(v2::SpaceFollow {
                tab_id: 7,
                space: Some("home".into()),
            }),
            last_tabs: vec![v2::SpaceLastTab {
                space_id: "home".into(),
                tab_id: 7,
            }],
            summary: vec![v2::SpaceSummary {
                id: "home".into(),
                name: "Home".into(),
                icon: None,
                count: 1,
                unseen: false,
            }],
            visible_tab_ids: vec![7],
            theme_overrides: v2::ThemeOverrides {
                accent: Some("#123456".into()),
                ..Default::default()
            },
            warnings: Vec::new(),
        }),
    };
    let value: serde_json::Value = serde_json::from_str(&resolved.to_json()).unwrap();
    assert_eq!(value["t"], "spaces_resolved");
    assert_eq!(value["generation"], 21);
    assert_eq!(value["window_id"], 4);
    assert_eq!(value["active"], "home", "the resolution is flattened");
    assert!(value.get("resolution").is_none());
    assert_eq!(value["theme_overrides"]["accent"], "#123456");
}

#[test]
fn do_events_match_the_lua_reader() {
    assert_eq!(
        Intent::PressCard {
            tab_id: 7,
            x: 5,
            y: 6,
            part: Some(CardPart::Title),
        }
        .downgrade()
        .to_json(),
        r#"{"t":"do","a":"press_card","id":7,"args":{"x":5,"y":6,"part":"title"}}"#
    );
    assert_eq!(
        Intent::NewTab.downgrade().to_json(),
        r#"{"t":"do","a":"new_tab"}"#
    );
    assert_eq!(
        Intent::Strip {
            button_id: "settings".into(),
        }
        .downgrade()
        .to_json(),
        r#"{"t":"do","a":"strip","id":"settings"}"#
    );
    assert_eq!(
        Intent::SwitchSpace {
            space_id: "work".into(),
        }
        .downgrade()
        .to_json(),
        r#"{"t":"do","a":"switch_space","id":"work"}"#
    );
    assert_eq!(
        Intent::SetScroll { top: 3, user: true }
            .downgrade()
            .to_json(),
        r#"{"t":"do","a":"set_scroll","args":{"top":3,"user":true}}"#
    );
    assert_eq!(
        Intent::SetRailReserve { cols: 9 }.downgrade().to_json(),
        r#"{"t":"do","a":"set_rail_reserve","args":{"cols":9}}"#
    );
}

#[test]
fn typed_intents_flatten_their_variant_fields() {
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
fn generated_intent_inventory_matches_every_serde_and_legacy_tag() {
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
            button_id: "toggle".into(),
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
    for intent in intents {
        let typed = intent.name();
        let legacy: serde_json::Value =
            serde_json::from_str(&intent.downgrade().to_json()).unwrap();
        let expected = if typed == "activate_tab" {
            "activate_tab_by_id"
        } else {
            typed
        };
        assert_eq!(legacy["a"], expected);
    }
}

#[test]
fn the_legacy_do_shape_is_produced_only_by_the_intent_downgrade() {
    assert_eq!(
        Intent::MoveTab {
            tab_id: 7,
            slot: 3,
            focus_index: 3,
        }
        .downgrade()
        .to_json(),
        r#"{"t":"do","a":"move_tab","id":7,"args":{"slot":3,"focus_index":3}}"#
    );
    assert_eq!(
        Intent::SwitchSpace {
            space_id: "work".into(),
        }
        .downgrade()
        .to_json(),
        r#"{"t":"do","a":"switch_space","id":"work"}"#
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
            reason: "bounds"
        }
        .to_json(),
        r#"{"t":"dropped","what":"model","reason":"bounds"}"#
    );
    assert_eq!(Event::Pong { echo: None }.to_json(), r#"{"t":"pong"}"#);
    assert_eq!(
        Event::Note {
            k: "menu_refused",
            why: Some("bounds"),
            id: Some(7),
            a: Some("confirm"),
        }
        .to_json(),
        r#"{"t":"note","k":"menu_refused","why":"bounds","id":7,"a":"confirm"}"#
    );
    // `echo` is the ping's own number; App::emit appends the monotonic `n` on top of it
    assert_eq!(
        Event::Pong { echo: Some(5) }.to_json(),
        r#"{"t":"pong","echo":5}"#
    );
}

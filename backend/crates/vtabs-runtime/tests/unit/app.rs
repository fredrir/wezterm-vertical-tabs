use super::*;
use std::cell::Cell;
use vtabs_protocol::Mods;

thread_local! {
    static PROBE: Cell<Option<(u16, u16)>> = const { Cell::new(None) };
}

fn test_probe() -> Option<(u16, u16)> {
    PROBE.with(Cell::get)
}

fn probe_returns(size: (u16, u16)) {
    PROBE.with(|p| p.set(Some(size)));
}

fn app() -> App<Vec<u8>> {
    App {
        out: Vec::new(),
        log: Logger::from_env(),
        var: "vtabs".into(),
        size: (28, 24),
        probe: test_probe,
        pixel_probe: || None,
        parked_focus: None,
        needs_clear: false,
        fx: None,
        last_rows: None,
        shown_is_final: false,
        seq: 0,
        v2: V2State::default(),
        ui: Default::default(),
        started: Instant::now(),
        popover: None,
        settings_ui: Default::default(),
        menu_ui: Default::default(),
        noted_menu: None,
        hover_deadline: None,
        token: None,
        token_announced: None,
        resize: None,
        client_typed_intents: false,
        client_theme_hooks: false,
        client_spaces_policy: false,
        last_reported_theme: None,
        last_rail_reserve: None,
        cli: None,
        inbox: None,
        transport: Default::default(),
        server_keys: false,
        metrics: Default::default(),
    }
}

#[test]
fn a_kill_or_rescue_with_no_cli_to_run_reports_the_failure() {
    let mut a = app();
    a.handle(Input::Command(Command::Kill {
        title: Some("wez-vtabs:abcd".into()),
        pane: None,
    }))
    .unwrap();
    a.handle(Input::Command(Command::Rescue {
        band: 28,
        position: None,
    }))
    .unwrap();
    let sent = payloads(&a);
    assert!(
        sent[0].starts_with(r#"{"t":"cli","op":"kill","ok":false,"detail":"no cli here""#),
        "{sent:?}"
    );
    assert!(sent[1].starts_with(r#"{"t":"cli","op":"rescue","ok":false"#));
    a.handle(Input::Command(Command::Adjust {
        direction: "Left".into(),
        amount: 5,
        park: false,
        target: None,
        min_content: None,
    }))
    .unwrap();
    let sent = payloads(&a);
    assert!(
        sent[2].starts_with(r#"{"t":"cli","op":"adjust","ok":false,"detail":"no cli here""#),
        "an adjust with no cli reports the failure the same way: {}",
        sent[2]
    );
}

#[test]
fn quit_stops_the_loop() {
    let mut a = app();
    assert!(!a.handle(Input::Command(Command::Quit)).unwrap());
    assert!(a.out.is_empty());
}

#[test]
fn ping_answers_with_pong_user_var() {
    let mut a = app();
    a.handle(Input::Command(Command::Ping { n: Some(7) }))
        .unwrap();
    assert_eq!(
        a.out,
        set_user_var("vtabs", r#"{"t":"pong","echo":7,"n":1}"#).as_bytes()
    );
}

#[test]
fn auth_echoes_token_as_user_var() {
    let mut a = app();
    a.handle(Input::Command(Command::Auth {
        token: "abc".into(),
        caps: Vec::new(),
        keys: None,
    }))
    .unwrap();
    // the token never goes in the title: window titles are readable by the whole desktop
    let out = String::from_utf8_lossy(&a.out).to_string();
    assert!(out.starts_with(&set_user_var("vtabs_token", "abc")));
    // and it lands before the ready that follows it, so `is_ready` passes when that arrives
    assert!(saw(&a, r#""t":"ready""#));
}

#[test]
fn control_commands_are_bound_to_the_initial_authenticated_session() {
    let mut a = app();
    let input = |token: &str, command| Input::Control {
        token: token.into(),
        command,
    };

    a.handle(input("stranger", Command::Ping { n: Some(1) }))
        .unwrap();
    assert!(
        a.out.is_empty(),
        "a non-auth command cannot establish a session"
    );
    a.handle(input(
        "header",
        Command::Auth {
            token: "payload".into(),
            caps: Vec::new(),
            keys: None,
        },
    ))
    .unwrap();
    assert!(
        a.out.is_empty(),
        "the initial auth must match its frame token"
    );

    a.handle(input(
        "session1",
        Command::Auth {
            token: "session1".into(),
            caps: Vec::new(),
            keys: None,
        },
    ))
    .unwrap();
    assert_eq!(a.token.as_deref(), Some("session1"));
    a.out.clear();

    a.handle(input("stranger", Command::Ping { n: Some(2) }))
        .unwrap();
    for privileged in [
        Command::Quit,
        Command::Kill {
            title: Some("wez-vtabs:forged".into()),
            pane: None,
        },
        Command::Rescue {
            band: 28,
            position: Some("left".into()),
        },
    ] {
        assert!(
            a.handle(input("stranger", privileged)).unwrap(),
            "a foreign session cannot quit the backend"
        );
    }
    assert!(
        a.out.is_empty(),
        "foreign ordinary and privileged commands are inert"
    );
    a.handle(input(
        "session2",
        Command::Auth {
            token: "session2".into(),
            caps: Vec::new(),
            keys: None,
        },
    ))
    .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&a.out),
        set_user_var("vtabs_token", "session1"),
        "an unproved rotation changes nothing; the held token is published for its next try"
    );
    assert_eq!(a.token.as_deref(), Some("session1"));
    a.out.clear();

    a.handle(input(
        "session1",
        Command::Auth {
            token: "session2".into(),
            caps: Vec::new(),
            keys: None,
        },
    ))
    .unwrap();
    assert_eq!(a.token.as_deref(), Some("session2"));
    a.out.clear();
    a.handle(input("session1", Command::Ping { n: Some(3) }))
        .unwrap();
    assert!(a.out.is_empty(), "the old token is stale after rotation");
    a.handle(input("session2", Command::Ping { n: Some(4) }))
        .unwrap();
    assert!(saw(&a, r#""t":"pong","echo":4"#));
}

fn payloads(a: &App<Vec<u8>>) -> Vec<String> {
    use base64::Engine as _;
    String::from_utf8_lossy(&a.out)
        .split("\x1b]1337;SetUserVar=vtabs=")
        .skip(1)
        .filter_map(|rest| rest.split('\x07').next())
        .map(|b| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(b).unwrap();
            String::from_utf8(bytes).unwrap()
        })
        .collect()
}

fn saw(a: &App<Vec<u8>>, needle: &str) -> bool {
    payloads(a).iter().any(|p| p.contains(needle))
}

#[test]
fn resize_emits_only_on_change() {
    let mut a = app();
    a.resize((28, 24)).unwrap();
    assert!(a.out.is_empty());
    a.resize((30, 24)).unwrap();
    assert_eq!(
        a.out,
        set_user_var("vtabs", r#"{"t":"resize","cols":30,"rows":24,"n":1}"#).as_bytes()
    );
}

#[test]
fn repeated_identical_keys_stay_distinct_user_vars() {
    let mut a = app();
    for _ in 0..2 {
        a.handle(Input::Key {
            name: "j".into(),
            mods: Mods::default(),
            raw: b"j".to_vec(),
        })
        .unwrap();
    }
    assert_eq!(
        payloads(&a),
        vec![
            r#"{"t":"key","key":"j","raw":"ag==","n":1}"#.to_string(),
            r#"{"t":"key","key":"j","raw":"ag==","n":2}"#.to_string(),
        ]
    );
}

#[test]
fn pong_echoes_the_ping_in_echo_and_carries_its_own_n() {
    let mut a = app();
    a.handle(Input::Key {
        name: "j".into(),
        mods: Mods::default(),
        raw: b"j".to_vec(),
    })
    .unwrap();
    a.handle(Input::Command(Command::Ping { n: Some(7) }))
        .unwrap();
    assert_eq!(payloads(&a)[1], r#"{"t":"pong","echo":7,"n":2}"#);
}

const CONFIG: &str = r#"{"rev":1,"position":"left","icons":true,
        "meta":"auto","meta_sep":" ","double_click_ms":300,"tear_off":true,"wheel":"scroll",
        "context":"popover","hover_timeout_ms":1500,
        "render":{"meta":true,"padding":{"left":1,"right":1,"top":0,"bottom":0},
        "tab_height":"card","separator":"gap","pinned_style":"compact","close_button":"hover",
        "scroll_indicator":"auto","new_tab_button":true,"new_tab_label":"New tab","hover":"follow"}}"#;
const THEME: &str = r#"{"rev":1,"scheme":{"ansi":[]},"overrides":{}}"#;
const MODEL: &str = r#"{"rev":1,"screen":"sidebar","active":1,
        "strip":{"buttons":[{"id":"toggle"},{"id":"new_tab"}]},
        "tabs":[{"id":1,"index":1,"title":"one"},{"id":2,"index":2,"title":"two"}]}"#;

fn dress(a: &mut App<Vec<u8>>) {
    a.handle(Input::Command(Command::Config(Box::new(
        serde_json::from_str(CONFIG).unwrap(),
    ))))
    .unwrap();
    a.handle(Input::Command(Command::Theme(Box::new(
        serde_json::from_str(THEME).unwrap(),
    ))))
    .unwrap();
    a.handle(Input::Command(Command::Model(Box::new(
        serde_json::from_str(MODEL).unwrap(),
    ))))
    .unwrap();
}

#[test]
fn rust_reports_changed_raw_fact_geometry_as_a_typed_host_effect_once() {
    let mut a = app();
    a.client_typed_intents = true;
    a.handle(Input::Command(Command::Config(Box::new(
        serde_json::from_str(CONFIG).unwrap(),
    ))))
    .unwrap();
    a.handle(Input::Command(Command::Theme(Box::new(
        serde_json::from_str(THEME).unwrap(),
    ))))
    .unwrap();
    model(
        &mut a,
        r#"{"rev":1,"screen":"sidebar","strip":{
            "metrics":{"cols":28,"viewport_rows":30,"pixel_width":235,"pixel_height":570},
            "chrome":{"is_mac":true,"integrated_buttons":true,"native_button_style":true},
            "buttons":[{"id":"toggle"}]},"tabs":[]}"#,
    );
    assert!(saw(&a, r#""t":"intent","a":"set_rail_reserve","cols":9"#));

    a.out.clear();
    a.repaint().unwrap();
    assert!(
        !saw(&a, "set_rail_reserve"),
        "an unchanged reserve is quiet"
    );

    model(
        &mut a,
        r#"{"rev":2,"screen":"sidebar","strip":{
            "chrome":{"is_mac":true,"integrated_buttons":true,"native_button_style":true,
                "is_full_screen":true},"buttons":[{"id":"toggle"}]},"tabs":[]}"#,
    );
    assert!(saw(&a, r#""t":"intent","a":"set_rail_reserve","cols":0"#));
}

fn click(kind: vtabs_protocol::MouseKind, x: u16, y: u16) -> Input {
    Input::Mouse(vtabs_protocol::Mouse {
        kind,
        button: vtabs_protocol::Button::Left,
        x,
        y,
        dy: 0,
        mods: Mods::default(),
    })
}

fn model(a: &mut App<Vec<u8>>, json: &str) {
    a.handle(Input::Command(Command::Model(Box::new(
        serde_json::from_str(json).unwrap(),
    ))))
    .unwrap();
}

#[test]
fn a_model_past_the_space_bound_is_dropped_whole() {
    let mut a = app();
    dress(&mut a);
    let spaces: Vec<String> = (0..=MODEL_MAX_SPACES)
        .map(|i| format!(r#"{{"id":"s{i}"}}"#))
        .collect();
    let over = format!(
        r#"{{"rev":2,"screen":"sidebar","active":1,"space":"s0","spaces":[{}],
                "tabs":[{{"id":1,"index":1,"title":"one"}}]}}"#,
        spaces.join(",")
    );
    model(&mut a, &over);
    assert!(saw(&a, r#""t":"dropped","what":"model","reason":"bounds""#));
    assert_eq!(
        a.v2.model.as_ref().unwrap().rev,
        1,
        "the previous model is kept"
    );
}

#[test]
fn a_press_on_a_space_icon_asks_lua_to_switch() {
    let mut a = app();
    dress(&mut a);
    model(
        &mut a,
        r#"{"rev":2,"screen":"sidebar","active":1,"space":"home",
                "spaces":[{"id":"home","icon":"~"},{"id":"work","icon":"w"}],
                "tabs":[{"id":1,"index":1,"title":"one"}]}"#,
    );
    let (row, x) = {
        let rows = a.dims().1;
        let e = a.scene().0;
        let plan = layout::plan(&e.view);
        let row = (1..=rows)
            .find(|&y| plan.at(y).kind == layout::RegionKind::Spaces)
            .expect("a switcher row");
        let x = plan
            .at(row)
            .spans
            .iter()
            .find(|s| s.id == "work")
            .map(|s| s.x1)
            .expect("the work slot");
        (row, x)
    };
    a.handle(click(
        vtabs_protocol::MouseKind::Press,
        x as u16,
        row as u16,
    ))
    .unwrap();
    assert!(saw(&a, r#""t":"do","a":"switch_space","id":"work""#));
}

#[test]
fn a_new_token_re_announces_ready_but_the_same_one_does_not() {
    let mut a = app();
    let auth = |a: &mut App<Vec<u8>>, token: &str| {
        a.handle(Input::Command(Command::Auth {
            token: token.into(),
            caps: Vec::new(),
            keys: None,
        }))
        .unwrap();
    };
    auth(&mut a, "first");
    assert!(saw(&a, r#""t":"ready""#), "the first auth announces");
    let after = payloads(&a).len();

    // Lua re-auths from its own ready branch; announcing again would ping-pong forever
    auth(&mut a, "first");
    assert_eq!(payloads(&a).len(), after, "the same token says nothing new");

    // a gui restart mints a fresh token: store.proto/store.paints must be restored
    auth(&mut a, "second");
    let sent = &payloads(&a)[after..];
    let ready = sent
        .iter()
        .find(|p| p.contains(r#""t":"ready""#))
        .expect("a new token re-announces");
    assert!(ready.contains(r#""paints":true"#));
    assert!(ready.contains(r#""v":3"#));
}

#[test]
fn auth_negotiates_theme_hooks_per_client_without_reannouncing() {
    let mut a = app();
    send(
        &mut a,
        Command::Auth {
            token: "client".into(),
            caps: vec!["theme_hooks".into()],
            keys: None,
        },
    );
    assert!(a.client_theme_hooks);
    let ready_count = payloads(&a).len();

    send(
        &mut a,
        Command::Auth {
            token: "client".into(),
            caps: Vec::new(),
            keys: None,
        },
    );
    assert!(!a.client_theme_hooks);
    assert_eq!(payloads(&a).len(), ready_count);
}

#[test]
fn auth_negotiates_spaces_policy_and_rejects_the_section_without_it() {
    let mut a = app();
    send(
        &mut a,
        Command::Auth {
            token: "client".into(),
            caps: vec!["spaces_policy".into()],
            keys: None,
        },
    );
    assert!(a.client_spaces_policy);
    let ready_count = payloads(&a).len();
    send(
        &mut a,
        Command::Auth {
            token: "client".into(),
            caps: Vec::new(),
            keys: None,
        },
    );
    assert!(!a.client_spaces_policy);
    assert_eq!(payloads(&a).len(), ready_count);

    a.out.clear();
    send(&mut a, spaces_cmd(SPACES));
    assert!(saw(
        &a,
        r#""t":"dropped","what":"spaces","reason":"unsupported""#
    ));
    assert!(a.v2.spaces.is_none());
}

#[test]
fn a_painting_backend_draws_the_pane_once_the_state_is_complete() {
    let mut a = app();
    a.handle(Input::Command(Command::Config(Box::new(
        serde_json::from_str(CONFIG).unwrap(),
    ))))
    .unwrap();
    assert!(
        !String::from_utf8_lossy(&a.out).contains("\x1b[?25l"),
        "config alone paints nothing"
    );
    dress(&mut a);
    let painted = String::from_utf8_lossy(&a.out).to_string();
    assert!(painted.contains("\x1b[?25l"), "the frame hides the cursor");
    assert!(
        painted.contains("one") && painted.contains("two"),
        "both tabs listed"
    );
}

#[test]
fn clear_wipes_the_pane_and_draws_it_again() {
    let mut a = app();
    dress(&mut a);
    a.out.clear();
    a.handle(Input::Command(Command::Clear)).unwrap();
    let out = painted(&a);
    // a repaint alone leaves untouched rows as they were; only the wipe clears stale bytes
    assert!(out.starts_with(CLEAR_SCREEN), "the pane is wiped first");
    assert!(out.contains("one") && out.contains("two"), "then redrawn");
}

const SETTINGS_MODEL: &str = r#"{"rev":2,"screen":"settings","version":"9.9.9",
        "groups":[{"id":"layout","label":"Layout"},{"id":"cards","label":"Cards"}],
        "fields":[
          {"key":"width","label":"width","group":"layout","widget":"stepper","value_text":"< 28 >"},
          {"key":"position","label":"position","group":"layout","widget":"picker","value_text":"< left >"}]}"#;

const SETTINGS: &str = r#"{"rev":3,"values":{"width":28,"hover":"follow","close_button":"hover","frame":{},"strip_actions":["search"],
        "spaces":[],"backend":{"env":{"A.B":"one"}}},
        "explicit":[],"host_values":[],"opaque":[],"key_defaults":{},
        "is_macos":false,"version":"9.9.9"}"#;

fn settings_cmd(json: &str) -> Command {
    Command::Settings(Box::new(serde_json::from_str(json).unwrap()))
}

#[test]
fn a_settings_model_paints_the_page_and_answers_for_its_own_keys() {
    let mut a = app();
    a.size = (100, 21);
    dress(&mut a);
    a.out.clear();
    a.handle(Input::Command(Command::Model(Box::new(
        serde_json::from_str(SETTINGS_MODEL).unwrap(),
    ))))
    .unwrap();
    assert!(a.v2.model.is_none(), "the legacy DTO is not retained");
    assert!(a.v2.settings.is_some());
    let painted = String::from_utf8_lossy(&a.out).to_string();
    assert!(painted.contains("Settings"), "the header names the screen");
    assert!(
        painted.contains("Layout") && painted.contains("width"),
        "nav and form: {painted:?}"
    );
    assert!(!painted.contains("one"), "the sidebar's tabs are not here");

    let before = payloads(&a).len();
    a.handle(Input::Key {
        name: "r".into(),
        mods: Mods::default(),
        raw: b"r".to_vec(),
    })
    .unwrap();
    let sent = &payloads(&a)[before..];
    assert!(
        sent.iter()
            .any(|p| p.contains(r#""a":"reset_option""#) && p.contains(r#""key":"width""#)),
        "the verb names the focused key: {sent:?}"
    );
    assert!(
        !sent.iter().any(|p| p.contains(r#""t":"key""#)),
        "the page owns the keyboard: {sent:?}"
    );
}

#[test]
fn a_settings_document_paints_and_emits_canonical_commit_and_copy_effects() {
    let mut a = app();
    a.size = (100, 21);
    send(&mut a, config_cmd(CONFIG));
    send(&mut a, theme_cmd(THEME));
    send(&mut a, settings_cmd(SETTINGS));
    assert!(a.v2.settings_document.is_some());
    assert!(painted(&a).contains("Settings"));

    a.out.clear();
    a.handle(Input::Key {
        name: "right".into(),
        mods: Mods::default(),
        raw: Vec::new(),
    })
    .unwrap();
    let sent = payloads(&a);
    let commit = sent
        .iter()
        .find(|payload| payload.contains(r#""t":"settings_commit""#))
        .expect("a settings commit effect");
    let commit: serde_json::Value = serde_json::from_str(commit).unwrap();
    assert_eq!(commit["settings_rev"], 3);
    assert_eq!(commit["path"], serde_json::json!(["width"]));
    assert_eq!(
        commit["change"],
        serde_json::json!({"op":"set","value":29.0})
    );
    assert_eq!(commit["mode"], "instant");
    assert!(
        commit["persistence_json"]
            .as_str()
            .unwrap()
            .contains(r#""width":29.0"#)
    );
    assert_eq!(
        a.v2.settings.as_ref().unwrap().fields[0].value_text,
        "‹ 29 ›"
    );

    a.out.clear();
    a.handle(Input::Key {
        name: "c".into(),
        mods: Mods::default(),
        raw: Vec::new(),
    })
    .unwrap();
    let copy = payloads(&a)
        .into_iter()
        .find(|payload| payload.contains(r#""t":"settings_copy""#))
        .expect("a settings copy effect");
    assert!(copy.contains("vtabs.apply_to_config"));
    assert!(copy.contains(r#"[\"width\"] = 29"#));

    a.out.clear();
    a.apply_document_intent(&Intent::NudgeOption {
        key: "hover".into(),
        delta: 1,
    })
    .unwrap();
    let commit = payloads(&a)
        .into_iter()
        .find(|payload| payload.contains(r#""t":"settings_commit""#))
        .expect("hover commit");
    let commit: serde_json::Value = serde_json::from_str(&commit).unwrap();
    assert_eq!(commit["path"], serde_json::json!(["hover"]));
    assert_eq!(
        commit["derived"],
        serde_json::json!([{"path":["close_button"],"op":"set","value":"always"}])
    );
    assert!(
        commit["persistence_json"]
            .as_str()
            .unwrap()
            .contains(r#""close_button":"always""#)
    );
}

fn app_with_persistence_body_size(target: usize) -> App<Vec<u8>> {
    fn document(filler: usize) -> (SettingsPresentation, SettingsDocument) {
        let mut values = serde_json::Map::new();
        values.insert("width".into(), serde_json::json!(28));
        values.insert(
            "icon_map".into(),
            serde_json::json!({"oversize": "x".repeat(filler)}),
        );
        let (_, presentation, document) = settings_input(v2::SettingsMsg {
            rev: 7,
            values: serde_json::Value::Object(values),
            explicit: Vec::new(),
            host_values: Vec::new(),
            opaque: Vec::new(),
            key_defaults: Default::default(),
            is_macos: false,
            version: None,
        })
        .expect("valid settings document");
        (presentation, document)
    }

    let (_, mut empty) = document(0);
    let overhead = match empty
        .apply(DocumentAction::Step {
            path: SettingPath::from_dotted("width"),
            delta: 1,
        })
        .unwrap()
    {
        DocumentEffect::Commit(effect) => effect.persistence_json.len(),
        effect => panic!("expected sizing commit, got {effect:?}"),
    };
    assert!(target >= overhead);
    let (presentation, document) = document(target - overhead);
    let mut changed = document.clone();
    let changed_size = match changed
        .apply(DocumentAction::Step {
            path: SettingPath::from_dotted("width"),
            delta: 1,
        })
        .unwrap()
    {
        DocumentEffect::Commit(effect) => effect.persistence_json.len(),
        effect => panic!("expected sizing commit, got {effect:?}"),
    };
    assert_eq!(changed_size, target);
    let mut a = app();
    a.v2.settings = Some(presentation);
    a.v2.settings_rev = Some(7);
    a.v2.settings_document = Some(document);
    a
}

#[test]
fn settings_commit_and_save_share_the_same_inclusive_body_limit() {
    let mut at_limit = app_with_persistence_body_size(SETTINGS_BODY_MAX_BYTES);
    assert!(
        at_limit
            .apply_document_intent(&Intent::NudgeOption {
                key: "width".into(),
                delta: 1,
            })
            .unwrap()
    );
    assert!(
        saw(&at_limit, r#""t":"settings_commit""#),
        "{:?}",
        payloads(&at_limit)
    );
    assert!(!saw(&at_limit, r#""t":"dropped""#));

    let mut over = app_with_persistence_body_size(SETTINGS_BODY_MAX_BYTES + 1);
    assert!(
        over.apply_document_intent(&Intent::NudgeOption {
            key: "width".into(),
            delta: 1,
        })
        .unwrap()
    );
    assert!(saw(
        &over,
        r#""t":"dropped","what":"settings","reason":"size""#
    ));
    assert!(!saw(&over, r#""t":"settings_commit""#));
    assert_eq!(
        over.v2.settings.as_ref().unwrap().fields[0].value_text,
        "‹ 28 ›",
        "the rejected edit rolls back the Rust document"
    );
}

#[test]
fn invalid_settings_invalidate_the_atomic_batch_and_legacy_model_clears_ownership() {
    let mut a = app();
    send(&mut a, Command::Begin { generation: 1 });
    send(
        &mut a,
        settings_cmd(r#"{"rev":1,"values":"not-an-object"}"#),
    );
    assert!(!a.v2.pending.as_ref().unwrap().valid);
    assert!(saw(
        &a,
        r#""t":"dropped","what":"settings","reason":"invalid""#
    ));

    let mut typed = app();
    send(&mut typed, Command::Begin { generation: 2 });
    send(
        &mut typed,
        settings_cmd(r#"{"rev":2,"values":{"width":"wide"}}"#),
    );
    assert!(!typed.v2.pending.as_ref().unwrap().valid);
    assert!(saw(
        &typed,
        r#""t":"dropped","what":"settings","reason":"invalid""#
    ));

    let mut legacy = app();
    send(&mut legacy, config_cmd(CONFIG));
    send(&mut legacy, theme_cmd(THEME));
    send(&mut legacy, settings_cmd(SETTINGS));
    assert!(legacy.v2.settings_document.is_some());
    send(&mut legacy, model_cmd(SETTINGS_MODEL));
    assert!(legacy.v2.settings_document.is_none());
    assert!(legacy.v2.model.is_none());
    assert!(legacy.v2.settings.is_some());
}

#[test]
fn generated_settings_fields_obey_the_model_bound() {
    let mut a = app();
    send(&mut a, Command::Begin { generation: 1 });
    let key_defaults = (0..MODEL_MAX_FIELDS)
        .map(|index| (format!("binding_{index}"), serde_json::json!({"key":"x"})))
        .collect();
    send(
        &mut a,
        Command::Settings(Box::new(v2::SettingsMsg {
            rev: 1,
            values: serde_json::json!({"keys": {}}),
            explicit: Vec::new(),
            host_values: Vec::new(),
            opaque: Vec::new(),
            key_defaults,
            is_macos: false,
            version: None,
        })),
    );
    assert!(!a.v2.pending.as_ref().unwrap().valid);
    assert!(saw(
        &a,
        r#""t":"dropped","what":"settings","reason":"bounds""#
    ));
}

#[test]
fn a_painting_backend_speaks_do_and_never_mouse() {
    let mut a = app();
    dress(&mut a);
    let before = payloads(&a).len();
    a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
        .unwrap();
    let sent = &payloads(&a)[before..];
    assert!(
        !sent.iter().any(|p| p.contains(r#""t":"mouse""#)),
        "mouse is gone: {sent:?}"
    );
    assert!(
        sent.iter().any(|p| p.contains(r#""t":"do""#)),
        "a gesture was reported: {sent:?}"
    );
}

#[test]
fn a_typed_intents_client_receives_the_flat_intent_contract() {
    let mut a = app();
    send(
        &mut a,
        Command::Auth {
            token: "typed".into(),
            caps: vec!["typed_intents".into()],
            keys: None,
        },
    );
    dress(&mut a);
    a.out.clear();
    a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
        .unwrap();

    let sent = payloads(&a);
    assert!(sent.iter().any(|payload| {
        payload.contains(r#""t":"intent","a":"press_card""#) && payload.contains(r#""tab_id":1"#)
    }));
    assert!(!sent.iter().any(|payload| payload.contains(r#""t":"do""#)));
}

#[test]
fn input_before_the_first_full_state_is_swallowed() {
    let mut a = app();
    a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
        .unwrap();
    assert!(a.out.is_empty(), "nothing is drawn and nothing is reported");
}

#[test]
fn hover_expiry_clears_the_highlight_without_a_round_trip() {
    let mut a = app();
    dress(&mut a);
    a.handle(Input::Mouse(vtabs_protocol::Mouse {
        kind: vtabs_protocol::MouseKind::Move,
        button: vtabs_protocol::Button::None,
        x: 6,
        y: 3,
        dy: 0,
        mods: Mods::default(),
    }))
    .unwrap();
    assert!(a.ui.hover.is_some());
    assert!(a.next_hover().is_some(), "the clock is armed");
    a.hover_deadline = Some(Instant::now());
    a.started = Instant::now() - std::time::Duration::from_secs(60);
    a.tick_hover(Instant::now()).unwrap();
    assert!(a.ui.hover.is_none(), "stale hover is dropped");
    assert!(a.next_hover().is_none());
}

fn hover_at(a: &mut App<Vec<u8>>, x: u16, y: u16) {
    a.handle(Input::Mouse(vtabs_protocol::Mouse {
        kind: vtabs_protocol::MouseKind::Move,
        button: vtabs_protocol::Button::None,
        x,
        y,
        dy: 0,
        mods: Mods::default(),
    }))
    .unwrap();
}

#[test]
fn focus_leaving_the_pane_clears_the_hover_locally_and_sends_nothing() {
    let mut a = app();
    dress(&mut a);
    // the first row whose hover changes the frame: the active card lights nothing new
    for y in 2..=10 {
        a.out.clear();
        hover_at(&mut a, 6, y);
        if !a.out.is_empty() {
            break;
        }
    }
    assert!(a.ui.hover.is_some() && !a.out.is_empty(), "a row lit up");
    a.out.clear();
    a.handle(Input::Focus(false)).unwrap();
    assert!(a.ui.hover.is_none(), "the pointer left with the focus");
    let out = painted(&a);
    assert!(
        !out.contains("SetUserVar"),
        "nothing crossed to Lua: {out:?}"
    );
    assert!(cups(&out) >= 1, "the lit row is painted back: {out:?}");
    a.out.clear();
    a.handle(Input::Focus(true)).unwrap();
    assert!(a.out.is_empty(), "focus coming back changes nothing");
}

#[test]
fn hover_highlight_off_drops_motion_tracking_and_lights_no_row() {
    let mut a = app();
    dress(&mut a);
    a.out.clear();
    let off = CONFIG.replace(
        r#""hover_timeout_ms":1500,"#,
        r#""hover_timeout_ms":1500,"hover_highlight":false,"#,
    );
    send(
        &mut a,
        Command::Config(Box::new(serde_json::from_str(&off).unwrap())),
    );
    assert!(
        painted(&a).contains(MOTION_OFF),
        "any-motion reporting is switched off"
    );
    a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
        .unwrap();
    assert!(a.ui.hover.is_none(), "a press lights no row");
    a.out.clear();
    send(
        &mut a,
        Command::Config(Box::new(serde_json::from_str(CONFIG).unwrap())),
    );
    assert!(painted(&a).contains(MOTION_ON), "and back on");
    a.out.clear();
    send(
        &mut a,
        Command::Config(Box::new(serde_json::from_str(CONFIG).unwrap())),
    );
    assert!(!painted(&a).contains(MOTION_ON), "unchanged: not re-sent");
}

const MENU: &str = r#"{"rev":1,"open":true,"level":"root","anchor":{"row":3,"col":2},
        "target":1,"selected":1,"header":{"title":"one"},
        "items":[{"id":"activate","label":"Switch to tab"},
                 {"id":"close","label":"Close tab","danger":true}]}"#;
fn send(a: &mut App<Vec<u8>>, cmd: Command) {
    a.handle(Input::Command(cmd)).unwrap();
}

fn menu(json: &str) -> Command {
    Command::Menu(Box::new(serde_json::from_str(json).unwrap()))
}

fn painted(a: &App<Vec<u8>>) -> String {
    String::from_utf8_lossy(&a.out).to_string()
}

fn config_cmd(json: &str) -> Command {
    Command::Config(Box::new(serde_json::from_str(json).unwrap()))
}

fn theme_cmd(json: &str) -> Command {
    Command::Theme(Box::new(serde_json::from_str(json).unwrap()))
}

fn model_cmd(json: &str) -> Command {
    Command::Model(Box::new(serde_json::from_str(json).unwrap()))
}

fn spaces_cmd(json: &str) -> Command {
    Command::Spaces(Box::new(serde_json::from_str(json).unwrap()))
}

const SPACES: &str = r##"{"rev":1,"window_id":42,"enabled":true,
    "definitions":[{"id":"home","name":"Home"},{"id":"claude","name":"Claude","theme":{"accent":"#f5c2e7"},"match":{"proc":"claude"}}],
    "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh"},{"id":2,"index":2,"title":"two","proc":"claude"}],
    "active_tab":2,"active_space":"home","dynamics":[],"last_tabs":[]}"##;

const SPACES_HOOKED: &str = r##"{"rev":2,"window_id":42,"enabled":true,"hook":true,
    "definitions":[{"id":"home","name":"Home"},{"id":"claude","name":"Claude","theme":{"accent":"#f5c2e7"},"match":{"proc":"claude"}}],
    "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh"},{"id":2,"index":2,"title":"two","proc":"claude"}],
    "active_tab":2,"active_space":"home","dynamics":[],"last_tabs":[]}"##;

const HOOKED_PALETTE_THEME: &str = r##"{"rev":2,"hook":true,
    "scheme":{"background":"#1e1e2e","foreground":"#cdd6f4",
    "ansi":["#000000","#110000","#220000","#330000","#440000","#550000","#660000","#770000"],
    "brights":["#880000","#990000","#aa0000","#bb0000","#cc0000","#dd0000","#ee0000","#ff0000"]},
    "overrides":{}}"##;

fn stage_spaced(a: &mut App<Vec<u8>>, generation: u64, model: &str, theme: &str, spaces: &str) {
    send(a, Command::Begin { generation });
    send(a, config_cmd(CONFIG));
    send(a, theme_cmd(theme));
    send(a, spaces_cmd(spaces));
    send(a, model_cmd(model));
}

fn stage_complete(a: &mut App<Vec<u8>>, generation: u64, model: &str) {
    send(a, Command::Begin { generation });
    send(a, config_cmd(CONFIG));
    send(a, theme_cmd(THEME));
    send(a, model_cmd(model));
    send(a, menu(r#"{"rev":1,"open":false}"#));
}

fn stage_hooked(a: &mut App<Vec<u8>>, generation: u64, model: &str) {
    send(a, Command::Begin { generation });
    send(a, config_cmd(CONFIG));
    send(
        a,
        theme_cmd(
            r##"{"rev":2,"hook":true,"scheme":{"background":"#1e1e2e","foreground":"#cdd6f4"},"overrides":{"accent":"#89b4fa"}}"##,
        ),
    );
    send(a, model_cmd(model));
}

#[test]
fn an_atomic_generation_paints_once_after_all_four_sections() {
    let mut a = app();
    stage_complete(&mut a, 1, MODEL);
    assert!(a.out.is_empty(), "staged sections are not visible");

    send(&mut a, Command::Commit { generation: 1 });
    let out = painted(&a);
    assert_eq!(out.matches(SYNC_BEGIN).count(), 1, "one committed paint");
    assert!(out.contains("one") && out.contains("two"));
    assert_eq!(a.v2.committed_generation, Some(1));
    assert!(a.v2.pending.is_none());
    assert_eq!(a.metrics.committed_generations, 1);
    assert_eq!(a.metrics.terminal_paints, 1);
    assert!(a.metrics.paint_bytes > 0);
    assert!(
        a.paint_log_line()
            .contains("totals={commits=1,paints=1,bytes=")
    );
}

#[test]
fn a_model_only_delta_commits_over_the_cloned_atomic_state() {
    let mut a = app();
    stage_complete(&mut a, 1, MODEL);
    send(&mut a, Command::Commit { generation: 1 });
    a.out.clear();

    send(&mut a, Command::Begin { generation: 2 });
    send(
        &mut a,
        model_cmd(r#"{"rev":2,"screen":"sidebar","tabs":[{"id":1,"index":1,"title":"delta"}]}"#),
    );
    assert!(a.out.is_empty(), "the delta remains staged");
    send(&mut a, Command::Commit { generation: 2 });

    let out = painted(&a);
    assert_eq!(out.matches(SYNC_BEGIN).count(), 1);
    assert!(out.contains("delta"));
    assert!(
        saw(&a, r#""t":"theme_resolved","generation":2"#),
        "every committed generation carries its own projection answer"
    );
    assert_eq!(a.v2.committed_generation, Some(2));
    assert_eq!(a.v2.model.as_ref().unwrap().rev, 2);
}

#[test]
fn a_hooked_generation_waits_then_publishes_and_paints_exactly_once() {
    let mut a = app();
    a.client_theme_hooks = true;
    stage_hooked(&mut a, 1, MODEL);

    send(&mut a, Command::Commit { generation: 1 });
    assert!(saw(&a, r#""t":"theme_hook_request","generation":1"#));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 0);
    assert!(a.v2.committed_generation.is_none());
    assert!(a.v2.pending.as_ref().unwrap().hook_requested);
    assert_eq!(a.metrics.committed_generations, 0);
    assert_eq!(a.metrics.terminal_paints, 0);
    let requests = payloads(&a).len();
    send(&mut a, Command::Commit { generation: 1 });
    assert_eq!(payloads(&a).len(), requests, "the request is not repeated");

    a.out.clear();
    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 1,
            overrides: Box::new(v2::ThemeOverrides {
                bg: Some("#123456".into()),
                ..Default::default()
            }),
        },
    );
    let out = painted(&a);
    assert_eq!(out.matches(SYNC_BEGIN).count(), 1, "one completed paint");
    assert!(saw(&a, r#""t":"theme_resolved","generation":1"#));
    assert_eq!(a.v2.committed_generation, Some(1));
    assert!(a.v2.pending.is_none());
    assert_eq!(a.metrics.committed_generations, 1);
    assert_eq!(a.metrics.terminal_paints, 1);
    assert_eq!(a.v2.resolved_theme.as_ref().unwrap().bg, [0x12, 0x34, 0x56]);
    assert!(
        a.v2.theme.as_ref().unwrap().overrides.bg.is_none(),
        "the hook overlays the raw base without replacing it"
    );

    a.out.clear();
    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 1,
            overrides: Box::default(),
        },
    );
    assert!(a.out.is_empty(), "a duplicate result is inert");
}

#[test]
fn a_space_route_hook_then_theme_hook_publishes_and_paints_once() {
    let mut a = app();
    a.client_spaces_policy = true;
    a.client_theme_hooks = true;
    stage_spaced(&mut a, 11, MODEL, HOOKED_PALETTE_THEME, SPACES_HOOKED);

    send(&mut a, Command::Commit { generation: 11 });
    assert!(saw(
        &a,
        r#""t":"space_route_hook_request","generation":11,"window_id":42"#
    ));
    assert!(!saw(&a, r#""t":"theme_hook_request""#));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 0);
    assert_eq!(a.metrics.committed_generations, 0);

    a.out.clear();
    send(
        &mut a,
        Command::SpaceRouteHookResult {
            generation: 11,
            routes: vec![
                v2::SpaceRouteHookAnswer {
                    tab_id: 1,
                    space: None,
                },
                v2::SpaceRouteHookAnswer {
                    tab_id: 2,
                    space: Some("claude".into()),
                },
            ],
        },
    );
    assert!(saw(&a, r#""t":"theme_hook_request","generation":11"#));
    assert!(
        saw(&a, r#""accent":[245,194,231]"#),
        "the selected space layer is part of the base passed to the theme hook"
    );
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 0);

    a.out.clear();
    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 11,
            overrides: Box::new(v2::ThemeOverrides {
                bg: Some("#123456".into()),
                ..Default::default()
            }),
        },
    );
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 1);
    assert!(saw(
        &a,
        r#""t":"spaces_resolved","generation":11,"window_id":42,"active":"claude""#
    ));
    assert!(saw(&a, r#""t":"theme_resolved","generation":11"#));
    assert_eq!(a.metrics.committed_generations, 1);
    assert_eq!(a.metrics.terminal_paints, 1);
    assert_eq!(a.v2.committed_generation, Some(11));
    assert_eq!(a.v2.resolved_theme.as_ref().unwrap().bg, [0x12, 0x34, 0x56]);
    let sidebar = a.v2.model.as_ref().unwrap().sidebar().unwrap();
    assert_eq!(sidebar.space.as_deref(), Some("claude"));
    assert_eq!(
        sidebar.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
        [2]
    );
}

#[test]
fn route_hook_answers_must_be_exact_and_stale_results_are_inert() {
    assert!(App::<Vec<u8>>::valid_space_answer(
        &[1, 2],
        &[
            v2::SpaceRouteHookAnswer {
                tab_id: 2,
                space: None,
            },
            v2::SpaceRouteHookAnswer {
                tab_id: 1,
                space: None,
            },
        ]
    ));
    for invalid in [
        vec![v2::SpaceRouteHookAnswer {
            tab_id: 1,
            space: None,
        }],
        vec![
            v2::SpaceRouteHookAnswer {
                tab_id: 1,
                space: None,
            },
            v2::SpaceRouteHookAnswer {
                tab_id: 1,
                space: None,
            },
        ],
        vec![
            v2::SpaceRouteHookAnswer {
                tab_id: 1,
                space: None,
            },
            v2::SpaceRouteHookAnswer {
                tab_id: 9,
                space: None,
            },
        ],
    ] {
        assert!(!App::<Vec<u8>>::valid_space_answer(&[1, 2], &invalid));
    }

    let mut a = app();
    a.client_spaces_policy = true;
    stage_spaced(&mut a, 12, MODEL, THEME, SPACES_HOOKED);
    send(&mut a, Command::Commit { generation: 12 });
    a.out.clear();
    send(
        &mut a,
        Command::SpaceRouteHookResult {
            generation: 11,
            routes: Vec::new(),
        },
    );
    assert!(a.out.is_empty(), "a stale generation changes nothing");

    send(
        &mut a,
        Command::SpaceRouteHookResult {
            generation: 12,
            routes: vec![v2::SpaceRouteHookAnswer {
                tab_id: 1,
                space: None,
            }],
        },
    );
    assert!(saw(
        &a,
        r#""t":"dropped","what":"space_route_hook_result","reason":"invalid""#
    ));
    assert_eq!(a.v2.committed_generation, Some(12));
    assert!(a.v2.pending.is_none());
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 1);
}

#[test]
fn sidebar_and_settings_use_the_same_rust_selected_space_theme() {
    let resolve_for = |model: &str| {
        let mut a = app();
        a.size = (100, 21);
        a.client_spaces_policy = true;
        stage_spaced(&mut a, 13, model, THEME, SPACES);
        send(&mut a, Command::Commit { generation: 13 });
        assert!(saw(
            &a,
            r#""t":"spaces_resolved","generation":13,"window_id":42,"active":"claude""#
        ));
        assert_eq!(a.metrics.terminal_paints, 1);
        a.v2.resolved_theme.clone().unwrap()
    };
    let sidebar = resolve_for(MODEL);
    let settings = resolve_for(SETTINGS_MODEL);
    assert_eq!(sidebar, settings);
    assert_eq!(sidebar.accent, [0xf5, 0xc2, 0xe7]);
}

#[test]
fn an_invalid_spaces_delta_retains_the_committed_generation() {
    let mut a = app();
    a.client_spaces_policy = true;
    stage_spaced(&mut a, 1, MODEL, THEME, SPACES);
    send(&mut a, Command::Commit { generation: 1 });
    let committed = a.v2.space_resolution.clone().unwrap();
    a.out.clear();

    let mut oversized: v2::SpacesMsg = serde_json::from_str(SPACES).unwrap();
    oversized.dynamics = (0..=MODEL_MAX_SPACES)
        .map(|seq| v2::DynamicSpace {
            id: format!("s{seq}"),
            name: format!("S {seq}"),
            template: None,
            seq: seq as u64,
        })
        .collect();
    send(&mut a, Command::Begin { generation: 2 });
    send(&mut a, Command::Spaces(Box::new(oversized)));
    send(&mut a, Command::Commit { generation: 2 });

    assert_eq!(a.v2.committed_generation, Some(1));
    assert_eq!(a.v2.space_resolution.as_ref(), Some(&committed));
    assert_eq!(a.metrics.terminal_paints, 1);
    assert!(saw(
        &a,
        r#""t":"dropped","what":"spaces","reason":"bounds""#
    ));
}

#[test]
fn a_generation_is_frozen_once_its_hook_base_has_left_the_process() {
    let mut a = app();
    a.client_theme_hooks = true;
    stage_hooked(&mut a, 1, MODEL);
    send(&mut a, Command::Commit { generation: 1 });
    a.out.clear();

    // These untagged sections cannot be allowed to change the meaning of the outstanding request.
    send(&mut a, config_cmd(r#"{"rev":9,"rail_width":1}"#));
    send(
        &mut a,
        theme_cmd(r##"{"rev":9,"hook":true,"overrides":{"bg":"#ffffff"}}"##),
    );
    send(
        &mut a,
        model_cmd(r#"{"rev":9,"screen":"sidebar","private":true,"tabs":[]}"#),
    );
    assert!(
        a.out.is_empty(),
        "frozen sections are ignored without validation"
    );

    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 1,
            overrides: Box::default(),
        },
    );
    assert_eq!(a.v2.theme.as_ref().unwrap().rev, 2);
    assert_eq!(a.v2.model.as_ref().unwrap().rev, 1);
    assert_eq!(a.v2.resolved_theme.as_ref().unwrap().bg, [0x17, 0x17, 0x23]);
    assert_eq!(a.v2.committed_generation, Some(1));
}

#[test]
fn hook_results_are_generation_bound_and_a_newer_begin_replaces_the_wait() {
    let mut a = app();
    a.client_theme_hooks = true;
    stage_hooked(&mut a, 2, MODEL);
    send(&mut a, Command::Commit { generation: 2 });
    a.out.clear();

    send(&mut a, Command::Begin { generation: 3 });
    send(&mut a, config_cmd(CONFIG));
    send(
        &mut a,
        theme_cmd(r##"{"rev":3,"hook":true,"overrides":{"bg":"#222222"}}"##),
    );
    send(&mut a, model_cmd(MODEL));
    send(&mut a, Command::Commit { generation: 3 });
    assert!(saw(&a, r#""t":"theme_hook_request","generation":3"#));
    a.out.clear();

    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 2,
            overrides: Box::new(v2::ThemeOverrides {
                bg: Some("#ffffff".into()),
                ..Default::default()
            }),
        },
    );
    assert!(a.out.is_empty(), "the replaced generation cannot publish");
    assert_eq!(a.v2.pending.as_ref().unwrap().generation, 3);

    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 3,
            overrides: Box::default(),
        },
    );
    assert_eq!(a.v2.committed_generation, Some(3));
    assert_eq!(a.v2.resolved_theme.as_ref().unwrap().bg, [0x22, 0x22, 0x22]);
}

#[test]
fn a_rejected_begin_quarantines_its_untagged_sections_until_its_commit() {
    let mut a = app();
    stage_complete(&mut a, 5, MODEL);
    send(&mut a, Command::Begin { generation: 4 });
    assert_eq!(a.v2.discarding_generation, Some(4));

    send(
        &mut a,
        theme_cmd(r##"{"rev":4,"overrides":{"bg":"#ffffff"}}"##),
    );
    send(
        &mut a,
        model_cmd(r#"{"rev":4,"screen":"sidebar","tabs":[{"id":4,"index":1,"title":"stale"}]}"#),
    );
    send(&mut a, Command::Commit { generation: 4 });
    assert_eq!(a.v2.discarding_generation, None);
    assert_eq!(a.v2.pending.as_ref().unwrap().generation, 5);

    send(&mut a, Command::Commit { generation: 5 });
    assert_eq!(a.v2.committed_generation, Some(5));
    assert_eq!(a.v2.theme.as_ref().unwrap().rev, 1);
    assert_eq!(a.v2.model.as_ref().unwrap().rev, 1);
    assert!(painted(&a).contains("one"));
    assert!(!painted(&a).contains("stale"));
}

#[test]
fn an_equal_generation_replay_restarts_a_partially_delivered_build() {
    let mut a = app();
    send(&mut a, Command::Begin { generation: 7 });
    send(&mut a, config_cmd(CONFIG));
    assert_eq!(a.v2.pending.as_ref().unwrap().seen, SEEN_CONFIG);

    // The writer did not advance its sent ledger after a failed write, so it retries the same full
    // generation. The old prefix is discarded and the replay is accepted.
    send(&mut a, Command::Begin { generation: 7 });
    assert_eq!(a.v2.pending.as_ref().unwrap().seen, 0);
    send(&mut a, config_cmd(CONFIG));
    send(&mut a, theme_cmd(THEME));
    send(&mut a, model_cmd(MODEL));
    send(&mut a, Command::Commit { generation: 7 });

    assert_eq!(a.v2.committed_generation, Some(7));
    assert!(a.v2.pending.is_none());
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 1);
}

#[test]
fn an_invalid_theme_hook_result_commits_the_deterministic_base() {
    let mut a = app();
    a.client_theme_hooks = true;
    stage_hooked(&mut a, 4, MODEL);
    send(&mut a, Command::Commit { generation: 4 });
    a.out.clear();

    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 4,
            overrides: Box::new(v2::ThemeOverrides {
                accent: Some("not-a-colour".into()),
                ..Default::default()
            }),
        },
    );
    assert!(saw(
        &a,
        r#""t":"dropped","what":"theme_hook_result","reason":"invalid""#
    ));
    assert_eq!(a.v2.committed_generation, Some(4));
    assert!(a.v2.pending.is_none());
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 1);
    assert_eq!(a.v2.resolved_theme.as_ref().unwrap().bg, [0x17, 0x17, 0x23]);

    a.out.clear();
    send(&mut a, Command::Commit { generation: 4 });
    assert!(a.out.is_empty());
    assert!(a.v2.pending.is_none());
}

#[test]
fn a_theme_hook_retries_once_then_falls_back_and_late_results_are_inert() {
    let mut a = app();
    a.client_theme_hooks = true;
    stage_hooked(&mut a, 21, MODEL);
    send(&mut a, Command::Commit { generation: 21 });
    let first = payloads(&a)
        .into_iter()
        .find(|payload| payload.contains(r#""t":"theme_hook_request""#))
        .expect("initial hook request");
    a.out.clear();

    let retry_at = Instant::now();
    a.v2.pending.as_mut().unwrap().hook_deadline = Some(retry_at);
    a.tick_hooks(retry_at).unwrap();
    let retry = payloads(&a)
        .into_iter()
        .find(|payload| payload.contains(r#""t":"theme_hook_request""#))
        .expect("one retry");
    assert_eq!(
        first.split(r#","n":"#).next(),
        retry.split(r#","n":"#).next(),
        "the retry preserves the exact hook request"
    );
    assert_eq!(a.metrics.terminal_paints, 0);
    a.out.clear();

    let fallback_at = Instant::now();
    a.v2.pending.as_mut().unwrap().hook_deadline = Some(fallback_at);
    a.tick_hooks(fallback_at).unwrap();
    assert!(saw(
        &a,
        r#""t":"dropped","what":"theme_hook","reason":"timeout""#
    ));
    assert_eq!(a.v2.committed_generation, Some(21));
    assert!(a.v2.pending.is_none());
    assert_eq!(a.metrics.committed_generations, 1);
    assert_eq!(a.metrics.terminal_paints, 1);

    a.out.clear();
    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 21,
            overrides: Box::new(v2::ThemeOverrides {
                bg: Some("#ffffff".into()),
                ..Default::default()
            }),
        },
    );
    assert!(
        a.out.is_empty(),
        "a late answer cannot repaint the fallback"
    );
    assert_eq!(a.v2.resolved_theme.as_ref().unwrap().bg, [0x17, 0x17, 0x23]);
}

#[test]
fn route_hook_timeout_falls_through_rules_before_requesting_the_theme_hook() {
    let mut a = app();
    a.client_spaces_policy = true;
    a.client_theme_hooks = true;
    stage_spaced(&mut a, 22, MODEL, HOOKED_PALETTE_THEME, SPACES_HOOKED);
    send(&mut a, Command::Commit { generation: 22 });
    a.out.clear();

    for _ in 0..=HOOK_RETRIES {
        let now = Instant::now();
        a.v2.pending.as_mut().unwrap().hook_deadline = Some(now);
        a.tick_hooks(now).unwrap();
    }
    assert!(saw(
        &a,
        r#""t":"dropped","what":"space_route_hook","reason":"timeout""#
    ));
    assert!(saw(&a, r#""t":"theme_hook_request","generation":22"#));
    let pending = a.v2.pending.as_ref().unwrap();
    assert!(pending.space_hook_requested.is_none());
    assert!(pending.hook_requested);
    assert_eq!(a.metrics.terminal_paints, 0);
}

#[test]
fn hook_true_without_the_client_capability_commits_the_rust_base() {
    let mut a = app();
    stage_hooked(&mut a, 5, MODEL);
    send(&mut a, Command::Commit { generation: 5 });

    assert!(!saw(&a, r#""t":"theme_hook_request""#));
    assert!(saw(&a, r#""t":"theme_resolved","generation":5"#));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 1);
    assert_eq!(a.v2.committed_generation, Some(5));
}

#[test]
fn a_settings_generation_can_complete_the_same_three_section_hook_round_trip() {
    let mut a = app();
    a.size = (100, 21);
    a.client_theme_hooks = true;
    stage_hooked(&mut a, 6, SETTINGS_MODEL);
    send(&mut a, Command::Commit { generation: 6 });
    assert!(saw(&a, r#""t":"theme_hook_request","generation":6"#));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 0);

    a.out.clear();
    send(
        &mut a,
        Command::ThemeHookResult {
            generation: 6,
            overrides: Box::default(),
        },
    );
    assert!(painted(&a).contains("Settings"));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 1);
    assert_eq!(a.v2.committed_generation, Some(6));
}

#[test]
fn private_theme_input_is_window_global_for_sidebar_and_settings_models() {
    let resolve_for = |model: &str| {
        let mut a = app();
        send(&mut a, Command::Begin { generation: 1 });
        send(&mut a, config_cmd(CONFIG));
        send(
            &mut a,
            theme_cmd(r#"{"rev":1,"private":true,"overrides":{}}"#),
        );
        send(&mut a, model_cmd(model));
        send(&mut a, Command::Commit { generation: 1 });
        a.v2.resolved_theme.clone().unwrap()
    };
    let sidebar = resolve_for(MODEL);
    let settings = resolve_for(SETTINGS_MODEL);
    assert_eq!(sidebar, settings);
    assert_eq!(sidebar.accent, sidebar.private_accent);
}

#[test]
fn a_private_model_delta_recomputes_and_reports_the_effective_theme() {
    let mut a = app();
    stage_complete(&mut a, 1, MODEL);
    send(&mut a, Command::Commit { generation: 1 });
    let public_accent = a.v2.resolved_theme.as_ref().unwrap().accent;
    a.out.clear();

    send(&mut a, Command::Begin { generation: 2 });
    send(
        &mut a,
        model_cmd(
            r#"{"rev":2,"screen":"sidebar","private":true,"tabs":[{"id":1,"index":1,"title":"private"}]}"#,
        ),
    );
    send(&mut a, Command::Commit { generation: 2 });

    let private = a.v2.resolved_theme.as_ref().unwrap();
    assert_ne!(private.accent, public_accent);
    assert_eq!(private.accent, private.private_accent);
    assert!(saw(&a, r#""t":"theme_resolved","generation":2"#));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 1);
}

#[test]
fn a_partial_or_mismatched_commit_changes_nothing() {
    let mut a = app();
    send(&mut a, Command::Begin { generation: 2 });
    send(
        &mut a,
        model_cmd(r#"{"rev":2,"screen":"sidebar","tabs":[{"id":9,"index":1,"title":"staged"}]}"#),
    );

    send(&mut a, Command::Commit { generation: 3 });
    assert!(a.v2.pending.is_some(), "a mismatched commit is ignored");
    send(&mut a, Command::Commit { generation: 2 });

    assert!(a.out.is_empty(), "neither commit paints");
    assert!(a.v2.model.is_none());
    assert!(a.v2.committed_generation.is_none());
    assert!(a.v2.pending.is_none(), "a partial matching commit is spent");
}

#[test]
fn an_initial_settings_generation_needs_no_menu_section() {
    let mut a = app();
    a.size = (100, 21);
    send(&mut a, Command::Begin { generation: 1 });
    send(&mut a, config_cmd(CONFIG));
    send(&mut a, theme_cmd(THEME));
    send(&mut a, model_cmd(SETTINGS_MODEL));
    assert!(a.out.is_empty());

    send(&mut a, Command::Commit { generation: 1 });

    let out = painted(&a);
    assert_eq!(out.matches(SYNC_BEGIN).count(), 1);
    assert!(out.contains("Settings") && out.contains("Layout"));
    assert_eq!(a.v2.committed_generation, Some(1));
    assert!(a.v2.menu.is_none());
}

#[test]
fn stale_generations_cannot_replace_committed_state() {
    let mut a = app();
    stage_complete(&mut a, 5, MODEL);
    send(&mut a, Command::Commit { generation: 5 });
    a.out.clear();

    send(&mut a, Command::Begin { generation: 4 });
    send(
        &mut a,
        model_cmd(r#"{"rev":4,"screen":"sidebar","tabs":[{"id":4,"index":1,"title":"stale"}]}"#),
    );
    send(&mut a, Command::Commit { generation: 4 });

    assert!(a.out.is_empty());
    assert_eq!(a.v2.committed_generation, Some(5));
    assert_eq!(a.v2.model.as_ref().unwrap().rev, 1);
    assert!(a.v2.pending.is_none());
}

#[test]
fn a_bounds_failure_invalidates_the_whole_generation() {
    let mut a = app();
    dress(&mut a);
    a.out.clear();
    send(&mut a, Command::Begin { generation: 2 });
    send(&mut a, config_cmd(CONFIG));
    send(&mut a, theme_cmd(THEME));
    send(&mut a, menu(r#"{"rev":2,"open":false}"#));
    let spaces: Vec<String> = (0..=MODEL_MAX_SPACES)
        .map(|i| format!(r#"{{"id":"s{i}"}}"#))
        .collect();
    send(
        &mut a,
        model_cmd(&format!(
            r#"{{"rev":2,"screen":"sidebar","spaces":[{}],"tabs":[]}}"#,
            spaces.join(",")
        )),
    );
    send(&mut a, Command::Commit { generation: 2 });

    assert!(saw(&a, r#""t":"dropped","what":"model","reason":"bounds""#));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 0);
    assert_eq!(a.v2.model.as_ref().unwrap().rev, 1);
    assert!(a.v2.committed_generation.is_none());
    assert!(a.v2.pending.is_none());
}

#[test]
fn a_parser_drop_invalidates_the_pending_generation() {
    let mut a = app();
    a.token = Some("session".into());
    stage_complete(&mut a, 1, MODEL);
    a.handle(Input::Dropped {
        token: Some("stranger".into()),
        what: "line",
        reason: "size",
    })
    .unwrap();
    assert!(a.v2.pending.as_ref().unwrap().valid);
    assert!(!saw(&a, r#""t":"dropped","what":"line""#));
    a.handle(Input::Dropped {
        token: Some("session".into()),
        what: "line",
        reason: "size",
    })
    .unwrap();
    send(&mut a, Command::Commit { generation: 1 });

    assert!(saw(&a, r#""t":"dropped","what":"line","reason":"size""#));
    assert_eq!(painted(&a).matches(SYNC_BEGIN).count(), 0);
    assert!(a.v2.model.is_none());
    assert!(a.v2.pending.is_none());
}

#[test]
fn a_new_auth_resets_committed_pending_and_local_ui_before_ready() {
    let mut a = app();
    a.client_theme_hooks = true;
    stage_complete(&mut a, 7, MODEL);
    send(&mut a, Command::Commit { generation: 7 });
    send(&mut a, Command::Begin { generation: 8 });
    a.ui.set_hover(3, 4, 5);
    a.ui.scroll = Some(2);
    a.menu_ui.buffer = "rename".into();
    a.settings_ui.filter = "needle".into();
    a.popover = a.scene().0.popover;
    a.out.clear();

    send(
        &mut a,
        Command::Auth {
            token: "replacement".into(),
            caps: Vec::new(),
            keys: None,
        },
    );

    assert!(
        a.v2.config.is_none()
            && a.v2.theme.is_none()
            && a.v2.spaces.is_none()
            && a.v2.space_resolution.is_none()
            && a.v2.model.is_none()
    );
    assert!(a.v2.menu.is_none());
    assert!(a.v2.pending.is_none() && a.v2.committed_generation.is_none());
    assert!(!a.v2.atomic);
    assert!(!a.client_theme_hooks && !a.client_spaces_policy && a.last_reported_theme.is_none());
    assert_eq!(a.ui, UiState::default());
    assert_eq!(a.menu_ui, MenuState::default());
    assert_eq!(a.settings_ui, SettingsUi::default());
    assert!(a.popover.is_none() && a.last_rows.is_none());

    let out = painted(&a);
    let cleared = out.find(CLEAR_SCREEN).expect("auth clears the old pane");
    let ready = out
        .find("\x1b]1337;SetUserVar=vtabs=")
        .expect("auth announces ready");
    assert!(cleared < ready, "clear precedes Ready: {out:?}");
    assert!(payloads(&a).iter().any(|payload| {
        payload.contains(r#""t":"ready""#)
            && payload.contains(
                r#""caps":["atomic_sync","typed_intents","theme_hooks","settings_document","spaces_policy","inbox_transport"]"#,
            )
    }));
}

#[test]
fn a_fade_runs_on_the_frame_shown_and_a_repaint_lands_it_on_the_final_frame() {
    let mut a = app();
    dress(&mut a);
    // the first frame is whole; a repaint after the fade must write the same whole frame back
    let output = painted(&a);
    let final_frame = output[output.find(SYNC_BEGIN).expect("the initial frame")..].to_string();
    a.out.clear();
    send(
        &mut a,
        Command::Fx(v2::FxMsg {
            phase: "expand_in".into(),
            ms: Some(200),
            fps: Some(30),
        }),
    );
    assert!(a.fx.is_some(), "the fade is running");
    assert_ne!(
        painted(&a),
        final_frame,
        "t=0 is the anchor colour, not the final frame"
    );
    a.out.clear();
    a.repaint().unwrap();
    assert!(a.fx.is_none(), "a state repaint ends the fade");
    assert_eq!(
        painted(&a),
        final_frame,
        "and lands exactly on the final frame"
    );
    send(
        &mut a,
        Command::Fx(v2::FxMsg {
            phase: "nope".into(),
            ms: None,
            fps: None,
        }),
    );
    assert!(a.fx.is_none(), "an unknown phase plays nothing");
}

#[test]
fn an_open_menu_paints_and_a_closed_one_draws_nothing() {
    let mut a = app();
    dress(&mut a);
    a.out.clear();
    send(&mut a, menu(MENU));
    assert!(painted(&a).contains("Switch to tab"), "the widget drew it");

    a.out.clear();
    send(&mut a, menu(r#"{"rev":3,"open":false}"#));
    assert!(
        !painted(&a).contains("Switch to tab"),
        "a closed menu leaves the list alone"
    );
}

#[test]
fn a_menu_that_cannot_be_placed_is_refused_once_and_draws_nothing() {
    let mut a = app();
    a.size = (28, 2);
    dress(&mut a);
    let before = payloads(&a).len();
    send(&mut a, menu(MENU));
    let sent = &payloads(&a)[before..];
    let note = sent
        .iter()
        .find(|p| p.contains(r#""t":"note""#))
        .expect("a refusal");
    assert!(note.contains(r#""k":"menu_refused""#));
    assert!(note.contains(r#""why":"rows""#));
    assert!(note.contains(r#""id":1"#) && note.contains(r#""a":"root""#));
    assert!(!painted(&a).contains("Switch to tab"), "nothing is drawn");

    let after = payloads(&a).len();
    a.repaint().unwrap();
    assert_eq!(payloads(&a).len(), after, "and the note is not repeated");
}

#[test]
fn while_the_menu_is_open_the_pane_answers_to_it_and_not_to_the_list() {
    let mut a = app();
    dress(&mut a);
    send(&mut a, menu(MENU));
    let hits = a.popover.clone().expect("the menu placed itself");
    let row = hits
        .rows
        .iter()
        .position(|(id, _)| id.as_deref() == Some("activate"))
        .expect("the item was drawn");
    let (x, y) = ((hits.x + 1) as u16, (hits.y + row as i64) as u16);

    let before = payloads(&a).len();
    a.handle(click(vtabs_protocol::MouseKind::Press, x, y))
        .unwrap();
    let sent = &payloads(&a)[before..];
    assert!(
        sent.iter().any(|p| p.contains(r#""a":"menu_pick""#)),
        "the item ran: {sent:?}"
    );
    assert!(
        !sent.iter().any(|p| p.contains(r#""a":"press_card""#)),
        "and the list under it never saw the click"
    );

    // keys belong to the menu too: escape closes it instead of blurring the sidebar
    let before = payloads(&a).len();
    a.handle(Input::Key {
        name: "escape".into(),
        mods: Mods::default(),
        raw: b"\x1b".to_vec(),
    })
    .unwrap();
    let sent = &payloads(&a)[before..];
    assert!(sent.iter().any(|p| p.contains(r#""a":"menu_closed""#)));
    assert!(!sent.iter().any(|p| p.contains(r#""a":"blur_sidebar""#)));
}

#[test]
fn key_events_are_emitted() {
    let mut a = app();
    a.handle(Input::Key {
        name: "x".into(),
        mods: Mods::default(),
        raw: b"x".to_vec(),
    })
    .unwrap();
    assert_eq!(
        a.out,
        set_user_var("vtabs", r#"{"t":"key","key":"x","raw":"eA==","n":1}"#).as_bytes()
    );
}

const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";

#[test]
fn a_frame_is_bracketed_in_synchronized_output() {
    let mut a = app();
    dress(&mut a);
    let out = painted(&a);
    let frame = &out[out.find(SYNC_BEGIN).expect("a synchronized frame")..];
    assert!(frame.starts_with(SYNC_BEGIN), "{out:?}");
    assert!(frame.ends_with(SYNC_END), "{out:?}");
}

fn cups(out: &str) -> usize {
    out.matches(";1H").count()
}

#[test]
fn a_repaint_writes_only_the_rows_that_changed() {
    let mut a = app();
    dress(&mut a);
    let whole = cups(&painted(&a));
    assert!(whole > 2, "the first frame paints every row: {whole}");
    a.out.clear();
    model(
        &mut a,
        r#"{"rev":2,"screen":"sidebar","active":1,
                "strip":{"buttons":[{"id":"toggle"},{"id":"new_tab"}]},
                "tabs":[{"id":1,"index":1,"title":"one"},{"id":2,"index":2,"title":"deux"}]}"#,
    );
    let out = painted(&a);
    assert!(out.contains("deux"), "the new title is drawn: {out:?}");
    assert!(
        cups(&out) >= 1 && cups(&out) < whole / 2,
        "a title change rewrites its own rows, not the pane: {} of {whole}",
        cups(&out)
    );
    assert!(out.starts_with(SYNC_BEGIN) && out.ends_with(SYNC_END));
}

#[test]
fn a_repaint_of_the_frame_already_shown_writes_nothing() {
    let mut a = app();
    dress(&mut a);
    probe_returns((28, 24));
    a.out.clear();
    let metrics = a.metrics;
    a.repaint().unwrap();
    assert!(a.out.is_empty(), "{:?}", painted(&a));
    assert_eq!(a.metrics, metrics, "a no-op repaint is not counted");
}

#[test]
fn a_size_change_seen_at_paint_time_is_announced_and_clears() {
    let mut a = app();
    dress(&mut a);
    a.out.clear();
    probe_returns((30, 24));
    model(&mut a, MODEL);
    assert!(saw(&a, r#""t":"resize","cols":30,"rows":24"#));
    let out = painted(&a);
    let announced = out.find("SetUserVar").expect("the resize event");
    let cleared = out.find(CLEAR_SCREEN).expect("a wipe");
    let framed = out.find(SYNC_BEGIN).expect("the frame");
    assert!(announced < cleared && cleared < framed, "{out:?}");
    assert_eq!(a.size, (30, 24));
}

#[test]
fn a_resize_mid_fade_cancels_the_fade() {
    let mut a = app();
    dress(&mut a);
    send(
        &mut a,
        Command::Fx(v2::FxMsg {
            phase: "expand_in".into(),
            ms: Some(200),
            fps: Some(30),
        }),
    );
    let at = a.next_fx().expect("the fade is running");
    probe_returns((30, 24));
    a.out.clear();
    a.tick_fx(at).unwrap();
    assert!(a.fx.is_none(), "the fade is dropped");
    assert_eq!(a.size, (30, 24));
    assert!(saw(&a, r#""t":"resize","cols":30"#));
    assert!(painted(&a).contains(CLEAR_SCREEN), "the pane is wiped");
}

// --- adoption, adjust and resize bursts ----------------------------------------------------------

#[test]
fn an_auth_the_session_cannot_read_is_answered_with_the_token_it_holds_once_a_second() {
    let mut a = app();
    let auth = |frame: &str, claimed: &str| Input::Control {
        token: frame.into(),
        command: Command::Auth {
            token: claimed.into(),
            caps: Vec::new(),
            keys: None,
        },
    };
    a.handle(auth("old", "old")).unwrap();
    a.out.clear();
    // a GUI that just attached to the mux frames blind, with the fresh token it minted
    a.handle(auth("new", "new")).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&a.out),
        set_user_var("vtabs_token", "old"),
        "the held token is published again, and nothing else"
    );
    assert_eq!(
        a.token.as_deref(),
        Some("old"),
        "the session is not taken over"
    );
    a.out.clear();
    a.handle(auth("new", "new")).unwrap();
    assert!(a.out.is_empty(), "once a second at most");
    a.handle(Input::Control {
        token: "stranger".into(),
        command: Command::Ping { n: Some(1) },
    })
    .unwrap();
    assert!(a.out.is_empty(), "only an auth is answered");
    // framed with the token it just learned, the same auth rotates the session
    a.handle(auth("old", "new")).unwrap();
    assert!(String::from_utf8_lossy(&a.out).starts_with(&set_user_var("vtabs_token", "new")));
    assert!(saw(&a, r#""t":"ready""#));
    assert_eq!(a.token.as_deref(), Some("new"));
}

#[test]
fn a_resize_burst_is_adopted_once_it_pauses_and_reported_once() {
    let mut a = app();
    dress(&mut a);
    a.out.clear();
    let t0 = Instant::now();
    probe_returns((30, 24));
    a.note_resize(t0);
    assert!(a.out.is_empty(), "a frame is noted, not adopted");
    assert_eq!(a.next_resize(), Some(t0 + RESIZE_DEBOUNCE));
    probe_returns((31, 24));
    let t1 = t0 + Duration::from_millis(30);
    a.note_resize(t1);
    assert_eq!(
        a.next_resize(),
        Some(t1 + RESIZE_DEBOUNCE),
        "each frame pushes the pause out"
    );
    a.tick_resize(t1 + RESIZE_DEBOUNCE - Duration::from_millis(1))
        .unwrap();
    assert!(a.out.is_empty());
    a.tick_resize(t1 + RESIZE_DEBOUNCE).unwrap();
    let reports = payloads(&a)
        .iter()
        .filter(|p| p.contains(r#""t":"resize""#))
        .count();
    assert_eq!(reports, 1, "one report for the burst: {:?}", payloads(&a));
    assert!(saw(&a, r#""t":"resize","cols":31,"rows":24"#));
    assert_eq!(a.size, (31, 24));
    assert!(
        painted(&a).contains(CLEAR_SCREEN),
        "and one frame at the new size"
    );
    assert!(a.next_resize().is_none());
    // a frame back to the size the pane already has withdraws the note
    probe_returns((32, 24));
    a.note_resize(t1);
    probe_returns((31, 24));
    a.note_resize(t1);
    assert!(a.next_resize().is_none());
}

#[test]
fn a_burst_that_never_pauses_is_adopted_after_the_cap() {
    let mut a = app();
    let t0 = Instant::now();
    let mut now = t0;
    for i in 0..10u16 {
        probe_returns((30 + i, 24));
        a.note_resize(now);
        now += Duration::from_millis(30);
    }
    assert_eq!(a.next_resize(), Some(t0 + RESIZE_MAX_WAIT));
    a.tick_resize(now).unwrap();
    assert_eq!(a.size, (39, 24));
    assert!(saw(&a, r#""t":"resize","cols":39"#));
}

#[cfg(unix)]
#[test]
fn an_adjust_with_a_target_works_the_delta_out_on_the_server_and_answers_with_the_width() {
    use std::os::unix::fs::PermissionsExt;
    let dir = transport_root("adjust-target");
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("calls.log");
    let list = r#"[{"window_id":0,"tab_id":7,"pane_id":1,"title":"wez-vtabs:abcd","left_col":0,"size":{"cols":36,"rows":24},"is_active":true},{"window_id":0,"tab_id":7,"pane_id":2,"title":"zsh","left_col":37,"size":{"cols":83,"rows":24},"is_active":false}]"#;
    let script = dir.join("wezterm");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nshift 2\n[ \"$1\" = --prefer-mux ] && shift\nverb=$1\nshift\necho \"$verb $*\" >> '{}'\n[ \"$verb\" = list ] && printf '%s' '{list}'\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut a = app();
    a.cli = Some(Cli::at(script, 1));
    a.size = (36, 24);
    probe_returns((36, 24));
    a.out.clear();
    // the mirror the plugin read was 60 wide: the delta it worked out is wrong, the target is not
    a.handle(Input::Command(Command::Adjust {
        direction: "Left".into(),
        amount: 32,
        park: false,
        target: Some(28),
        min_content: Some(20),
    }))
    .unwrap();
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("adjust-pane-size --pane-id 1 --amount 8 Left"),
        "{calls}"
    );
    assert!(!calls.contains("--amount 32"), "{calls}");
    let answer = payloads(&a)
        .into_iter()
        .find(|p| p.contains(r#""op":"adjust""#))
        .expect("the cli answer");
    assert!(
        answer.contains(r#""ok":true"#) && answer.contains(r#""cols":36"#),
        "{answer}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- inbox transport ---------------------------------------------------------------------------

const CONTROL: &str = "\x1eVTABS ";

fn transport_root(tag: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("vtabs-app-inbox-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn record(token: &str, json: &str) -> Vec<u8> {
    format!("{CONTROL}{token} {json}\n").into_bytes()
}

fn write_msg(dir: &std::path::Path, seq: u32, bytes: &[u8]) {
    std::fs::write(dir.join(format!("{seq:08}.msg")), bytes).unwrap();
}

fn negotiating(root: &std::path::Path) -> App<Vec<u8>> {
    let mut a = app();
    a.token = Some("t".into());
    let inbox = Inbox::create(root).unwrap();
    a.transport = Transport::Negotiating { inbox };
    a
}

fn session_dir(a: &App<Vec<u8>>) -> (String, std::path::PathBuf) {
    match &a.transport {
        Transport::Negotiating { inbox } | Transport::Active { inbox, .. } => {
            (inbox.session().to_owned(), inbox.dir().to_owned())
        }
        Transport::Off => panic!("no session"),
    }
}

fn active_scan(a: &App<Vec<u8>>) -> Batch {
    match &a.transport {
        Transport::Active { inbox, .. } => Batch {
            session: inbox.session().to_owned(),
            messages: inbox.scan(),
        },
        _ => panic!("not active"),
    }
}

#[test]
fn ready_offers_an_inbox_only_when_a_root_was_given() {
    let root = transport_root("offer");
    let mut a = app();
    a.inbox = Some(Offer {
        root: root.clone(),
        wake: std::sync::Arc::new(|_| true),
    });
    a.handle(Input::Command(Command::Auth {
        token: "t".into(),
        caps: Vec::new(),
        keys: None,
    }))
    .unwrap();
    let ready = payloads(&a)
        .into_iter()
        .find(|p| p.contains(r#""t":"ready""#))
        .expect("ready");
    assert!(ready.contains(r#""transport":{"inbox":"inbox-"#), "{ready}");
    assert!(
        !ready.contains(r#""pane""#),
        "no cli means no server pane id"
    );
    assert!(matches!(a.transport, Transport::Negotiating { .. }));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_barrier_acks_a_present_probe_and_refuses_a_missing_one() {
    let root = transport_root("barrier-ok");
    let mut a = negotiating(&root);
    let (session, dir) = session_dir(&a);
    write_msg(
        &dir,
        1,
        &record(
            "t",
            &format!(r#"{{"t":"transport_probe","session":"{session}"}}"#),
        ),
    );
    a.handle(Input::Command(Command::TransportBarrier {
        session: session.clone(),
    }))
    .unwrap();
    assert!(saw(
        &a,
        &format!(r#""t":"transport_ready","session":"{session}""#)
    ));
    assert!(matches!(a.transport, Transport::Active { last_seq: 1, .. }));
    assert!(!dir.join("00000001.msg").exists(), "the probe is consumed");
    let _ = std::fs::remove_dir_all(&root);

    let root = transport_root("barrier-noprobe");
    let mut a = negotiating(&root);
    let (session, _) = session_dir(&a);
    a.handle(Input::Command(Command::TransportBarrier {
        session: session.clone(),
    }))
    .unwrap();
    assert!(saw(
        &a,
        &format!(r#""t":"transport_refused","session":"{session}","why":"probe""#)
    ));
    assert!(matches!(a.transport, Transport::Off));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_barrier_for_another_session_or_no_session_is_refused_without_dropping_this_one() {
    let root = transport_root("barrier-wrong");
    let mut a = negotiating(&root);
    a.handle(Input::Command(Command::TransportBarrier {
        session: "inbox-1-strange".into(),
    }))
    .unwrap();
    assert!(saw(&a, r#""why":"session""#));
    assert!(matches!(a.transport, Transport::Negotiating { .. }));

    let mut off = app();
    off.token = Some("t".into());
    off.handle(Input::Command(Command::TransportBarrier {
        session: "inbox-1-strange".into(),
    }))
    .unwrap();
    assert!(saw(&off, r#""why":"state""#));
    let _ = std::fs::remove_dir_all(&root);
}

fn activate(root: &std::path::Path) -> App<Vec<u8>> {
    let mut a = negotiating(root);
    let (session, dir) = session_dir(&a);
    write_msg(
        &dir,
        1,
        &record(
            "t",
            &format!(r#"{{"t":"transport_probe","session":"{session}"}}"#),
        ),
    );
    a.handle(Input::Command(Command::TransportBarrier { session }))
        .unwrap();
    a.out.clear();
    a
}

#[test]
fn an_active_inbox_applies_in_order_and_a_duplicate_is_removed_unprocessed() {
    let root = transport_root("active-dup");
    let mut a = activate(&root);
    let (_, dir) = session_dir(&a);
    write_msg(&dir, 1, &record("t", r#"{"t":"ping","n":9}"#));
    write_msg(&dir, 2, &record("t", r#"{"t":"ping","n":7}"#));
    let batch = active_scan(&a);
    a.inbox_batch(batch).unwrap();
    let pongs: Vec<_> = payloads(&a)
        .into_iter()
        .filter(|p| p.contains(r#""t":"pong""#))
        .collect();
    assert_eq!(pongs.len(), 1, "the duplicate seq 1 never ran: {pongs:?}");
    assert!(pongs[0].contains(r#""echo":7"#));
    assert!(matches!(a.transport, Transport::Active { last_seq: 2, .. }));
    assert!(!dir.join("00000001.msg").exists() && !dir.join("00000002.msg").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_gap_holds_until_the_grace_expires_then_reports_the_missing_message() {
    let root = transport_root("gap");
    let mut a = activate(&root);
    let (_, dir) = session_dir(&a);
    write_msg(&dir, 3, &record("t", r#"{"t":"ping","n":5}"#));
    let batch = active_scan(&a);
    a.inbox_batch(batch).unwrap();
    assert!(
        !saw(&a, r#""t":"pong""#),
        "seq 3 waits behind the hole at seq 2"
    );
    assert!(a.next_transport().is_some(), "the grace clock is running");

    a.tick_transport(Instant::now() + Duration::from_millis(150))
        .unwrap();
    assert!(saw(
        &a,
        r#""t":"dropped","what":"message","reason":"gap","seq":2"#
    ));
    assert!(
        saw(&a, r#""t":"pong","echo":5"#),
        "seq 3 runs once 2 is lost"
    );
    assert!(matches!(a.transport, Transport::Active { last_seq: 3, .. }));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_stop_drains_the_directory_in_order_then_returns_to_stdin_and_removes_it() {
    let root = transport_root("stop");
    let mut a = activate(&root);
    let (session, dir) = session_dir(&a);
    write_msg(&dir, 2, &record("t", r#"{"t":"ping","n":1}"#));
    write_msg(&dir, 3, &record("t", r#"{"t":"ping","n":2}"#));
    a.handle(Input::Command(Command::TransportStop {
        session: session.clone(),
    }))
    .unwrap();
    let echoes: Vec<_> = payloads(&a)
        .into_iter()
        .filter_map(|p| p.contains(r#""t":"pong""#).then(|| p.clone()))
        .collect();
    assert_eq!(
        echoes.len(),
        2,
        "both remaining messages drained: {echoes:?}"
    );
    assert!(echoes[0].contains(r#""echo":1"#) && echoes[1].contains(r#""echo":2"#));
    assert!(matches!(a.transport, Transport::Off));
    assert!(!dir.exists(), "the session directory is removed on stop");

    let mut a = activate(&root);
    a.handle(Input::Command(Command::TransportStop {
        session: "inbox-9-elsewhere".into(),
    }))
    .unwrap();
    assert!(
        matches!(a.transport, Transport::Active { .. }),
        "a stop for another session is inert"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_key_is_forwarded_server_side_only_when_a_cli_is_present() {
    let mut a = app();
    a.server_keys = true;
    a.handle(Input::Key {
        name: "x".into(),
        mods: Mods::default(),
        raw: b"x".to_vec(),
    })
    .unwrap();
    let key = payloads(&a)
        .into_iter()
        .find(|p| p.contains(r#""t":"key""#))
        .expect("a key event");
    assert!(
        !key.contains("delivered"),
        "with no cli the key is left to Lua: {key}"
    );

    let mut plain = app();
    plain
        .handle(Input::Key {
            name: "y".into(),
            mods: Mods::default(),
            raw: b"y".to_vec(),
        })
        .unwrap();
    assert!(
        !saw(&plain, "delivered"),
        "server delivery is off by default"
    );
}

#[test]
fn kill_dispatches_by_pane_id_and_by_title() {
    let mut a = app();
    for kill in [
        Command::Kill {
            title: None,
            pane: Some(7),
        },
        Command::Kill {
            title: Some("wez-vtabs:abcd".into()),
            pane: None,
        },
    ] {
        a.handle(Input::Command(kill)).unwrap();
    }
    let kills: Vec<_> = payloads(&a)
        .into_iter()
        .filter(|p| p.contains(r#""op":"kill""#))
        .collect();
    assert_eq!(
        kills.len(),
        2,
        "both kill forms reach the cli path: {kills:?}"
    );
    assert!(
        kills
            .iter()
            .all(|p| p.contains(r#""ok":false,"detail":"no cli here""#)),
        "{kills:?}"
    );
}

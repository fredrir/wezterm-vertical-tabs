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
        cli: None,
    }
}

#[test]
fn a_kill_or_rescue_with_no_cli_to_run_reports_the_failure() {
    let mut a = app();
    a.handle(Input::Command(Command::Kill {
        title: "wez-vtabs:abcd".into(),
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
    }))
    .unwrap();
    // the token never goes in the title: window titles are readable by the whole desktop
    let out = String::from_utf8_lossy(&a.out).to_string();
    assert!(out.starts_with(&set_user_var("vtabs_token", "abc")));
    // and it lands before the ready that follows it, so `is_ready` passes when that arrives
    assert!(saw(&a, r#""t":"ready""#));
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

const CONFIG: &str = r#"{"rev":1,"desired_width":28,"position":"left","icons":true,
        "meta":"auto","meta_sep":" ","double_click_ms":300,"tear_off":true,"wheel":"scroll",
        "context":"popover","hover_timeout_ms":1500,
        "render":{"meta":true,"padding":{"left":1,"right":1,"top":0,"bottom":0},
        "tab_height":"card","separator":"gap","pinned_style":"compact","close_button":"hover",
        "scroll_indicator":"auto","new_tab_button":true,"new_tab_label":"New tab","hover":"follow"}}"#;
const THEME: &str = r#"{"rev":1,"scheme":{"ansi":[],"brights":[]},"overrides":{}}"#;
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
    assert!(ready.contains(r#""v":2"#));
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

#[test]
fn a_fade_runs_on_the_frame_shown_and_a_repaint_lands_it_on_the_final_frame() {
    let mut a = app();
    dress(&mut a);
    // the first frame is whole; a repaint after the fade must write the same whole frame back
    let final_frame = painted(&a);
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
    assert!(out.starts_with(SYNC_BEGIN), "{out:?}");
    assert!(out.ends_with(SYNC_END), "{out:?}");
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
    a.repaint().unwrap();
    assert!(a.out.is_empty(), "{:?}", painted(&a));
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

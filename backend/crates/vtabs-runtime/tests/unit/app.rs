use super::*;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vtabs_engine::menu::Level;

thread_local! {
    static PROBE: Cell<Option<(u16, u16)>> = const { Cell::new(None) };
}

fn test_probe() -> Option<(u16, u16)> {
    PROBE.with(Cell::get)
}

fn probe_returns(size: (u16, u16)) {
    PROBE.with(|probe| probe.set(Some(size)));
}

fn app() -> App<Vec<u8>> {
    App {
        out: Vec::new(),
        log: Logger::from_env(),
        var: "vtabs".into(),
        size: (28, 24),
        probe: test_probe,
        pixel_probe: || None,
        needs_clear: false,
        fx: None,
        last_rows: None,
        shown_is_final: false,
        seq: 0,
        sync: SyncState::default(),
        ui: Default::default(),
        started: Instant::now(),
        popover: None,
        settings_ui: Default::default(),
        menu_ui: Default::default(),
        menu_refused: false,
        hover_deadline: None,
        token: None,
        token_announced: None,
        resize: None,
        last_reported_theme: None,
        last_rail_reserve: None,
        cli: Some(Cli::at(PathBuf::from("wezterm"), 77)),
        inbox: None,
        transport: Default::default(),
        server_keys: false,
        metrics: Default::default(),
    }
}

fn send(app: &mut App<Vec<u8>>, command: Command) {
    app.handle(Input::Command(command)).unwrap();
}

fn payloads(app: &App<Vec<u8>>) -> Vec<String> {
    use base64::Engine as _;
    String::from_utf8_lossy(&app.out)
        .split("\x1b]1337;SetUserVar=vtabs=")
        .skip(1)
        .filter_map(|rest| rest.split('\x07').next())
        .map(|body| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(body)
                .unwrap();
            String::from_utf8(bytes).unwrap()
        })
        .collect()
}

fn saw(app: &App<Vec<u8>>, needle: &str) -> bool {
    payloads(app).iter().any(|payload| payload.contains(needle))
}

fn painted(app: &App<Vec<u8>>) -> String {
    String::from_utf8_lossy(&app.out).to_string()
}

const CONFIG: &str = r#"{
    "position":"left","icons":true,"meta":"auto","meta_sep":" ",
    "double_click_ms":300,"tear_off":true,"wheel":"scroll","context":"popover",
    "hover_timeout_ms":1500,
    "render":{"meta":true,"padding":{"left":1,"right":1,"top":0,"bottom":0},
      "tab_height":"card","separator":"gap","pinned_style":"compact",
      "close_button":"hover","scroll_indicator":"auto","new_tab_button":true,
      "new_tab_label":"New tab","hover":"follow"}
}"#;
const THEME: &str = r#"{"private":false,"scheme":{"ansi":[]},"overrides":{}}"#;
const MODEL: &str = r#"{
    "active":1,"focus":{"on":false,"index":1},"scroll":{"top":0,"user":false},
    "strip":{"buttons":[{"id":"toggle_sidebar"},{"id":"new_tab"}]},"footer":[]
}"#;
const SPACES: &str = r#"{
    "window_id":42,"enabled":true,"definitions":[{"id":"home","name":"Home"}],
    "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh"},
            {"id":2,"index":2,"title":"two","proc":"nvim"}],
    "active_tab":1,"active_space":"home","dynamics":[],"last_tabs":[]
}"#;
const MENU_CLOSED: &str = r#"{"open":false}"#;
const SETTINGS: &str = r#"{
    "values":{"width":28},"explicit":[],"host_values":[],"opaque":[],
    "key_defaults":{},"is_macos":false,"version":"9.9.9"
}"#;

fn config() -> Command {
    Command::Config(Box::new(serde_json::from_str(CONFIG).unwrap()))
}

fn theme(body: &str) -> Command {
    Command::Theme(Box::new(serde_json::from_str(body).unwrap()))
}

fn model(body: &str) -> Command {
    Command::Model(Box::new(serde_json::from_str(body).unwrap()))
}

fn spaces(body: &str) -> Command {
    Command::Spaces(Box::new(serde_json::from_str(body).unwrap()))
}

fn menu(body: &str) -> Command {
    Command::Menu(Box::new(serde_json::from_str(body).unwrap()))
}

fn settings(body: &str) -> Command {
    Command::Settings(Box::new(serde_json::from_str(body).unwrap()))
}

fn stage_sidebar(app: &mut App<Vec<u8>>, theme_body: &str) {
    send(app, Command::Begin);
    send(app, config());
    send(app, theme(theme_body));
    send(app, spaces(SPACES));
    send(app, model(MODEL));
    send(app, menu(MENU_CLOSED));
}

fn commit_sidebar(app: &mut App<Vec<u8>>) {
    stage_sidebar(app, THEME);
    send(app, Command::Commit);
}

fn commit_settings(app: &mut App<Vec<u8>>) {
    send(app, Command::Begin);
    send(app, config());
    send(app, theme(THEME));
    send(app, spaces(SPACES));
    send(app, settings(SETTINGS));
    send(app, Command::Commit);
}

#[test]
fn maintenance_commands_use_only_current_shapes() {
    let mut app = app();
    app.cli = None;
    send(&mut app, Command::Kill { pane: 9 });
    send(
        &mut app,
        Command::Rescue {
            band: 28,
            position: "left".into(),
        },
    );
    send(
        &mut app,
        Command::Adjust {
            target: 32,
            min_content: 20,
        },
    );
    let sent = payloads(&app);
    assert!(sent[0].contains(r#""op":"kill","ok":false"#));
    assert!(sent[1].contains(r#""op":"rescue","ok":false"#));
    assert!(sent[2].contains(r#""op":"adjust","ok":false"#));
}

#[test]
fn auth_announces_the_backend_pane() {
    let mut app = app();
    send(
        &mut app,
        Command::Auth {
            token: "current".into(),
            keys: Some("server".into()),
        },
    );
    assert_eq!(app.token.as_deref(), Some("current"));
    assert!(app.server_keys);
    let ready = payloads(&app)
        .into_iter()
        .find(|payload| payload.contains(r#""t":"ready""#))
        .unwrap();
    assert!(ready.contains(r#""pane":77"#));
}

#[test]
fn control_commands_remain_bound_to_the_authenticated_session() {
    let mut app = app();
    let framed = |token: &str, command| Input::Control {
        token: token.into(),
        command,
    };
    app.handle(framed(
        "session",
        Command::Auth {
            token: "session".into(),
            keys: None,
        },
    ))
    .unwrap();
    app.out.clear();
    app.handle(framed("stranger", Command::Quit)).unwrap();
    assert!(app.out.is_empty());
    assert!(
        app.handle(framed("session", Command::Ping { n: Some(4) }))
            .unwrap()
    );
    assert!(saw(&app, r#""t":"pong","echo":4"#));
}

#[test]
fn sections_outside_a_transaction_are_inert() {
    let mut app = app();
    send(&mut app, config());
    send(&mut app, theme(THEME));
    send(&mut app, spaces(SPACES));
    send(&mut app, model(MODEL));
    send(&mut app, menu(MENU_CLOSED));
    assert!(app.sync.config.is_none());
    assert!(app.out.is_empty());
}

#[test]
fn first_transaction_is_complete_and_spaces_owns_the_tab_census() {
    let mut app = app();
    stage_sidebar(&mut app, THEME);
    assert!(app.out.is_empty());
    send(&mut app, Command::Commit);
    assert_eq!(app.metrics.commits, 1);
    assert_eq!(app.metrics.terminal_paints, 1);
    let model = app.sync.model.as_ref().unwrap();
    assert_eq!(model.tabs.len(), 2);
    assert_eq!(model.tabs[0].title, "one");
    assert_eq!(model.space.as_deref(), Some("home"));
    assert!(painted(&app).contains("one") && painted(&app).contains("two"));
    assert!(saw(&app, r#""t":"spaces_resolved""#));
    assert!(saw(&app, r#""t":"theme_resolved""#));
}

#[test]
fn incomplete_first_transaction_is_discarded() {
    let mut app = app();
    send(&mut app, Command::Begin);
    send(&mut app, config());
    send(&mut app, theme(THEME));
    send(&mut app, Command::Commit);
    assert!(app.sync.pending.is_none());
    assert!(app.sync.config.is_none());
    assert!(app.out.is_empty());
}

#[test]
fn later_transactions_may_replace_only_changed_sections() {
    let mut app = app();
    commit_sidebar(&mut app);
    app.out.clear();
    send(&mut app, Command::Begin);
    send(
        &mut app,
        model(r#"{"active":2,"focus":{"on":true,"index":2}}"#),
    );
    send(&mut app, Command::Commit);
    assert_eq!(app.metrics.commits, 2);
    assert_eq!(app.sync.model.as_ref().unwrap().active, Some(2));
    assert_eq!(app.sync.model.as_ref().unwrap().tabs.len(), 2);
}

#[test]
fn malformed_section_poisoning_prevents_a_cloned_transaction_from_committing() {
    let mut app = app();
    commit_sidebar(&mut app);
    app.token = Some("session".into());
    app.out.clear();

    let framed = |json: &str| format!("\x1eVTABS session {json}\n");
    let bytes = [
        framed(r#"{"t":"begin"}"#),
        // `screen` was removed from the model section. This must poison the transaction rather
        // than letting the committed model silently stand in for it.
        framed(r#"{"t":"model","screen":"sidebar","active":2}"#),
        framed(r#"{"t":"menu","open":false}"#),
        framed(r#"{"t":"commit"}"#),
    ]
    .concat();
    let mut parser = crate::input::Parser::new();
    for input in parser.feed(bytes.as_bytes()) {
        app.handle(input).unwrap();
    }

    assert_eq!(app.metrics.commits, 1);
    assert_eq!(app.sync.model.as_ref().unwrap().active, Some(1));
    assert!(app.sync.pending.is_none());
    assert!(saw(
        &app,
        r#""t":"dropped","what":"command","reason":"invalid""#
    ));
}

#[test]
fn authenticated_malformed_command_outside_a_transaction_stays_quiet() {
    let mut app = app();
    app.token = Some("session".into());
    let mut parser = crate::input::Parser::new();
    let inputs = parser.feed(b"\x1eVTABS session {not json}\n");
    assert_eq!(inputs.len(), 1);
    app.handle(inputs.into_iter().next().unwrap()).unwrap();
    assert!(app.out.is_empty());
}

#[test]
fn foreign_malformed_command_cannot_poison_an_authenticated_transaction() {
    let mut app = app();
    commit_sidebar(&mut app);
    app.token = Some("session".into());
    app.out.clear();
    send(&mut app, Command::Begin);

    let mut parser = crate::input::Parser::new();
    let inputs = parser.feed(b"\x1eVTABS stranger {not json}\n");
    assert_eq!(inputs.len(), 1);
    app.handle(inputs.into_iter().next().unwrap()).unwrap();

    assert!(app.sync.pending.as_ref().unwrap().valid);
    assert!(app.out.is_empty());
}

#[test]
fn intents_are_always_emitted_in_the_typed_envelope() {
    let mut app = app();
    app.emit(&Event::intent(Intent::NewTab)).unwrap();
    let sent = payloads(&app);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains(r#""t":"intent","a":"new_tab""#));
}

#[test]
fn begin_restarts_uncommitted_staging_from_committed_state() {
    let mut app = app();
    commit_sidebar(&mut app);
    app.out.clear();
    send(&mut app, Command::Begin);
    send(
        &mut app,
        model(r#"{"active":2,"focus":{"on":true,"index":2}}"#),
    );
    send(&mut app, Command::Begin);
    send(&mut app, menu(MENU_CLOSED));
    send(&mut app, Command::Commit);
    assert_eq!(app.metrics.commits, 2);
    assert_eq!(app.sync.model.as_ref().unwrap().active, Some(1));
}

#[test]
fn settings_are_raw_and_emit_typed_commits() {
    let mut app = app();
    app.size = (100, 21);
    commit_settings(&mut app);
    assert!(app.sync.model.is_none());
    assert!(app.sync.settings_document.is_some());
    assert!(painted(&app).contains("Settings"));
    app.out.clear();
    assert!(
        app.apply_document_intent(&Intent::NudgeOption {
            key: "width".into(),
            delta: 1,
        })
        .unwrap()
    );
    let commit = payloads(&app)
        .into_iter()
        .find(|payload| payload.contains(r#""t":"settings_commit""#))
        .unwrap();
    let commit: serde_json::Value = serde_json::from_str(&commit).unwrap();
    assert_eq!(commit["path"], serde_json::json!(["width"]));
}

#[test]
fn invalid_settings_invalidate_the_pending_transaction() {
    let mut app = app();
    send(&mut app, Command::Begin);
    send(&mut app, settings(r#"{"values":"not-an-object"}"#));
    assert!(!app.sync.pending.as_ref().unwrap().valid);
    assert!(saw(
        &app,
        r#""t":"dropped","what":"settings","reason":"invalid""#
    ));
}

#[test]
fn theme_hooks_are_a_transaction_barrier() {
    let mut app = app();
    let hooked = r##"{
        "private":false,"hook":true,
        "scheme":{"background":"#1e1e2e","foreground":"#cdd6f4"},
        "overrides":{"accent":"#89b4fa"}
    }"##;
    stage_sidebar(&mut app, hooked);
    send(&mut app, Command::Commit);
    assert_eq!(app.metrics.commits, 0);
    assert!(saw(&app, r#""t":"theme_hook_request""#));
    app.out.clear();
    send(
        &mut app,
        Command::ThemeHookResult {
            overrides: Box::new(payload::ThemeOverrides::default()),
        },
    );
    assert_eq!(app.metrics.commits, 1);
    assert_eq!(app.metrics.terminal_paints, 1);
}

#[test]
fn a_busy_hook_wait_quarantines_the_overlapping_batch_and_allows_republish() {
    let mut app = app();
    let hooked = r#"{"private":false,"hook":true,"scheme":{},"overrides":{}}"#;
    stage_sidebar(&mut app, hooked);
    send(&mut app, Command::Commit);
    assert_eq!(app.metrics.commits, 0);
    app.out.clear();

    send(&mut app, Command::Begin);
    assert!(saw(&app, r#""t":"dropped","what":"sync","reason":"busy""#));
    assert!(app.sync.discarding);
    send(
        &mut app,
        model(r#"{"active":2,"focus":{"on":true,"index":2}}"#),
    );
    send(&mut app, menu(MENU_CLOSED));
    send(&mut app, Command::Commit);
    assert!(!app.sync.discarding);
    assert_eq!(app.metrics.commits, 0);

    send(
        &mut app,
        Command::ThemeHookResult {
            overrides: Box::new(payload::ThemeOverrides::default()),
        },
    );
    assert_eq!(app.metrics.commits, 1);
    assert_eq!(app.sync.model.as_ref().unwrap().active, Some(1));

    send(&mut app, Command::Begin);
    send(
        &mut app,
        model(r#"{"active":2,"focus":{"on":true,"index":2}}"#),
    );
    send(&mut app, Command::Commit);
    send(
        &mut app,
        Command::ThemeHookResult {
            overrides: Box::new(payload::ThemeOverrides::default()),
        },
    );
    assert_eq!(app.metrics.commits, 2);
    assert_eq!(app.sync.model.as_ref().unwrap().active, Some(2));
}

#[test]
fn space_route_hooks_are_a_transaction_barrier() {
    let mut app = app();
    let hooked_spaces = SPACES.replace(r#""enabled":true,"#, r#""enabled":true,"hook":true,"#);
    send(&mut app, Command::Begin);
    send(&mut app, config());
    send(&mut app, theme(THEME));
    send(&mut app, spaces(&hooked_spaces));
    send(&mut app, model(MODEL));
    send(&mut app, menu(MENU_CLOSED));
    send(&mut app, Command::Commit);
    assert_eq!(app.metrics.commits, 0);
    assert!(saw(&app, r#""t":"space_route_hook_request""#));
    send(
        &mut app,
        Command::SpaceRouteHookResult {
            routes: vec![
                payload::SpaceRouteHookAnswer {
                    tab_id: 1,
                    space: None,
                },
                payload::SpaceRouteHookAnswer {
                    tab_id: 2,
                    space: None,
                },
            ],
        },
    );
    assert_eq!(app.metrics.commits, 1);
}

#[test]
fn lost_theme_hook_publishes_the_safe_fallback_without_replay() {
    let mut app = app();
    let hooked = r#"{"private":false,"hook":true,"scheme":{},"overrides":{}}"#;
    stage_sidebar(&mut app, hooked);
    send(&mut app, Command::Commit);
    let timeout = Instant::now();
    app.sync.pending.as_mut().unwrap().hook_deadline = Some(timeout);
    app.out.clear();
    app.tick_hooks(timeout).unwrap();
    assert!(saw(
        &app,
        r#""t":"dropped","what":"theme_hook","reason":"timeout""#
    ));
    assert_eq!(app.metrics.commits, 1);
    assert_eq!(
        payloads(&app)
            .iter()
            .filter(|event| event.contains(r#""t":"theme_hook_request""#))
            .count(),
        0
    );

    // Once an uncorrelated answer times out, no future request of that kind is issued in this
    // authenticated session. A late answer therefore cannot alter a later publication.
    app.out.clear();
    stage_sidebar(&mut app, hooked);
    send(&mut app, Command::Commit);
    assert_eq!(app.metrics.commits, 2);
    assert!(!saw(&app, r#""t":"theme_hook_request""#));
    send(
        &mut app,
        Command::ThemeHookResult {
            overrides: Box::new(payload::ThemeOverrides::default()),
        },
    );
    assert_eq!(app.metrics.commits, 2);
}

#[test]
fn menu_refusal_is_typed_and_deduped_per_publication() {
    let mut app = app();
    commit_sidebar(&mut app);
    app.out.clear();
    let refused = Outcome::Refused {
        why: "rows",
        level: Level::Root,
    };
    app.refuse(&refused).unwrap();
    app.refuse(&refused).unwrap();
    let events: Vec<_> = payloads(&app)
        .into_iter()
        .filter(|payload| payload.contains(r#""t":"menu_refused""#))
        .collect();
    assert_eq!(events.len(), 1);
}

#[test]
fn resize_burst_is_adopted_once_after_it_pauses() {
    let mut app = app();
    commit_sidebar(&mut app);
    app.out.clear();
    let start = Instant::now();
    probe_returns((30, 24));
    app.note_resize(start);
    probe_returns((31, 24));
    let second = start + Duration::from_millis(30);
    app.note_resize(second);
    assert_eq!(app.next_resize(), Some(second + RESIZE_DEBOUNCE));
    app.tick_resize(second + RESIZE_DEBOUNCE).unwrap();
    assert_eq!(app.size, (31, 24));
    assert_eq!(
        payloads(&app)
            .iter()
            .filter(|payload| payload.contains(r#""t":"resize""#))
            .count(),
        1
    );
    assert!(painted(&app).contains(CLEAR_SCREEN));
}

#[test]
fn new_authenticated_process_resets_committed_state() {
    let mut app = app();
    commit_sidebar(&mut app);
    send(
        &mut app,
        Command::Auth {
            token: "fresh".into(),
            keys: None,
        },
    );
    assert!(app.sync.config.is_none());
    assert!(app.sync.pending.is_none());
    assert!(saw(&app, r#""t":"ready""#));
}

const CONTROL: &str = "\x1eVTABS ";

fn transport_root(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let serial = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "vtabs-app-inbox-{tag}-{}-{serial}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn record(token: &str, json: &str) -> Vec<u8> {
    format!("{CONTROL}{token} {json}\n").into_bytes()
}

fn write_msg(dir: &Path, seq: u32, bytes: &[u8]) {
    std::fs::write(dir.join(format!("{seq:08}.msg")), bytes).unwrap();
}

fn session_dir(app: &App<Vec<u8>>) -> (String, PathBuf) {
    match &app.transport {
        Transport::Negotiating { inbox } | Transport::Active { inbox, .. } => {
            (inbox.session().to_owned(), inbox.dir().to_owned())
        }
        Transport::Off => panic!("no session"),
    }
}

#[test]
fn ready_offers_an_inbox_and_the_barrier_activates_it() {
    let root = transport_root("offer");
    let mut app = app();
    app.inbox = Some(Offer {
        root: root.clone(),
        wake: Arc::new(|_| true),
    });
    app.announce_ready().unwrap();
    let ready = payloads(&app)
        .into_iter()
        .find(|payload| payload.contains(r#""t":"ready""#))
        .unwrap();
    assert!(ready.contains(r#""transport":{"inbox":"inbox-"#));
    let (session, dir) = session_dir(&app);
    write_msg(
        &dir,
        1,
        &record(
            "t",
            &format!(r#"{{"t":"transport_probe","session":"{session}"}}"#),
        ),
    );
    app.token = Some("t".into());
    send(&mut app, Command::TransportBarrier { session });
    assert!(matches!(
        app.transport,
        Transport::Active { last_seq: 1, .. }
    ));
    assert!(saw(&app, r#""t":"transport_ready""#));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_transport_probe_falls_back_to_stdin() {
    let root = transport_root("missing");
    let mut app = app();
    app.token = Some("t".into());
    app.transport = Transport::Negotiating {
        inbox: Inbox::create(&root).unwrap(),
    };
    let (session, _) = session_dir(&app);
    send(&mut app, Command::TransportBarrier { session });
    assert!(matches!(app.transport, Transport::Off));
    assert!(saw(&app, r#""t":"transport_refused""#));
    let _ = std::fs::remove_dir_all(root);
}

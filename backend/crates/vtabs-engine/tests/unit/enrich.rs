use super::*;
use vtabs_protocol::v2::{ChromeFacts, PaneMetrics, StripButton, StripState};

fn space(icon: Option<&str>, name: &str) -> SpaceItem {
    SpaceItem {
        id: name.to_lowercase(),
        name: name.into(),
        icon: icon.map(str::to_string),
        unseen: false,
    }
}

#[test]
fn a_space_shows_its_icon_else_its_initial_else_a_dot() {
    assert_eq!(space_icon(&space(Some("󰋜"), "Home")), "󰋜");
    assert_eq!(space_icon(&space(None, "Home")), "H");
    assert_eq!(space_icon(&space(Some("  "), " pi")), "p");
    assert_eq!(
        space_icon(&space(Some("\u{7}"), "")),
        "·",
        "a control byte is no glyph"
    );
    assert_eq!(space_icon(&space(None, "")), "·");
}

#[test]
fn the_active_space_is_the_one_the_model_names() {
    let cfg = EngineConfig::try_from(
        serde_json::from_str::<vtabs_protocol::v2::ConfigMsg>(r#"{"rev":1}"#).unwrap(),
    )
    .unwrap();
    let raw_theme: ThemeMsg = serde_json::from_str(r#"{"rev":1}"#).unwrap();
    let model: vtabs_protocol::v2::ModelMsg = serde_json::from_str(
        r#"{"rev":1,"space":"pi","spaces":[{"id":"home","name":"Home"},{"id":"pi","icon":"@","unseen":true}]}"#,
    )
    .unwrap();
    let theme = theme_of(&raw_theme, model.private());
    let view = enrich(
        &cfg,
        &theme,
        model.sidebar().unwrap(),
        (28, 20),
        &UiState::default(),
    )
    .view;
    let seen: Vec<(&str, bool, bool, &str)> = view
        .spaces
        .iter()
        .map(|s| (s.id.as_str(), s.is_active, s.has_unseen, s.icon.as_str()))
        .collect();
    assert_eq!(
        seen,
        vec![("home", false, false, "H"), ("pi", true, true, "@")]
    );
}

fn tab(proc: Option<&str>, cwd: Option<&str>) -> TabRecord {
    TabRecord {
        id: 1,
        index: 1,
        title: String::new(),
        pane_title: None,
        override_title: None,
        proc: proc.map(str::to_string),
        icon: None,
        cwd: cwd.map(str::to_string),
        host: None,
        user: None,
        domain: None,
        pinned: false,
        unseen: false,
        settings: false,
    }
}

#[test]
fn a_shell_shows_where_it_is_and_a_tool_shows_what_it_runs() {
    let shell = tab(Some("zsh"), Some("~/src/vtabs"));
    assert_eq!(
        meta_from(&shell, MetaMode::Auto, " "),
        Some("~/src/vtabs".into())
    );
    let tool = tab(Some("nvim"), Some("~/src/vtabs"));
    assert_eq!(
        meta_from(&tool, MetaMode::Auto, " "),
        Some("nvim vtabs".into())
    );
    assert_eq!(
        meta_from(&tool, MetaMode::Auto, " · "),
        Some("nvim · vtabs".into()),
        "the separator is the user's"
    );
}

#[test]
fn a_lua_resolved_user_icon_wins_over_the_builtin_process_icon() {
    let cfg = EngineConfig::try_from(
        serde_json::from_str::<vtabs_protocol::v2::ConfigMsg>(
            r#"{"rev":1,"icons":true,"render":{"padding":{},"meta":true}}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let raw_theme: ThemeMsg = serde_json::from_str(r#"{"rev":1}"#).unwrap();
    let mut custom = tab(Some("nvim"), None);
    custom.icon = Some("CUSTOM".into());
    let mut model = vtabs_protocol::v2::SidebarModel::default();
    model.tabs.push(custom);
    let theme = theme_of(&raw_theme, model.private);
    let view = enrich(&cfg, &theme, &model, (28, 20), &UiState::default()).view;
    assert_eq!(view.items[0].icon, "CUSTOM");
}

#[test]
fn ssh_names_the_far_end() {
    let mut t = tab(Some("ssh"), Some("~"));
    assert_eq!(meta_from(&t, MetaMode::Auto, " "), Some("ssh".into()));
    t.host = Some("box".into());
    assert_eq!(meta_from(&t, MetaMode::Auto, " "), Some("box".into()));
    t.user = Some("root".into());
    assert_eq!(meta_from(&t, MetaMode::Auto, " "), Some("root@box".into()));
}

#[test]
fn a_pane_with_no_process_names_its_domain_instead() {
    let mut t = tab(None, Some("~/x"));
    t.domain = Some("local".into());
    assert_eq!(meta_from(&t, MetaMode::Auto, " "), Some("~/x".into()));
    t.domain = Some("ssh:box".into());
    assert_eq!(
        meta_from(&t, MetaMode::Auto, " "),
        Some("ssh:box ~/x".into())
    );
}

#[test]
fn the_mode_overrides_the_composition_and_off_means_off() {
    let t = tab(Some("nvim"), Some("~/src/vtabs"));
    assert_eq!(
        meta_from(&t, MetaMode::Cwd, " "),
        Some("~/src/vtabs".into())
    );
    assert_eq!(meta_from(&t, MetaMode::Process, " "), Some("nvim".into()));
    assert_eq!(meta_from(&t, MetaMode::Off, " "), None, "meta = false");
}

#[test]
fn the_title_falls_back_through_every_source() {
    let mut t = tab(None, None);
    assert_eq!(title_of(&t), "tab 1");
    t.pane_title = Some("pane".into());
    assert_eq!(title_of(&t), "pane");
    t.title = "tab".into();
    assert_eq!(title_of(&t), "tab");
    t.override_title = Some("mine".into());
    assert_eq!(title_of(&t), "mine", "the hook wins");
}

#[test]
fn current_menu_headers_share_the_card_title_and_meta_policy() {
    let cfg = EngineConfig::try_from(
        serde_json::from_str::<vtabs_protocol::v2::ConfigMsg>(
            r#"{"rev":1,"meta":"auto","meta_sep":" · "}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let mut subject = tab(Some("nvim"), Some("~/src/vtabs"));
    subject.id = 7;
    subject.override_title = Some("Editor".into());
    let mut model = SidebarModel::default();
    model.tabs.push(subject);

    let root: MenuMsg =
        serde_json::from_str(r#"{"rev":1,"open":true,"target":7,"subject":7,"items":[]}"#).unwrap();
    assert_eq!(
        menu_header(&root, &model, &cfg),
        Some(MenuHeader {
            title: "Editor".into(),
            meta: Some("nvim · vtabs".into()),
        })
    );

    let confirm: MenuMsg = serde_json::from_str(
        r#"{"rev":2,"open":true,"level":"confirm","subject":7,"victims":3,"items":[]}"#,
    )
    .unwrap();
    assert_eq!(
        menu_header(&confirm, &model, &cfg),
        Some(MenuHeader {
            title: "Close Editor?".into(),
            meta: Some("and 2 others".into()),
        })
    );
}

#[test]
fn unknown_theme_keys_are_ignored_not_fatal() {
    let msg: ThemeMsg = serde_json::from_str(
        r##"{"rev":1,"overrides":{"accent":"#ff0000","nonsense":"#00ff00","scrim":0.25}}"##,
    )
    .unwrap();
    let t = msg.overrides;
    assert_eq!(t.accent.as_deref(), Some("#ff0000"));
    assert_eq!(t.scrim, Some(0.25));
}

fn geometry_cfg() -> EngineConfig {
    EngineConfig::try_from(
        serde_json::from_str::<vtabs_protocol::v2::ConfigMsg>(
            r#"{"rev":1,"rail_width":5,"position":"left","render":{"padding":{"left":1,"right":1,"top":1,"bottom":1}}}"#,
        )
        .unwrap(),
    )
    .unwrap()
}

fn raw_geometry_model(chrome: ChromeFacts, rail: bool, toggle: bool) -> SidebarModel {
    SidebarModel {
        rail,
        strip: Some(StripState {
            // 8.4 point cells across and 19 down: 70/8.4 -> 9 cols, 28/19 -> 2 rows.
            metrics: Some(PaneMetrics {
                cols: 28,
                viewport_rows: 30,
                pixel_width: 235.0,
                pixel_height: 570.0,
                dpi: None,
            }),
            chrome: Some(chrome),
            buttons: toggle
                .then(|| StripButton {
                    id: "toggle".into(),
                    icon: None,
                })
                .into_iter()
                .collect(),
        }),
        ..Default::default()
    }
}

#[test]
fn raw_pane_and_chrome_facts_are_the_only_strip_geometry_input() {
    let cfg = geometry_cfg();
    let mac = ChromeFacts {
        is_mac: true,
        integrated_buttons: true,
        native_button_style: true,
        ..Default::default()
    };

    let (strip, reserve) = strip_of(&cfg, &raw_geometry_model(mac, false, true), 28, 30);
    assert_eq!((strip.rows, strip.cols, strip.toggle_row), (3, 9, Some(1)));
    assert_eq!(reserve, Some(9));

    let preview = ChromeFacts {
        preview: true,
        integrated_buttons: true,
        native_button_style: true,
        ..Default::default()
    };
    let (strip, reserve) = strip_of(&cfg, &raw_geometry_model(preview, false, true), 28, 30);
    assert_eq!((strip.rows, strip.cols), (3, 9));
    assert_eq!(reserve, Some(9), "preview uses non-Mac logical DPI");

    let fullscreen = ChromeFacts {
        is_full_screen: true,
        ..mac
    };
    let (strip, reserve) = strip_of(&cfg, &raw_geometry_model(fullscreen, false, true), 28, 30);
    assert_eq!((strip.rows, strip.cols, strip.toggle_row), (2, 0, Some(2)));
    assert_eq!(reserve, Some(0), "fullscreen clears Lua's prior reserve");

    let (strip, reserve) = strip_of(&cfg, &raw_geometry_model(mac, true, true), 28, 30);
    assert_eq!((strip.rows, strip.cols, strip.toggle_row), (4, 9, Some(3)));
    assert_eq!(reserve, Some(9), "rail toggle sits below the lights");

    let (strip, reserve) = strip_of(&cfg, &raw_geometry_model(mac, false, false), 28, 30);
    assert_eq!((strip.rows, strip.cols), (3, 9));
    assert_eq!(reserve, Some(9), "no toggle still reserves native chrome");

    let mut no_metrics = raw_geometry_model(fullscreen, false, true);
    no_metrics.strip.as_mut().unwrap().metrics = None;
    let (_, reserve) = strip_of(&cfg, &no_metrics, 28, 30);
    assert_eq!(
        reserve,
        Some(0),
        "fullscreen conclusively clears a reserve without pane metrics"
    );

    let mut enabled_without_metrics = raw_geometry_model(mac, false, true);
    enabled_without_metrics.strip.as_mut().unwrap().metrics = None;
    let (_, reserve) = strip_of(&cfg, &enabled_without_metrics, 28, 30);
    assert_eq!(reserve, None, "a positive reserve requires current metrics");

    let right = EngineConfig::try_from(
        serde_json::from_str::<vtabs_protocol::v2::ConfigMsg>(
            r#"{"rev":1,"position":"right","render":{"padding":{"left":1,"right":1,"top":1,"bottom":1}}}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let (_, reserve) = strip_of(&right, &enabled_without_metrics, 28, 30);
    assert_eq!(
        reserve,
        Some(0),
        "right position conclusively has no reserve"
    );
}

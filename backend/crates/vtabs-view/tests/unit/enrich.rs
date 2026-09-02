use super::*;

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
    let cfg: ConfigMsg = serde_json::from_str(r#"{"rev":1}"#).unwrap();
    let theme: ThemeMsg = serde_json::from_str(r#"{"rev":1}"#).unwrap();
    let model: ModelMsg = serde_json::from_str(
        r#"{"rev":1,"space":"pi","spaces":[{"id":"home","name":"Home"},{"id":"pi","icon":"@","unseen":true}]}"#,
    )
    .unwrap();
    let view = enrich(&cfg, &theme, &model, (28, 20), &UiState::default()).view;
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
        meta_from(&shell, Some("auto"), " "),
        Some("~/src/vtabs".into())
    );
    let tool = tab(Some("nvim"), Some("~/src/vtabs"));
    assert_eq!(
        meta_from(&tool, Some("auto"), " "),
        Some("nvim vtabs".into())
    );
    assert_eq!(
        meta_from(&tool, Some("auto"), " · "),
        Some("nvim · vtabs".into()),
        "the separator is the user's"
    );
}

#[test]
fn ssh_names_the_far_end() {
    let mut t = tab(Some("ssh"), Some("~"));
    assert_eq!(meta_from(&t, Some("auto"), " "), Some("ssh".into()));
    t.host = Some("box".into());
    assert_eq!(meta_from(&t, Some("auto"), " "), Some("box".into()));
    t.user = Some("root".into());
    assert_eq!(meta_from(&t, Some("auto"), " "), Some("root@box".into()));
}

#[test]
fn a_pane_with_no_process_names_its_domain_instead() {
    let mut t = tab(None, Some("~/x"));
    t.domain = Some("local".into());
    assert_eq!(meta_from(&t, Some("auto"), " "), Some("~/x".into()));
    t.domain = Some("ssh:box".into());
    assert_eq!(meta_from(&t, Some("auto"), " "), Some("ssh:box ~/x".into()));
}

#[test]
fn the_mode_overrides_the_composition_and_off_means_off() {
    let t = tab(Some("nvim"), Some("~/src/vtabs"));
    assert_eq!(meta_from(&t, Some("cwd"), " "), Some("~/src/vtabs".into()));
    assert_eq!(meta_from(&t, Some("process"), " "), Some("nvim".into()));
    assert_eq!(meta_from(&t, None, " "), None, "meta = false");
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
fn unknown_theme_keys_are_ignored_not_fatal() {
    let msg: ThemeMsg = serde_json::from_str(
        r##"{"rev":1,"overrides":{"accent":"#ff0000","nonsense":"#00ff00","scrim":0.25}}"##,
    )
    .unwrap();
    let t = user_theme(&msg);
    assert_eq!(t.accent.as_deref(), Some("#ff0000"));
    assert_eq!(t.scrim, Some(0.25));
}

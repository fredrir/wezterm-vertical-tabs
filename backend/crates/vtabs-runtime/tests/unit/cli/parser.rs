use super::*;

#[test]
fn kill_reaches_only_a_backend_listed_on_this_server() {
    let pane = |pane_id, title: &str| PaneInfo {
        pane_id,
        tab_id: 7,
        title: title.into(),
        left_col: 0,
        cols: 30,
        is_active: false,
    };
    let panes = vec![
        pane(3, "wez-vtabs:beef"),
        pane(4, "zsh"),
        pane(5, "wez-vtabs-settings:cafe"),
    ];
    assert_eq!(kill_target(&panes, 3), Ok(()));
    assert_eq!(kill_target(&panes, 5), Ok(()));
    assert_eq!(
        kill_target(&panes, 4),
        Err("pane 4 is not a backend".into())
    );
    assert_eq!(
        kill_target(&panes, 9),
        Err("pane 9 not on this server".into())
    );
}

#[test]
fn standalone_socket_prefers_the_mux_protocol() {
    assert!(is_mux_socket(Some(OsStr::new("/tmp/e2e/mux.sock"))));
    assert!(is_mux_socket(Some(OsStr::new(
        "/run/user/1000/wezterm-mux"
    ))));
}

#[test]
fn gui_socket_keeps_the_gui_protocol() {
    assert!(!is_mux_socket(Some(OsStr::new(
        "/run/user/1000/wezterm/gui-sock-42"
    ))));
    assert!(!is_mux_socket(None));
}

#[test]
fn negative_pane_and_tab_ids_are_skipped_before_conversion() {
    let json = r#"[
        {"pane_id": 3, "tab_id": 7, "title": "valid", "left_col": 0, "size": {"cols": 80}},
        {"pane_id": -1, "tab_id": 7, "title": "pane", "left_col": 0, "size": {"cols": 80}},
        {"pane_id": 4, "tab_id": -1, "title": "tab", "left_col": 0, "size": {"cols": 80}}
    ]"#;
    assert_eq!(
        panes_from_json(json).unwrap(),
        vec![PaneInfo {
            pane_id: 3,
            tab_id: 7,
            title: "valid".into(),
            left_col: 0,
            cols: 80,
            is_active: false,
        }]
    );
}

#[test]
fn invalid_geometry_is_skipped() {
    let json = format!(
        r#"[
            {{"pane_id": 1, "tab_id": 7, "title": "negative position", "left_col": -1, "size": {{"cols": 80}}}},
            {{"pane_id": 2, "tab_id": 7, "title": "negative size", "left_col": 0, "size": {{"cols": -1}}}},
            {{"pane_id": 3, "tab_id": 7, "title": "zero size", "left_col": 0, "size": {{"cols": 0}}}},
            {{"pane_id": 4, "tab_id": 7, "title": "overflow", "left_col": {}, "size": {{"cols": 1}}}},
            {{"pane_id": 5, "tab_id": 7, "title": "wrong type", "left_col": "0", "size": {{"cols": 80}}}}
        ]"#,
        i64::MAX
    );
    assert!(panes_from_json(&json).unwrap().is_empty());
}

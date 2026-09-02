use vtabs_runtime::cli::{PaneInfo, is_marker, kill_target, panes_from_json, rescue_plan};

fn pane(id: u64, tab: u64, title: &str, left: i64, cols: i64) -> PaneInfo {
    PaneInfo {
        pane_id: id,
        tab_id: tab,
        title: title.into(),
        left_col: left,
        cols,
    }
}

#[test]
fn only_a_backend_title_names_a_kill_and_only_one_pane_may_carry_it() {
    let panes = vec![
        pane(1, 1, "wez-vtabs:abcd", 0, 28),
        pane(2, 1, "zsh", 29, 100),
        pane(3, 2, "wez-vtabs:ffff", 0, 28),
        pane(4, 2, "wez-vtabs:ffff", 29, 28),
    ];
    assert_eq!(kill_target(&panes, "wez-vtabs:abcd"), Ok(1));
    assert!(
        kill_target(&panes, "zsh").is_err(),
        "a shell is never a target"
    );
    assert!(kill_target(&panes, "wez-vtabs:0000").is_err(), "unknown");
    assert!(kill_target(&panes, "wez-vtabs:ffff").is_err(), "ambiguous");
    assert!(is_marker("wez-vtabs-settings:1a2b"));
    assert!(!is_marker("wez-vtabs:") && !is_marker("wez-vtabs:zz"));
}

#[test]
fn the_plan_moves_the_pane_inside_the_band_under_the_content_beside_it() {
    // a SplitHorizontal on the sidebar: the shell landed at column 15, inside the 28 the
    // sidebar is meant to have, and the sidebar's own box shrank to 14
    let panes = vec![
        pane(1, 7, "wez-vtabs:abcd", 0, 14),
        pane(2, 7, "zsh", 15, 13),
        pane(3, 7, "nvim", 29, 100),
        pane(9, 8, "zsh", 0, 129),
    ];
    assert_eq!(rescue_plan(&panes, 1, 28, false), Ok((vec![2], 3)));
    assert!(
        rescue_plan(&panes, 9, 28, false).is_err(),
        "another tab has nothing inside its band"
    );
    let right = vec![
        pane(1, 7, "nvim", 0, 100),
        pane(2, 7, "zsh", 101, 13),
        pane(3, 7, "wez-vtabs:abcd", 115, 14),
    ];
    assert_eq!(rescue_plan(&right, 3, 28, true), Ok((vec![2], 1)));
}

#[test]
fn the_list_is_read_from_the_cli_json_shape() {
    let json = r#"[{"window_id":0,"tab_id":7,"pane_id":3,"title":"zsh","left_col":29,"top_row":0,
        "size":{"rows":60,"cols":100,"pixel_width":1,"pixel_height":1,"dpi":144},"is_active":true},
        {"tab_id":7,"pane_id":"x"}]"#;
    assert_eq!(
        panes_from_json(json).unwrap(),
        vec![pane(3, 7, "zsh", 29, 100)],
        "a malformed entry is skipped, not fatal"
    );
    assert!(panes_from_json("nope").is_err());
}

#[test]
fn a_tab_with_no_content_outside_the_band_has_nowhere_to_move_to() {
    let panes = vec![
        pane(1, 7, "wez-vtabs:abcd", 0, 14),
        pane(2, 7, "zsh", 15, 13),
    ];
    assert!(rescue_plan(&panes, 1, 28, false).is_err());
}

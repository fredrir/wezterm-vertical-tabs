use super::{core::*, *};
use serde_json::json;

fn tab(id: u64) -> Tab {
    Tab {
        id,
        title: format!("Tab {id}"),
        ..Tab::default()
    }
}
fn configured(value: serde_json::Value) -> WindowApp {
    let mut app = WindowApp::default();
    app.config(value).unwrap();
    app
}
fn set(app: &mut WindowApp, key: &str, value: serde_json::Value) {
    app.dispatch(Intent::SetSetting {
        key: key.into(),
        value,
    })
    .unwrap();
}

#[test]
fn settings_edits_write_only_the_managed_layer_once_per_change() {
    let mut app = configured(json!({"settings": {"cards": false}, "managed": {}}));
    assert!(app.take_managed_write().is_none());
    set(&mut app, "width", json!(300));
    let source = app.take_managed_write().unwrap();
    assert!(source.contains("---@type TabsManaged"));
    assert!(source.contains("width = 300"));
    assert!(!source.contains("cards"));
    set(&mut app, "width", json!(310));
    assert!(app.take_managed_write().is_none());
    app.complete_managed_write(Ok(()));
    assert!(app.take_managed_write().unwrap().contains("width = 310"));
}

#[test]
fn a_reload_of_older_file_content_keeps_edits_not_yet_written() {
    let mut app = configured(json!({"managed": {"settings": {"width": 280}}}));
    set(&mut app, "width", json!(350));
    let written = app.take_managed_write().unwrap();
    app.config(json!({"managed": {"settings": {"width": 280}}}))
        .unwrap();
    assert_eq!(app.model().settings.width, 350);

    app.complete_managed_write(Ok(()));
    app.config(json!({"managed": {"settings": {"width": 350}}}))
        .unwrap();
    assert!(app.take_managed_write().is_none());
    assert!(written.contains("width = 350"));
}

#[test]
fn hand_edits_to_the_file_replace_the_managed_layer_but_config_values_win() {
    let mut app = configured(json!({"managed": {"settings": {"width": 280}}}));
    app.config(json!({
        "settings": {"side": "right"},
        "managed": {"settings": {"side": "left", "show_metadata": true}},
    }))
    .unwrap();
    assert_eq!(app.model().settings.width, Settings::default().width);
    assert_eq!(app.model().settings.side, Side::Right);
    assert!(app.model().settings.show_metadata);
    assert!(app.model().config_owned.contains("side"));
    assert!(app.take_managed_write().is_none());
}

#[test]
fn failed_writes_are_reported_and_the_next_edit_retries() {
    let mut app = configured(json!({"managed": {}}));
    set(&mut app, "width", json!(300));
    assert!(app.take_managed_write().is_some());
    app.complete_managed_write(Err("read-only file system".into()));
    assert_eq!(
        app.take_errors(),
        ["Settings file: read-only file system".to_owned()]
    );
    set(&mut app, "cards", json!(false));
    assert!(app.take_managed_write().unwrap().contains("width = 300"));
}

#[test]
fn without_a_settings_file_edits_stay_in_memory() {
    let mut app = configured(json!({}));
    set(&mut app, "width", json!(300));
    assert_eq!(app.model().settings.width, 300);
    assert!(app.take_managed_write().is_none());
}

#[test]
fn rail_toggles_are_session_state_that_survives_reloads() {
    let mut app = configured(json!({"settings": {"rail": "expanded"}, "managed": {}}));
    app.dispatch(Intent::SetRail(RailMode::Collapsed)).unwrap();
    assert!(app.take_managed_write().is_none());
    app.config(json!({"settings": {"rail": "expanded"}, "managed": {}}))
        .unwrap();
    assert_eq!(app.model().settings.rail, RailMode::Collapsed);
}

#[test]
fn ui_spaces_are_written_after_declared_ones_without_runtime_state() {
    let mut app = configured(json!({
        "spaces": [{"id": "work", "name": "Work"}],
        "managed": {"spaces": [{"id": "work", "name": "Shadowed"}, {"id": "notes", "name": "Notes"}]},
    }));
    let ids = |app: &WindowApp| {
        app.model()
            .spaces
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&app), ["work", "notes"]);
    assert_eq!(app.model().spaces[0].name, "Work");
    app.dispatch(Intent::ToggleSpace("notes".into())).unwrap();
    assert!(app.take_managed_write().is_none());
    app.dispatch(Intent::RenameSpace {
        id: "notes".into(),
        name: "Journal".into(),
    })
    .unwrap();
    let source = app.take_managed_write().unwrap();
    assert!(source.contains(r#"name = "Journal""#));
    assert!(!source.contains("collapsed"));
    assert!(!source.contains(r#""Work""#));
    app.complete_managed_write(Ok(()));
    app.config(json!({
        "spaces": [{"id": "work", "name": "Work"}],
        "managed": {"spaces": [{"id": "notes", "name": "Journal"}]},
    }))
    .unwrap();
    assert!(app.model().spaces[1].collapsed);
}

#[test]
fn removing_a_space_from_the_file_reroutes_its_tabs() {
    let managed =
        json!({"spaces": [{"id": "home", "name": "Home"}, {"id": "notes", "name": "Notes"}]});
    let mut app = configured(json!({"managed": managed}));
    app.update(HostSnapshot {
        revision: 1,
        tabs: vec![tab(1)],
        active_tab: Some(1),
        metrics: Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
    app.dispatch(Intent::AssignTab {
        id: 1,
        space_id: "notes".into(),
    })
    .unwrap();
    app.complete_managed_write(Ok(()));
    app.config(json!({"managed": {"spaces": [{"id": "home", "name": "Home"}]}}))
        .unwrap();
    assert_eq!(app.model().spaces.len(), 1);
    assert_eq!(app.model().tabs[&1].space_id, "home");
}

#[test]
fn invalid_file_content_is_rejected_before_anything_is_published() {
    let mut app = configured(json!({"managed": {"settings": {"width": 300}}}));
    assert!(
        app.config(json!({"managed": {"settings": {"width": 300, "side": "top"}}}))
            .is_err()
    );
    assert!(
        app.config(json!({"managed": {"settings": {}, "unknown": true}}))
            .is_err()
    );
    assert_eq!(app.model().settings.width, 300);
}

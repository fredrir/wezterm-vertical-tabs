use super::{
    core::HostTab,
    store::{self, Operation, Record, Request, Response},
    *,
};
use serde_json::json;
use std::{collections::BTreeMap, time::Duration};

fn tab(id: u64) -> HostTab {
    HostTab {
        id,
        title: format!("Tab {id}"),
        ..HostTab::default()
    }
}
fn snapshot(revision: u64, active: Option<u64>, tabs: Vec<HostTab>) -> HostSnapshot {
    HostSnapshot {
        revision,
        active_tab: active,
        tabs,
        metrics: Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    }
}
fn app() -> WindowApp {
    let mut app = WindowApp::default();
    app.update(snapshot(1, Some(1), vec![tab(1), tab(2)]))
        .unwrap();
    app
}
fn success(request: &Request, records: Vec<Record>) -> Response {
    Response {
        version: store::PROTOCOL_VERSION,
        request_id: request.request_id,
        revision: 1,
        records,
        error: None,
    }
}
fn drain_initial(app: &mut WindowApp) {
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.complete_storage(success(&request, vec![])).unwrap();
}

#[test]
fn unchanged_paint_does_not_render_or_schedule_idle_frames() {
    let mut app = app();
    app.apply_config(BTreeMap::from([("animations".into(), json!(false))]), 0)
        .unwrap();
    assert!(app.render(Duration::ZERO).is_some());
    assert!(app.render(Duration::from_millis(10)).is_none());
    drain_initial(&mut app);
    assert_eq!(app.next_deadline(), None);
}
#[test]
fn resize_and_activation_do_not_write_storage() {
    let mut app = app();
    drain_initial(&mut app);
    for i in [1, 2] {
        app.resize(Metrics {
            cols: 20 + i,
            rows: 30 + i,
            ..Metrics::default()
        })
        .unwrap();
        app.dispatch(Intent::ActivateTab(if i % 2 == 0 { 1 } else { 2 }))
            .unwrap();
        app.render(Duration::from_millis(u64::from(i) * 10));
        assert!(app.take_storage_request(Duration::from_secs(5)).is_none());
    }
}
#[test]
fn stale_snapshot_and_hook_cannot_restore_old_view() {
    let mut app = app();
    let token = app.hook_token(Some(1));
    app.update(snapshot(2, Some(2), vec![tab(1), tab(2)]))
        .unwrap();
    assert!(
        !app.complete_hook_batch(vec![(token, HookResult::Title("Old title".into()))])
            .unwrap()
    );
    assert!(app.update(snapshot(1, Some(1), vec![tab(1)])).is_err());
    assert_eq!(app.model().selected_tab, Some(2));
}
#[test]
fn pixel_only_resize_rejects_stale_hooks_without_cell_recomposition() {
    let mut app = app();
    app.apply_config(BTreeMap::from([("animations".into(), json!(false))]), 0)
        .unwrap();
    app.render(Duration::ZERO);
    let token = app.hook_token(Some(1));
    let metrics = app.metrics();
    app.resize(Metrics {
        pixel_width: metrics.pixel_width + 1,
        ..metrics
    })
    .unwrap();
    assert!(
        !app.complete_hook_batch(vec![(token, HookResult::Title("Stale geometry".into()))])
            .unwrap()
    );
    assert!(app.render(Duration::from_millis(1)).is_none());
}
#[test]
fn hook_batch_commits_all_tabs_and_rolls_back_invalid_result() {
    let mut app = app();
    let a = app.hook_token(Some(1));
    let b = app.hook_token(Some(2));
    assert!(
        app.complete_hook_batch(vec![
            (a, HookResult::Title("One".into())),
            (b, HookResult::Title("Two".into()))
        ])
        .unwrap()
    );
    assert_eq!(app.model().tabs[&1].display_title(), "One");
    assert_eq!(app.model().tabs[&2].display_title(), "Two");
    let a = app.hook_token(Some(1));
    let b = app.hook_token(Some(999));
    assert!(
        app.complete_hook_batch(vec![
            (a, HookResult::Title("Must roll back".into())),
            (b, HookResult::Title("Missing".into()))
        ])
        .is_err()
    );
    assert_eq!(app.model().tabs[&1].display_title(), "One");
}
#[test]
fn reserved_spawn_does_not_steal_newer_space_selection() {
    let mut app = app();
    let token = app.reserve_spawn();
    app.dispatch(Intent::CreateSpace {
        name: "Later selection".into(),
    })
    .unwrap();
    let selected = app.model().selected_space.clone();
    app.update(snapshot(2, Some(3), vec![tab(1), tab(2), tab(3)]))
        .unwrap();
    app.spawn_completed_with_token(3, token).unwrap();
    assert_eq!(app.model().selected_space, selected);
    assert_eq!(app.model().selected_tab, None);
    assert_eq!(app.model().tabs[&3].space_id, "home");
}
#[test]
fn reserved_spawn_activates_captured_space_when_interaction_unchanged() {
    let mut app = app();
    app.dispatch(Intent::CreateSpace {
        name: "Work".into(),
    })
    .unwrap();
    let token = app.reserve_spawn();
    let space = token.space_id.clone();
    app.update(snapshot(2, Some(3), vec![tab(1), tab(2), tab(3)]))
        .unwrap();
    let result = app.spawn_completed_with_token(3, token).unwrap();
    assert_eq!(app.model().selected_space, space);
    assert_eq!(app.model().selected_tab, Some(3));
    assert_eq!(app.projection().active, Some(3));
    assert!(app.projection().visible.contains(&3));
    // The snapshot already reports tab 3 active, so no other tab is activated.
    assert!(result.commands.iter().all(|command| {
        !matches!(command, Command::Host(HostCommand::Activate(id)) if *id != 3)
    }));
}
#[test]
fn storage_requests_coalesce_and_old_completion_cannot_clear_newer_write() {
    let mut app = app();
    drain_initial(&mut app);
    app.dispatch(Intent::CreateFolder {
        name: "First".into(),
    })
    .unwrap();
    let id = app.model().folders[0].id.clone();
    let rename = |app: &mut WindowApp, name: &str| {
        app.dispatch(Intent::RenameFolder {
            id: id.clone(),
            name: name.into(),
        })
        .unwrap();
    };
    rename(&mut app, "Second");
    assert!(
        app.take_storage_request(Duration::from_millis(50))
            .is_none()
    );
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    rename(&mut app, "Third");
    let records = write
        .operations
        .iter()
        .filter_map(|o| {
            if let Operation::Put { key, value, .. } = o {
                Some(Record {
                    key: key.clone(),
                    value: Some(value.clone()),
                    revision: 1,
                })
            } else {
                None
            }
        })
        .collect();
    app.complete_storage(success(&write, records)).unwrap();
    let next = app.take_storage_request(Duration::from_secs(2)).unwrap();
    assert!(next.operations.iter().any(
        |o| matches!(o,Operation::Put{key,value,..}if key.field=="name"&&value==&json!("Third"))
    ));
}
#[test]
fn close_restores_visible_successor_as_explicit_id_command() {
    let mut app = app();
    app.dispatch(Intent::CreateSpace {
        name: "Other".into(),
    })
    .unwrap();
    let other = app.model().selected_space.clone();
    app.dispatch(Intent::AssignTab {
        id: 2,
        space_id: other,
    })
    .unwrap();
    app.dispatch(Intent::SelectSpace("home".into())).unwrap();
    app.update(snapshot(2, Some(2), vec![tab(2)])).unwrap();
    assert_eq!(app.model().selected_space, "home");
    assert_eq!(app.projection().active, None);
}

#[test]
fn close_requests_ask_the_host_only_while_confirmation_is_enabled() {
    let mut app = app();
    app.render(Duration::ZERO);
    let row = app
        .ui()
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ui::ElementId::Tab(1))
        .unwrap()
        .rect;
    // The close control exists only while its row is hovered.
    app.input(ui::UiInput::PointerMove {
        x: row.x + 1,
        y: row.y,
    })
    .unwrap();
    app.render(Duration::ZERO);
    let close = |app: &WindowApp| {
        app.ui()
            .hit_regions()
            .iter()
            .find(|hit| hit.id == ui::ElementId::CloseTab(1))
            .unwrap()
            .rect
    };
    let rect = close(&app);
    let press = ui::UiInput::PointerDown {
        x: rect.x,
        y: rect.y,
        button: ui::MouseButton::Left,
        modifiers: ui::Modifiers::default(),
    };
    let release = ui::UiInput::PointerUp {
        x: rect.x,
        y: rect.y,
        button: ui::MouseButton::Left,
    };
    app.input(press.clone()).unwrap();
    assert_eq!(
        app.input(release.clone()).unwrap().commands,
        vec![Command::ConfirmClose(1)]
    );
    assert!(app.model().tabs.contains_key(&1));

    app.apply_config(BTreeMap::from([("confirm_close".into(), json!(false))]), 0)
        .unwrap();
    app.render(Duration::from_millis(1));
    let rect = close(&app);
    app.input(ui::UiInput::PointerDown {
        x: rect.x,
        y: rect.y,
        button: ui::MouseButton::Left,
        modifiers: ui::Modifiers::default(),
    })
    .unwrap();
    assert_eq!(
        app.input(ui::UiInput::PointerUp {
            x: rect.x,
            y: rect.y,
            button: ui::MouseButton::Left,
        })
        .unwrap()
        .commands,
        vec![Command::Host(HostCommand::Close(1))]
    );
}
#[test]
fn any_tab_activation_leaves_the_settings_page_and_releases_its_search() {
    let mut app = app();
    app.open_settings();
    app.input(UiInput::key(ui::Key::Character('w'))).unwrap();
    assert!(app.ui().text_input_active());

    app.update(snapshot(2, Some(3), vec![tab(1), tab(2), tab(3)]))
        .unwrap();
    assert!(!app.ui().content_page());
    assert_eq!(app.ui().focused(), None);
    assert!(!app.is_modal());

    app.open_settings();
    app.update(snapshot(3, Some(2), vec![tab(1), tab(2)]))
        .unwrap();
    assert!(
        app.ui().content_page(),
        "closing the tab beneath the page is not a switch away"
    );
    app.dispatch(Intent::ActivateTab(1)).unwrap();
    assert!(!app.ui().content_page());
}

#[test]
fn settings_pages_open_and_close_independently_in_each_space() {
    let mut app = app();
    app.open_settings();
    app.dispatch(Intent::CreateSpace {
        name: "Other".into(),
    })
    .unwrap();
    let other = app.model().selected_space.clone();
    assert!(!app.ui().content_page());
    assert!(!app.is_modal());

    app.dispatch(Intent::AssignTab {
        id: 2,
        space_id: other.clone(),
    })
    .unwrap();
    app.activate_index(-1).unwrap();
    assert!(!app.ui().content_page());
    app.open_settings();

    app.dispatch(Intent::SelectSpace("home".into())).unwrap();
    app.update(snapshot(2, Some(1), vec![tab(1), tab(2)]))
        .unwrap();
    assert!(app.ui().content_page());
    app.ui_mut().close_settings();

    app.dispatch(Intent::SelectSpace(other.clone())).unwrap();
    app.update(snapshot(3, Some(2), vec![tab(1), tab(2)]))
        .unwrap();
    assert!(app.ui().content_page());
    app.activate_index(0).unwrap();
    assert!(!app.ui().content_page());
    app.activate_index(-1).unwrap();
    assert!(app.ui().content_page());

    app.dispatch(Intent::SelectSpace("home".into())).unwrap();
    app.activate_index(-1).unwrap();
    assert!(!app.ui().content_page());
    app.dispatch(Intent::SelectSpace(other)).unwrap();
    assert!(app.ui().content_page());
}

#[test]
fn activating_another_spaces_tab_preserves_the_page_in_the_space_left_behind() {
    for host_activation in [false, true] {
        let mut app = app();
        app.dispatch(Intent::CreateSpace {
            name: "Other".into(),
        })
        .unwrap();
        let other = app.model().selected_space.clone();
        app.dispatch(Intent::AssignTab {
            id: 2,
            space_id: other.clone(),
        })
        .unwrap();
        app.open_settings();
        app.dispatch(Intent::SelectSpace("home".into())).unwrap();
        app.open_settings();

        if host_activation {
            app.update(snapshot(2, Some(2), vec![tab(1), tab(2)]))
                .unwrap();
        } else {
            app.dispatch(Intent::ActivateTab(2)).unwrap();
        }
        assert_eq!(app.model().selected_space, other);
        assert!(!app.ui().content_page());
        app.dispatch(Intent::SelectSpace("home".into())).unwrap();
        assert!(app.ui().content_page());
        app.dispatch(Intent::SelectSpace(other)).unwrap();
        assert!(!app.ui().content_page());
        app.activate_index(-1).unwrap();
        assert!(app.ui().content_page());
    }
}

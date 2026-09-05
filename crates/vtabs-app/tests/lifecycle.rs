use serde_json::json;
use std::{collections::BTreeMap, time::Duration};
use vtabs_app::{
    core::*,
    store::{self, Operation, Record, Request, Response},
    *,
};

fn tab(id: u64) -> Tab {
    Tab {
        id,
        title: format!("Tab {id}"),
        ..Tab::default()
    }
}
fn snapshot(revision: u64, active: Option<u64>, tabs: Vec<Tab>) -> NativeSnapshot {
    NativeSnapshot {
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
fn unchanged_native_paint_reuses_buffer_and_schedules_no_idle_frames() {
    let mut app = app();
    app.apply_config(BTreeMap::from([("animations".into(), json!(false))]), 0)
        .unwrap();
    assert!(app.render(Duration::ZERO).is_some());
    let before = app.buffer() as *const _;
    assert!(app.render(Duration::from_millis(10)).is_none());
    assert_eq!(before, app.buffer() as *const _);
    drain_initial(&mut app);
    assert_eq!(app.next_deadline(), None);
}
#[test]
fn resize_and_activation_do_not_write_storage() {
    let mut app = app();
    drain_initial(&mut app);
    for i in 1..20 {
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
        !app.complete_hook(token, HookResult::Title("Old title".into()))
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
        !app.complete_hook(token, HookResult::Title("Stale geometry".into()))
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
    assert!(result.projection_changed);
    // The native snapshot already reports tab 3 active; repairing the empty projection
    // can select it before arrival delivery without requiring a duplicate host activation.
    assert!(result.commands.iter().all(|command| {
        !matches!(command, Command::Host(HostCommand::Activate(id)) if *id != 3)
    }));
}
#[test]
fn private_state_excludes_session_writes_but_persists_public_preferences() {
    let mut app = WindowApp::new("default", true);
    drain_initial(&mut app);
    app.dispatch(Intent::CreateSpace {
        name: "Secret".into(),
    })
    .unwrap();
    app.dispatch(Intent::SetSetting {
        key: "width".into(),
        value: json!(300),
    })
    .unwrap();
    app.set_verified_session(Some("verified-session".into()));
    let request = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(!request.private);
    assert!(request.operations.iter().all(|op| match op {
        Operation::Put { key, .. } | Operation::Delete { key, .. } =>
            matches!(key.scope, store::Scope::Profile { .. }),
        Operation::Read { scope } => matches!(scope, store::Scope::Profile { .. }),
    }));
}
#[test]
fn delayed_storage_read_merges_without_losing_local_settings() {
    let mut app = app();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.dispatch(Intent::SetSetting {
        key: "width".into(),
        value: json!(350),
    })
    .unwrap();
    let key = store::Key {
        scope: store::Scope::profile("default"),
        entity: "settings".into(),
        field: "width".into(),
    };
    let other = store::Key {
        field: "cards".into(),
        ..key.clone()
    };
    app.complete_storage(success(
        &request,
        vec![
            Record {
                key,
                value: Some(json!(280)),
                revision: 1,
            },
            Record {
                key: other,
                value: Some(json!(false)),
                revision: 1,
            },
        ],
    ))
    .unwrap();
    assert_eq!(app.model().settings.width, 350);
    assert!(!app.model().settings.cards);
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(write.operations.iter().any(
        |o| matches!(o,Operation::Put{key,value,..}if key.field=="width"&&value==&json!(350))
    ));
}
#[test]
fn unverified_session_ids_cannot_restore_assignment() {
    let mut app = app();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    let record = Record {
        key: store::Key {
            scope: store::Scope::Session {
                profile: "default".into(),
                incarnation: "unverified".into(),
            },
            entity: "tab:1".into(),
            field: "membership".into(),
        },
        value: Some(json!({"space":"home","manual":true,"pinned":true})),
        revision: 1,
    };
    app.complete_storage(success(&request, vec![record]))
        .unwrap();
    assert!(!app.model().tabs[&1].pinned);
}
#[test]
fn storage_requests_coalesce_and_old_completion_cannot_clear_newer_write() {
    let mut app = app();
    drain_initial(&mut app);
    app.dispatch(Intent::SetSetting {
        key: "width".into(),
        value: json!(300),
    })
    .unwrap();
    app.dispatch(Intent::SetSetting {
        key: "width".into(),
        value: json!(310),
    })
    .unwrap();
    assert!(
        app.take_storage_request(Duration::from_millis(50))
            .is_none()
    );
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    app.dispatch(Intent::SetSetting {
        key: "width".into(),
        value: json!(320),
    })
    .unwrap();
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
        |o| matches!(o,Operation::Put{key,value,..}if key.field=="width"&&value==&json!(320))
    ));
}
#[test]
fn native_close_restores_visible_successor_as_explicit_id_command() {
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

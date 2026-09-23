use super::{core::*, store::*, *};
use serde_json::json;
use std::time::Duration;

fn response(request: &Request, records: Vec<Record>) -> Response {
    Response {
        version: PROTOCOL_VERSION,
        request_id: request.request_id,
        revision: 1,
        records,
        error: None,
    }
}
fn profile_record(entity: &str, field: &str, value: serde_json::Value) -> Record {
    Record {
        key: Key {
            scope: Scope::profile("default"),
            entity: entity.into(),
            field: field.into(),
        },
        value: Some(value),
        revision: 1,
    }
}
fn initial(app: &mut WindowApp) {
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.complete_storage(response(&request, vec![])).unwrap();
}

fn declare(app: &mut WindowApp, spaces: serde_json::Value) {
    app.config(json!({"managed": {"spaces": spaces}})).unwrap();
}

#[test]
fn spaces_created_in_the_ui_leave_only_collapse_state_in_the_store() {
    let mut app = WindowApp::default();
    declare(&mut app, json!([]));
    initial(&mut app);
    app.dispatch(Intent::CreateSpace {
        name: "Created in UI".into(),
    })
    .unwrap();
    let id = app.model().selected_space.clone();
    let request = app.take_storage_request(Duration::from_secs(1)).unwrap();
    let fields = request
        .operations
        .iter()
        .filter_map(|op| match op {
            Operation::Put { key, .. } if key.entity == format!("space:{id}") => {
                Some(key.field.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(fields, ["collapsed"]);
    assert!(app.take_managed_write().unwrap().contains("Created in UI"));
}
#[test]
fn stale_store_records_for_declared_spaces_and_settings_are_ignored() {
    let mut app = WindowApp::default();
    declare(&mut app, json!([{"id": "work", "name": "Declared"}]));
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.complete_storage(response(
        &request,
        vec![
            profile_record("catalog", "order", json!(["work", "old"])),
            profile_record("space:work", "name", json!("Old name")),
            profile_record("space:old", "name", json!("Old space")),
            profile_record("settings", "width", json!(300)),
        ],
    ))
    .unwrap();
    let spaces = &app.model().spaces;
    assert_eq!((spaces.len(), spaces[0].name.as_str()), (1, "Declared"));
    assert_eq!(app.model().settings.width, Settings::default().width);
}
#[test]
fn refresh_is_coalesced_and_restores_template_derived_spaces() {
    let mut app = WindowApp::default();
    initial(&mut app);
    app.refresh_storage();
    app.refresh_storage();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    assert_eq!(request.operations.len(), 1);
    app.refresh_storage();
    let mut derived = Space::new("host-arch", "arch");
    derived.template = Some("host-$host".into());
    app.complete_storage(response(
        &request,
        vec![
            profile_record("catalog", "derived", json!([derived])),
            profile_record("space:host-arch", "collapsed", json!(true)),
        ],
    ))
    .unwrap();
    assert!(
        app.model()
            .spaces
            .iter()
            .any(|s| s.id == "host-arch" && s.collapsed)
    );
    assert!(app.take_storage_request(Duration::from_secs(1)).is_none());
}
#[test]
fn space_ids_are_unique_across_window_apps() {
    let (mut a, mut b) = (WindowApp::default(), WindowApp::default());
    a.dispatch(Intent::CreateSpace { name: "A".into() })
        .unwrap();
    b.dispatch(Intent::CreateSpace { name: "B".into() })
        .unwrap();
    assert_ne!(a.model().selected_space, b.model().selected_space);
}
#[test]
fn window_preferences_require_verified_session() {
    let mut app = WindowApp::default();
    app.set_window_identity(7);
    initial(&mut app);
    app.dispatch(Intent::CreateSpace {
        name: "Work".into(),
    })
    .unwrap();
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(write.operations.iter().all(|op| match op {
        Operation::Put { key, .. } | Operation::Delete { key, .. } =>
            !matches!(key.scope, Scope::Session { .. }),
        Operation::Read { scope } => !matches!(scope, Scope::Session { .. }),
    }));
}
#[test]
fn verified_window_preferences_restore_only_exact_window() {
    let mut app = WindowApp::default();
    app.set_window_identity(7);
    app.set_verified_session(Some("verified".into()));
    declare(
        &mut app,
        json!([{"id": "home", "name": "Home"}, {"id": "work", "name": "Work"}]),
    );
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    let scope = Scope::Session {
        profile: "default".into(),
        incarnation: "verified".into(),
    };
    app.complete_storage(response(
        &request,
        vec![Record {
            key: Key {
                scope,
                entity: "window:7".into(),
                field: "selected_space".into(),
            },
            value: Some(json!("work")),
            revision: 1,
        }],
    ))
    .unwrap();
    assert_eq!(app.model().selected_space, "work");
}
#[test]
fn shutdown_retries_cancelled_io_using_same_clock_domain() {
    let mut app = WindowApp::default();
    initial(&mut app);
    app.render(Duration::from_secs(50));
    app.dispatch(Intent::CreateSpace {
        name: "Durable".into(),
    })
    .unwrap();
    let write = app.take_storage_request(Duration::from_secs(51)).unwrap();
    app.restart_storage_after_cancel(write.request_id);
    app.teardown();
    assert!(app.storage_pending());
    assert!(app.storage_deadline().unwrap() >= Duration::from_secs(50));
    let retry = app.take_storage_request(app.now()).unwrap();
    assert_ne!(write.request_id, retry.request_id);
    assert_eq!(write.operations, retry.operations);
}

fn folder_records(id: &str, name: &str, space: &str) -> Vec<Record> {
    vec![
        profile_record(&format!("folder:{id}"), "name", json!(name)),
        profile_record(&format!("folder:{id}"), "space", json!(space)),
        profile_record(&format!("folder:{id}"), "collapsed", json!(true)),
    ]
}

fn discover(app: &mut WindowApp, revision: u64, ids: &[TabId]) {
    app.update(HostSnapshot {
        revision,
        tabs: ids
            .iter()
            .map(|id| Tab {
                id: *id,
                title: format!("Terminal {id}"),
                ..Tab::default()
            })
            .collect(),
        active_tab: ids.first().copied(),
        metrics: Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
}

fn membership_record(incarnation: &str, id: TabId, folder: &str) -> Record {
    Record {
        key: Key {
            scope: Scope::Session {
                profile: "default".into(),
                incarnation: incarnation.into(),
            },
            entity: format!("tab:{id}"),
            field: "membership".into(),
        },
        value: Some(json!({"space":"home","manual":true,"pinned":true,"folder":folder})),
        revision: 1,
    }
}

#[test]
fn folder_catalog_restores_without_a_session_but_membership_requires_verification() {
    for verified in [false, true] {
        let mut app = WindowApp::default();
        if verified {
            app.set_verified_session(Some("verified".into()));
        }
        discover(&mut app, 1, &[1]);
        let request = app.take_storage_request(Duration::ZERO).unwrap();
        let mut records = vec![profile_record("catalog", "folder_order", json!(["tools"]))];
        records.extend(folder_records("tools", "Tools", "home"));
        records.push(membership_record("verified", 1, "tools"));
        app.complete_storage(response(&request, records)).unwrap();
        assert_eq!(app.model().folders[0].name, "Tools");
        assert!(app.model().folders[0].collapsed);
        assert_eq!(
            app.model().tabs[&1].folder_id.as_deref(),
            verified.then_some("tools")
        );
    }
}

#[test]
fn delayed_discovery_restores_only_matching_session_folder_membership() {
    let mut app = WindowApp::default();
    app.set_verified_session(Some("verified".into()));
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    let mut records = vec![profile_record("catalog", "folder_order", json!(["tools"]))];
    records.extend(folder_records("tools", "Tools", "home"));
    records.extend([
        membership_record("verified", 1, "tools"),
        membership_record("old", 2, "tools"),
    ]);
    app.complete_storage(response(&request, records)).unwrap();
    discover(&mut app, 1, &[1, 2]);
    assert_eq!(app.model().tabs[&1].folder_id.as_deref(), Some("tools"));
    assert!(app.model().tabs[&2].folder_id.is_none());
    app.dispatch(Intent::AssignFolder {
        tab_id: 1,
        folder_id: None,
    })
    .unwrap();
    discover(&mut app, 2, &[1, 2]);
    assert!(app.model().tabs[&1].folder_id.is_none());
}

#[test]
fn private_folder_edits_persist_catalog_without_live_membership() {
    let mut app = WindowApp::new("default", true);
    app.set_verified_session(Some("verified".into()));
    initial(&mut app);
    discover(&mut app, 1, &[1]);
    app.dispatch(Intent::CreateFolder {
        name: "Tools".into(),
    })
    .unwrap();
    let folder = app.model().folders[0].id.clone();
    app.dispatch(Intent::AssignFolder {
        tab_id: 1,
        folder_id: Some(folder.clone()),
    })
    .unwrap();
    let request = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(request.operations.iter().any(|operation| matches!(operation, Operation::Put { key, .. } if key.entity == format!("folder:{folder}"))));
    assert!(request.operations.iter().all(|operation| match operation {
        Operation::Put { key, .. } | Operation::Delete { key, .. } =>
            matches!(key.scope, Scope::Profile { .. }),
        Operation::Read { scope } => matches!(scope, Scope::Profile { .. }),
    }));
}

#[test]
fn concurrent_folder_creation_merges_catalog_and_keeps_local_fields() {
    let mut app = WindowApp::default();
    initial(&mut app);
    app.dispatch(Intent::CreateFolder {
        name: "Local".into(),
    })
    .unwrap();
    let local = app.model().folders[0].id.clone();
    app.refresh_storage();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    let mut records = vec![profile_record("catalog", "folder_order", json!(["remote"]))];
    records.extend(folder_records("remote", "Remote", "home"));
    app.complete_storage(response(&request, records)).unwrap();
    assert_eq!(
        app.model()
            .folders
            .iter()
            .map(|folder| folder.name.as_str())
            .collect::<Vec<_>>(),
        ["Local", "Remote"]
    );
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(write.operations.iter().any(|operation| matches!(operation, Operation::Put { key, value, .. } if key.field == "folder_order" && value == &json!([local, "remote"]))));
}

#[test]
fn explicit_folder_deletion_wins_over_a_stale_remote_catalog() {
    let mut app = WindowApp::default();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    let mut records = vec![profile_record("catalog", "folder_order", json!(["tools"]))];
    records.extend(folder_records("tools", "Tools", "home"));
    app.complete_storage(response(&request, records.clone()))
        .unwrap();
    app.dispatch(Intent::DeleteFolder("tools".into())).unwrap();
    app.refresh_storage();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.complete_storage(response(&request, records)).unwrap();
    assert!(app.model().folders.is_empty());
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(write.operations.iter().any(|operation| matches!(operation, Operation::Delete { key, .. } if key.entity == "folder:tools" && key.field == "name")));
}

#[test]
fn malformed_folder_catalog_does_not_publish_partial_catalog_changes() {
    let mut app = WindowApp::default();
    declare(&mut app, json!([{"id": "work", "name": "Work"}]));
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    let mut records = vec![
        profile_record("space:work", "collapsed", json!(true)),
        profile_record("catalog", "folder_order", json!(["tools", "tools"])),
    ];
    records.extend(folder_records("tools", "Tools", "work"));
    assert!(app.complete_storage(response(&request, records)).is_err());
    assert!(!app.model().spaces[0].collapsed);
    assert!(app.model().folders.is_empty());
    assert!(app.storage_pending());
    let retry = app.take_storage_request(Duration::from_secs(3)).unwrap();
    app.complete_storage(response(
        &retry,
        vec![profile_record("catalog", "folder_order", json!(["tools"]))],
    ))
    .unwrap();
    assert_eq!(app.model().folders[0].name, "Tools");
    assert!(app.model().spaces[0].collapsed);
}

#[test]
fn folder_ids_are_unique_across_window_apps() {
    let (mut a, mut b) = (WindowApp::default(), WindowApp::default());
    a.dispatch(Intent::CreateFolder {
        name: "Tools".into(),
    })
    .unwrap();
    b.dispatch(Intent::CreateFolder {
        name: "Tools".into(),
    })
    .unwrap();
    assert_ne!(a.model().folders[0].id, b.model().folders[0].id);
}

fn acknowledge_write(app: &mut WindowApp, request: &Request) {
    let records = request
        .operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::Put { key, value, .. } => Some(Record {
                key: key.clone(),
                value: Some(value.clone()),
                revision: 1,
            }),
            Operation::Delete { key, .. } => Some(Record {
                key: key.clone(),
                value: None,
                revision: 1,
            }),
            Operation::Read { .. } => None,
        })
        .collect();
    app.complete_storage(response(request, records)).unwrap();
}

#[test]
fn acknowledged_local_folder_edits_do_not_hide_later_remote_catalog_changes() {
    let mut app = WindowApp::default();
    initial(&mut app);
    app.dispatch(Intent::CreateFolder {
        name: "Local".into(),
    })
    .unwrap();
    let local = app.model().folders[0].id.clone();
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    acknowledge_write(&mut app, &write);
    app.refresh_storage();
    let read = app.take_storage_request(Duration::from_secs(1)).unwrap();
    let mut records = vec![profile_record(
        "catalog",
        "folder_order",
        json!([local, "remote"]),
    )];
    records.extend(folder_records("remote", "Remote", "home"));
    records.push(profile_record(
        &format!("folder:{local}"),
        "name",
        json!("Renamed remotely"),
    ));
    app.complete_storage(response(&read, records)).unwrap();
    assert_eq!(
        app.model()
            .folders
            .iter()
            .map(|folder| folder.name.as_str())
            .collect::<Vec<_>>(),
        ["Renamed remotely", "Remote"]
    );
}

#[test]
fn acknowledging_an_older_write_preserves_edits_made_while_it_was_running() {
    let mut app = WindowApp::default();
    initial(&mut app);
    app.dispatch(Intent::CreateFolder {
        name: "First".into(),
    })
    .unwrap();
    let local = app.model().folders[0].id.clone();
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    app.dispatch(Intent::RenameFolder {
        id: local.clone(),
        name: "Later local edit".into(),
    })
    .unwrap();
    acknowledge_write(&mut app, &write);
    app.refresh_storage();
    let read = app.take_storage_request(Duration::from_secs(1)).unwrap();
    app.complete_storage(response(
        &read,
        vec![profile_record(
            &format!("folder:{local}"),
            "name",
            json!("First"),
        )],
    ))
    .unwrap();
    assert_eq!(app.model().folders[0].name, "Later local edit");
    let write = app.take_storage_request(Duration::from_secs(2)).unwrap();
    assert!(write.operations.iter().any(|operation| matches!(operation, Operation::Put { key, value, .. } if key.entity == format!("folder:{local}") && key.field == "name" && value == &json!("Later local edit"))));
}

#[test]
fn remote_folder_deletion_survives_a_concurrent_local_reorder() {
    let mut app = WindowApp::default();
    let read = app.take_storage_request(Duration::ZERO).unwrap();
    let mut records = vec![profile_record(
        "catalog",
        "folder_order",
        json!(["first", "second"]),
    )];
    records.extend(folder_records("first", "First", "home"));
    records.extend(folder_records("second", "Second", "home"));
    app.complete_storage(response(&read, records)).unwrap();
    app.dispatch(Intent::MoveFolder {
        id: "second".into(),
        index: 0,
    })
    .unwrap();
    app.refresh_storage();
    let read = app.take_storage_request(Duration::ZERO).unwrap();
    let mut deleted = profile_record("folder:first", "name", json!(null));
    deleted.value = None;
    app.complete_storage(response(
        &read,
        vec![
            profile_record("catalog", "folder_order", json!(["second"])),
            deleted,
        ],
    ))
    .unwrap();
    assert_eq!(
        app.model()
            .folders
            .iter()
            .map(|folder| folder.id.as_str())
            .collect::<Vec<_>>(),
        ["second"]
    );
}

#[test]
fn space_collapse_state_round_trips_through_the_store() {
    let mut app = WindowApp::default();
    discover(&mut app, 1, &[1]);
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.complete_storage(response(
        &request,
        vec![profile_record("space:home", "collapsed", json!(true))],
    ))
    .unwrap();
    assert!(app.model().spaces[0].collapsed);

    app.dispatch(Intent::ToggleSpace("home".into())).unwrap();
    let write = app.take_storage_request(Duration::from_secs(60)).unwrap();
    assert!(write.operations.iter().any(|op| matches!(
        op,
        Operation::Put { key, value, .. }
            if key.entity == "space:home" && key.field == "collapsed" && value == &json!(false)
    )));
}

use serde_json::json;
use std::time::Duration;
use vtabs_app::{core::*, store::*, *};

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

#[test]
fn configured_spaces_keep_project_created_catalog_and_persistence() {
    let mut app = WindowApp::default();
    app.configure_spaces(vec![Space::new("work", "From config")], vec![])
        .unwrap();
    initial(&mut app);
    app.dispatch(Intent::CreateSpace {
        name: "Created in UI".into(),
    })
    .unwrap();
    let id = app.model().selected_space.clone();
    let request = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(
        request
            .operations
            .iter()
            .any(|op| matches!(op,Operation::Put{key,..}if key.entity==format!("space:{id}")))
    );
    app.configure_spaces(vec![Space::new("work", "Updated config")], vec![])
        .unwrap();
    assert!(app.model().spaces.iter().any(|s| s.id == id));
    assert_eq!(app.model().spaces[0].name, "Updated config");
}
#[test]
fn configuration_wins_restored_fields_without_hiding_dynamic_spaces() {
    let mut app = WindowApp::default();
    app.configure_spaces(vec![Space::new("work", "Configured")], vec![])
        .unwrap();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.complete_storage(response(
        &request,
        vec![
            profile_record("catalog", "order", json!(["work", "dynamic"])),
            profile_record("space:work", "name", json!("Old name")),
            profile_record("space:dynamic", "name", json!("User-created")),
        ],
    ))
    .unwrap();
    assert_eq!(app.model().spaces[0].name, "Configured");
    assert!(
        app.model()
            .spaces
            .iter()
            .any(|s| s.id == "dynamic" && s.name == "User-created")
    );
}
#[test]
fn refresh_is_coalesced_and_discovers_remote_spaces() {
    let mut app = WindowApp::default();
    initial(&mut app);
    app.refresh_storage();
    app.refresh_storage();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    assert_eq!(request.operations.len(), 1);
    app.refresh_storage();
    app.complete_storage(response(
        &request,
        vec![
            profile_record("catalog", "order", json!(["home", "remote"])),
            profile_record("space:home", "name", json!("Home")),
            profile_record("space:remote", "name", json!("Other window")),
        ],
    ))
    .unwrap();
    assert!(app.model().spaces.iter().any(|s| s.id == "remote"));
    assert!(app.take_storage_request(Duration::from_secs(1)).is_none());
}
#[test]
fn concurrent_space_creation_merges_remote_catalog_ids() {
    let mut app = WindowApp::default();
    initial(&mut app);
    app.dispatch(Intent::CreateSpace {
        name: "Local".into(),
    })
    .unwrap();
    let local = app.model().selected_space.clone();
    app.refresh_storage();
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    app.complete_storage(response(
        &request,
        vec![
            profile_record("catalog", "order", json!(["home", "remote"])),
            profile_record("space:remote", "name", json!("Remote")),
        ],
    ))
    .unwrap();
    assert!(app.model().spaces.iter().any(|s| s.id == local));
    assert!(app.model().spaces.iter().any(|s| s.id == "remote"));
    let write = app.take_storage_request(Duration::from_secs(1)).unwrap();
    assert!(write.operations.iter().any(|op|matches!(op,Operation::Put{key,value,..}if key.entity=="catalog"&&key.field=="order"&&value.as_array().unwrap().contains(&json!(local))&&value.as_array().unwrap().contains(&json!("remote")))));
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
    let request = app.take_storage_request(Duration::ZERO).unwrap();
    let scope = Scope::Session {
        profile: "default".into(),
        incarnation: "verified".into(),
    };
    app.complete_storage(response(
        &request,
        vec![
            profile_record("catalog", "order", json!(["home", "work"])),
            profile_record("space:work", "name", json!("Work")),
            Record {
                key: Key {
                    scope,
                    entity: "window:7".into(),
                    field: "selected_space".into(),
                },
                value: Some(json!("work")),
                revision: 1,
            },
        ],
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

use vtabs_app::{core::*, *};

fn discover(app: &mut WindowApp, revision: u64, active: TabId) {
    app.update(NativeSnapshot {
        revision,
        tabs: (1..=active)
            .map(|id| Tab {
                id,
                title: format!("Terminal {id}"),
                ..Tab::default()
            })
            .collect(),
        active_tab: Some(active),
        metrics: Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
}

fn app() -> (WindowApp, String) {
    let mut app = WindowApp::default();
    discover(&mut app, 1, 1);
    app.dispatch(Intent::CreateFolder {
        name: "Tools".into(),
    })
    .unwrap();
    let folder = app.model().folders[0].id.clone();
    (app, folder)
}

#[test]
fn folder_spawn_assigns_exact_new_tab_and_expands_its_folder() {
    let (mut app, folder) = app();
    app.dispatch(Intent::ToggleFolder(folder.clone())).unwrap();
    let token = app.reserve_spawn_in_folder(&folder).unwrap();
    assert_eq!(token.folder_id.as_deref(), Some(folder.as_str()));
    discover(&mut app, 2, 3);
    app.spawn_completed_with_token(2, token).unwrap();
    assert_eq!(
        app.model().tabs[&2].folder_id.as_deref(),
        Some(folder.as_str())
    );
    assert!(app.model().tabs[&3].folder_id.is_none());
    assert!(app.model().tabs[&2].pinned);
    assert!(app.model().tabs[&2].manual_assignment);
    assert_eq!(app.model().selected_tab, Some(2));
    assert!(!app.model().folders[0].collapsed);
}

#[test]
fn folder_spawn_never_steals_newer_space_selection() {
    let (mut app, folder) = app();
    let token = app.reserve_spawn_in_folder(&folder).unwrap();
    app.dispatch(Intent::CreateSpace {
        name: "Later".into(),
    })
    .unwrap();
    let selected = app.model().selected_space.clone();
    discover(&mut app, 2, 2);
    app.spawn_completed_with_token(2, token).unwrap();
    assert_eq!(app.model().selected_space, selected);
    assert_eq!(app.model().selected_tab, None);
    assert_eq!(
        app.model().tabs[&2].folder_id.as_deref(),
        Some(folder.as_str())
    );
    assert_eq!(app.model().tabs[&2].space_id, "home");
}

#[test]
fn deleted_folder_does_not_capture_new_tab_in_recreated_folder() {
    let (mut app, folder) = app();
    let token = app.reserve_spawn_in_folder(&folder).unwrap();
    app.dispatch(Intent::DeleteFolder(folder.clone())).unwrap();
    app.dispatch(Intent::CreateFolder {
        name: "Tools".into(),
    })
    .unwrap();
    assert_ne!(app.model().folders[0].id, folder);
    discover(&mut app, 2, 2);
    app.spawn_completed_with_token(2, token).unwrap();
    assert!(app.model().tabs[&2].folder_id.is_none());
    assert!(!app.model().tabs[&2].pinned);
    assert_eq!(app.model().tabs[&2].space_id, "home");
}

#[test]
fn deleted_spawn_space_uses_current_space_without_orphan_membership() {
    let (mut app, folder) = app();
    let token = app.reserve_spawn_in_folder(&folder).unwrap();
    app.dispatch(Intent::CreateSpace {
        name: "Survivor".into(),
    })
    .unwrap();
    let selected = app.model().selected_space.clone();
    app.dispatch(Intent::DeleteSpace {
        id: "home".into(),
        destination: Some(selected.clone()),
    })
    .unwrap();
    discover(&mut app, 2, 2);
    app.spawn_completed_with_token(2, token).unwrap();
    assert!(app.model().folders.is_empty());
    assert!(app.model().tabs[&2].folder_id.is_none());
    assert_eq!(app.model().tabs[&2].space_id, selected);
}

#[test]
fn canceled_failed_spawn_never_assigns_a_later_terminal() {
    let (mut app, folder) = app();
    let token = app.reserve_spawn_in_folder(&folder).unwrap();
    app.cancel_spawn(&token);
    discover(&mut app, 2, 2);
    let update = app.spawn_completed_with_token(2, token).unwrap();
    assert!(!update.model_changed);
    assert!(update.commands.is_empty());
    assert!(app.model().tabs[&2].folder_id.is_none());
}

#[test]
fn tampered_folder_token_cannot_modify_or_consume_a_valid_pending_spawn() {
    let (mut app, folder) = app();
    let token = app.reserve_spawn_in_folder(&folder).unwrap();
    let mut tampered = token.clone();
    tampered.folder_id = None;
    discover(&mut app, 2, 2);
    app.cancel_spawn(&tampered);
    assert!(
        !app.spawn_completed_with_token(2, tampered)
            .unwrap()
            .model_changed
    );
    assert!(
        app.spawn_completed_with_token(2, token.clone())
            .unwrap()
            .model_changed
    );
    assert_eq!(
        app.model().tabs[&2].folder_id.as_deref(),
        Some(folder.as_str())
    );
    assert!(
        !app.spawn_completed_with_token(2, token)
            .unwrap()
            .model_changed
    );
}

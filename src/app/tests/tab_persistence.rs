use super::{core::*, store::*, *};
use serde_json::json;
use std::path::PathBuf;

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "vtabs-tabs-{}-{epoch}-{sequence}.sqlite",
            std::process::id()
        )))
    }
    fn answer(&self, request: &Request) -> Response {
        let mut connection = sqlite::open(&self.0).unwrap();
        sqlite::execute(&mut connection, request)
            .unwrap_or_else(|error| Response::failure(request.request_id, error))
    }
    fn flush(&self, app: &mut WindowApp) {
        for _ in 0..200 {
            if !app.storage_pending() {
                return;
            }
            let now = app.storage_deadline().unwrap().max(app.now());
            let request = app.take_storage_request(now).unwrap();
            app.complete_storage(self.answer(&request)).unwrap();
        }
        panic!("storage never settled");
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

fn app() -> WindowApp {
    let mut app = WindowApp::default();
    app.config(json!({"spaces": [
        {"id": "home", "name": "Home"},
        {"id": "work", "name": "Work"},
        {"id": "docs", "name": "Docs"}
    ]}))
    .unwrap();
    app
}

fn tab(id: TabId, session: &str, entity: &str) -> HostTab {
    HostTab {
        id,
        identities: vec![TabIdentity {
            session: session.into(),
            entity: entity.into(),
        }],
        ..Default::default()
    }
}

fn discover(app: &mut WindowApp, revision: u64, tabs: Vec<HostTab>) {
    app.update(HostSnapshot {
        active_tab: tabs.first().map(|tab| tab.id),
        revision,
        tabs,
        metrics: Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
}

fn place(app: &mut WindowApp, id: TabId, space: &str) {
    app.dispatch(Intent::AssignTab {
        id,
        space_id: space.into(),
    })
    .unwrap();
    app.dispatch(Intent::PinTab { id, pinned: true }).unwrap();
}

#[test]
fn gui_restart_restores_two_mux_servers_with_new_local_ids() {
    let db = Database::new();
    let mut first = app();
    discover(
        &mut first,
        1,
        vec![tab(1, "macie", "tab:7"), tab(2, "archie", "tab:7")],
    );
    db.flush(&mut first);
    place(&mut first, 1, "work");
    place(&mut first, 2, "docs");
    first.dispatch(Intent::SelectSpace("docs".into())).unwrap();
    first
        .dispatch(Intent::CreateFolder {
            name: "References".into(),
        })
        .unwrap();
    let folder = first.model().folders[0].id.clone();
    first
        .dispatch(Intent::AssignFolder {
            tab_id: 2,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    first.teardown();
    db.flush(&mut first);
    drop(first);

    let mut restarted = app();
    discover(
        &mut restarted,
        1,
        vec![tab(101, "macie", "tab:7"), tab(203, "archie", "tab:7")],
    );
    db.flush(&mut restarted);
    let macie = &restarted.model().tabs[&101];
    let archie = &restarted.model().tabs[&203];
    assert_eq!(macie.space_id, "work");
    assert!(macie.pinned && macie.manual_assignment);
    assert_eq!(archie.space_id, "docs");
    assert!(archie.pinned && archie.manual_assignment);
    assert_eq!(archie.folder_id.as_deref(), Some(folder.as_str()));
    assert_eq!(restarted.model().selected_tab, Some(101));
    assert_eq!(restarted.model().selected_space, "work");
}

#[test]
fn disconnect_keeps_placement_but_a_new_server_session_cannot_inherit_it() {
    let db = Database::new();
    let mut app = app();
    discover(&mut app, 1, vec![tab(1, "macie-before-reboot", "tab:7")]);
    db.flush(&mut app);
    place(&mut app, 1, "work");
    db.flush(&mut app);
    discover(&mut app, 2, vec![]);
    db.flush(&mut app);
    discover(&mut app, 3, vec![tab(99, "macie-before-reboot", "tab:7")]);
    assert_eq!(app.model().tabs[&99].space_id, "work");
    assert!(app.model().tabs[&99].pinned);

    discover(&mut app, 4, vec![]);
    app.dispatch(Intent::SelectSpace("home".into())).unwrap();
    discover(&mut app, 5, vec![tab(1, "macie-after-reboot", "tab:7")]);
    db.flush(&mut app);
    assert_eq!(app.model().tabs[&1].space_id, "home");
    assert!(!app.model().tabs[&1].pinned);
}

#[test]
fn late_storage_read_preserves_local_edits_field_by_field() {
    let db = Database::new();
    let mut first = app();
    discover(&mut first, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut first);
    first
        .dispatch(Intent::AssignTab {
            id: 1,
            space_id: "work".into(),
        })
        .unwrap();
    first.dispatch(Intent::SelectSpace("work".into())).unwrap();
    first
        .dispatch(Intent::CreateFolder {
            name: "Tools".into(),
        })
        .unwrap();
    let folder = first.model().folders[0].id.clone();
    first
        .dispatch(Intent::AssignFolder {
            tab_id: 1,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    db.flush(&mut first);
    let mut second = app();
    discover(&mut second, 1, vec![tab(9, "macie", "tab:7")]);
    let read = second.take_storage_request(second.now()).unwrap();
    let answer = db.answer(&read);
    second
        .dispatch(Intent::PinTab {
            id: 9,
            pinned: true,
        })
        .unwrap();
    second.complete_storage(answer).unwrap();
    assert_eq!(second.model().tabs[&9].space_id, "work");
    assert!(second.model().tabs[&9].pinned);
    assert_eq!(
        second.model().tabs[&9].folder_id.as_deref(),
        Some(folder.as_str())
    );
    db.flush(&mut second);
}

#[test]
fn restored_panes_keep_membership_after_new_backing_tabs_and_split_selection() {
    let db = Database::new();
    let mut first = app();
    let mut split = tab(1, "archie", "pane:5");
    split.identities.push(TabIdentity {
        session: "archie".into(),
        entity: "pane:6".into(),
    });
    discover(&mut first, 1, vec![split.clone()]);
    db.flush(&mut first);
    place(&mut first, 1, "work");
    db.flush(&mut first);
    let mut second = app();
    discover(&mut second, 1, vec![tab(45, "archie", "pane:6")]);
    db.flush(&mut second);
    assert_eq!(second.model().tabs[&45].space_id, "work");
    assert!(second.model().tabs[&45].pinned);
    // A later pane snapshot must not revert membership after restoration.
    discover(&mut second, 2, vec![tab(45, "archie", "pane:6")]);
    assert_eq!(second.model().tabs[&45].space_id, "work");
    assert!(!second.storage_pending());
}

#[test]
fn deleting_a_folder_while_disconnected_does_not_restore_it() {
    let db = Database::new();
    let mut first = app();
    discover(&mut first, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut first);
    first
        .dispatch(Intent::CreateFolder {
            name: "Tools".into(),
        })
        .unwrap();
    let folder = first.model().folders[0].id.clone();
    first
        .dispatch(Intent::AssignFolder {
            tab_id: 1,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    db.flush(&mut first);
    discover(&mut first, 2, vec![]);
    first.dispatch(Intent::DeleteFolder(folder)).unwrap();
    db.flush(&mut first);
    let mut second = app();
    discover(&mut second, 1, vec![tab(77, "macie", "tab:7")]);
    db.flush(&mut second);
    assert!(second.model().folders.is_empty());
    assert!(second.model().tabs[&77].folder_id.is_none());
    assert!(second.model().tabs[&77].pinned);
}

#[test]
fn reading_other_windows_does_not_erase_their_tab_placement() {
    let db = Database::new();
    let mut first = app();
    discover(&mut first, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut first);
    place(&mut first, 1, "work");
    db.flush(&mut first);
    let mut other = app();
    discover(&mut other, 1, vec![tab(3, "macie", "tab:8")]);
    db.flush(&mut other);
    place(&mut other, 3, "docs");
    db.flush(&mut other);
    let mut restarted = app();
    discover(
        &mut restarted,
        1,
        vec![tab(45, "macie", "tab:7"), tab(46, "macie", "tab:8")],
    );
    db.flush(&mut restarted);
    assert_eq!(restarted.model().tabs[&45].space_id, "work");
    assert_eq!(restarted.model().tabs[&46].space_id, "docs");
}

#[test]
fn unchanged_host_updates_do_not_delay_pending_placement_writes() {
    let db = Database::new();
    let mut app = app();
    discover(&mut app, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut app);
    place(&mut app, 1, "work");
    let deadline = app.storage_deadline();
    app.prepare_render(app.now() + std::time::Duration::from_millis(50));
    discover(&mut app, 2, vec![tab(1, "macie", "tab:7")]);
    assert_eq!(app.storage_deadline(), deadline);
    db.flush(&mut app);
}

#[test]
fn failed_initial_read_does_not_overwrite_saved_placement() {
    let db = Database::new();
    let mut first = app();
    discover(&mut first, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut first);
    place(&mut first, 1, "work");
    db.flush(&mut first);

    let mut second = app();
    discover(&mut second, 1, vec![tab(9, "macie", "tab:7")]);
    let read = second.take_storage_request(second.now()).unwrap();
    let mut answer = db.answer(&read);
    let space = answer
        .records
        .iter_mut()
        .find(|r| r.key.field == "space")
        .unwrap();
    space.value = Some(json!(123));
    assert!(second.complete_storage(answer).is_err());
    discover(&mut second, 2, vec![tab(9, "macie", "tab:7")]);
    assert_eq!(second.model().tabs[&9].space_id, "home");
    assert!(!second.model().tabs[&9].pinned);
    db.flush(&mut second);
    assert_eq!(second.model().tabs[&9].space_id, "work");
    assert!(second.model().tabs[&9].pinned);
}

#[test]
fn another_profile_does_not_inherit_the_same_shells_placement() {
    let db = Database::new();
    let mut first = app();
    discover(&mut first, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut first);
    place(&mut first, 1, "work");
    db.flush(&mut first);

    let mut second = WindowApp::new("another-profile");
    discover(&mut second, 1, vec![tab(9, "macie", "tab:7")]);
    db.flush(&mut second);
    assert_eq!(second.model().tabs[&9].space_id, "home");
    assert!(!second.model().tabs[&9].pinned);
}

#[test]
fn provisional_routing_does_not_overwrite_saved_manual_membership() {
    let db = Database::new();
    let mut first = app();
    discover(&mut first, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut first);
    place(&mut first, 1, "work");
    db.flush(&mut first);

    let mut second = app();
    discover(&mut second, 1, vec![tab(9, "macie", "tab:7")]);
    let read = second.take_storage_request(second.now()).unwrap();
    second
        .complete_hook_batch(vec![(
            second.hook_token(Some(9)),
            HookResult::Route(Some("docs".into())),
        )])
        .unwrap();
    assert_eq!(second.model().tabs[&9].space_id, "docs");
    discover(&mut second, 2, vec![tab(9, "macie", "tab:7")]);
    second
        .dispatch(Intent::PinTab {
            id: 9,
            pinned: true,
        })
        .unwrap();
    second.complete_storage(db.answer(&read)).unwrap();
    assert_eq!(second.model().tabs[&9].space_id, "work");
    assert!(second.model().tabs[&9].manual_assignment);
    assert!(second.model().tabs[&9].pinned);
    db.flush(&mut second);
}

#[test]
fn joining_saved_panes_inherit_existing_tab_membership_before_and_after_reads() {
    for already_loaded in [false, true] {
        let db = Database::new();
        let mut source = app();
        discover(&mut source, 1, vec![tab(5, "archie", "pane:5")]);
        source
            .dispatch(Intent::AssignTab {
                id: 5,
                space_id: "docs".into(),
            })
            .unwrap();
        db.flush(&mut source);

        let mut destination = app();
        let local = HostTab {
            id: 1,
            ..HostTab::default()
        };
        let mut initial = vec![local.clone()];
        if already_loaded {
            initial.push(tab(2, "archie", "pane:5"));
        }
        discover(&mut destination, 1, initial);
        db.flush(&mut destination);
        place(&mut destination, 1, "work");
        destination
            .dispatch(Intent::SelectSpace("work".into()))
            .unwrap();
        destination
            .dispatch(Intent::CreateFolder {
                name: "Tools".into(),
            })
            .unwrap();
        let folder = destination.model().folders[0].id.clone();
        destination
            .dispatch(Intent::AssignFolder {
                tab_id: 1,
                folder_id: Some(folder.clone()),
            })
            .unwrap();
        db.flush(&mut destination);

        discover(&mut destination, 2, vec![tab(1, "archie", "pane:5")]);
        let assert_membership = |app: &WindowApp, id| {
            let tab = &app.model().tabs[&id];
            assert_eq!(tab.space_id, "work");
            assert!(tab.pinned && tab.manual_assignment);
            assert_eq!(tab.folder_id.as_deref(), Some(folder.as_str()));
        };
        assert_membership(&destination, 1);
        destination.refresh_storage();
        let request = destination.take_storage_request(destination.now()).unwrap();
        let response = db.answer(&request);
        assert!(
            response
                .records
                .iter()
                .any(|record| record.key.field == "space" && record.value == Some(json!("docs")))
        );
        // Another host snapshot while the stale storage response is in flight.
        discover(&mut destination, 3, vec![tab(1, "archie", "pane:5")]);
        destination.complete_storage(response).unwrap();
        assert_membership(&destination, 1);
        db.flush(&mut destination);

        let mut restarted = app();
        discover(&mut restarted, 1, vec![tab(99, "archie", "pane:5")]);
        db.flush(&mut restarted);
        assert_membership(&restarted, 99);
    }
}

#[test]
fn transfer_preserves_unflushed_profile_edits_against_a_stale_database_response() {
    let db = Database::new();
    let mut source = app();
    discover(&mut source, 1, vec![tab(1, "macie", "tab:7")]);
    source
        .dispatch(Intent::CreateFolder {
            name: "Original".into(),
        })
        .unwrap();
    let folder = source.model().folders[0].id.clone();
    source
        .dispatch(Intent::AssignFolder {
            tab_id: 1,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    db.flush(&mut source);
    source
        .dispatch(Intent::RenameFolder {
            id: folder.clone(),
            name: "Renamed".into(),
        })
        .unwrap();
    source
        .dispatch(Intent::ToggleFolder(folder.clone()))
        .unwrap();
    source.dispatch(Intent::ToggleSpace("home".into())).unwrap();
    let mut destination = app();
    destination
        .import_transfer_before_snapshot(source.export_transfer(1).unwrap())
        .unwrap();
    let read = destination.take_storage_request(destination.now()).unwrap();
    let response = db.answer(&read);
    assert!(
        response
            .records
            .iter()
            .any(|record| record.value == Some(json!("Original")))
    );
    destination.complete_storage(response).unwrap();
    assert_eq!(destination.model().folders[0].name, "Renamed");
    assert!(destination.model().folders[0].collapsed);
    assert!(destination.model().spaces[0].collapsed);
    assert_eq!(
        destination.model().tabs[&1].folder_id.as_deref(),
        Some(folder.as_str())
    );
    db.flush(&mut destination);
    let mut restarted = app();
    discover(&mut restarted, 1, vec![tab(9, "macie", "tab:7")]);
    db.flush(&mut restarted);
    assert_eq!(restarted.model().folders[0].name, "Renamed");
    assert!(restarted.model().folders[0].collapsed);
}

#[test]
fn transfer_keeps_pending_folder_deletions_and_merges_concurrent_additions() {
    let db = Database::new();
    let mut source = app();
    discover(&mut source, 1, vec![tab(1, "macie", "tab:7")]);
    source
        .dispatch(Intent::CreateFolder {
            name: "Delete me".into(),
        })
        .unwrap();
    let deleted = source.model().folders[0].id.clone();
    source
        .dispatch(Intent::AssignFolder {
            tab_id: 1,
            folder_id: Some(deleted.clone()),
        })
        .unwrap();
    db.flush(&mut source);
    source
        .dispatch(Intent::DeleteFolder(deleted.clone()))
        .unwrap();

    let mut other = app();
    db.flush(&mut other);
    other
        .dispatch(Intent::CreateFolder {
            name: "Concurrent".into(),
        })
        .unwrap();
    db.flush(&mut other);

    let mut destination = app();
    // Exercise the serialized transfer contract as well as the in-memory move.
    let transfer =
        serde_json::from_slice(&serde_json::to_vec(&source.export_transfer(1).unwrap()).unwrap())
            .unwrap();
    destination
        .import_transfer_before_snapshot(transfer)
        .unwrap();
    db.flush(&mut destination);
    assert_eq!(destination.model().folders.len(), 1);
    assert_eq!(destination.model().folders[0].name, "Concurrent");
    assert!(destination.model().tabs[&1].folder_id.is_none());
    let mut restarted = app();
    discover(&mut restarted, 1, vec![tab(9, "macie", "tab:7")]);
    db.flush(&mut restarted);
    assert_eq!(restarted.model().folders.len(), 1);
    assert_eq!(restarted.model().folders[0].name, "Concurrent");
    assert!(restarted.model().tabs[&9].folder_id.is_none());
}

#[test]
fn retained_tab_history_never_expands_live_identity_reads() {
    let db = Database::new();
    let mut first = app();
    // More than MAX_RECORDS / four fields, while only one shell is live at a time.
    for id in 1..=1025 {
        discover(&mut first, id, vec![tab(id, "macie", &format!("tab:{id}"))]);
        db.flush(&mut first);
        if id == 1 {
            place(&mut first, id, "docs");
            db.flush(&mut first);
            first.dispatch(Intent::SelectSpace("home".into())).unwrap();
        }
    }
    place(&mut first, 1025, "work");
    db.flush(&mut first);
    let mut restarted = app();
    discover(&mut restarted, 1, vec![tab(77, "macie", "tab:1025")]);
    let read = restarted.take_storage_request(restarted.now()).unwrap();
    assert_eq!(
        read.operations
            .iter()
            .filter(|op| matches!(op, Operation::ReadKey { .. }))
            .count(),
        4
    );
    assert!(!read.operations.iter().any(|op| matches!(
        op,
        Operation::Read {
            scope: Scope::MuxSession { .. }
        }
    )));
    let response = db.answer(&read);
    assert_eq!(
        response
            .records
            .iter()
            .filter(|record| matches!(record.key.scope, Scope::MuxSession { .. }))
            .count(),
        4
    );
    restarted.complete_storage(response).unwrap();
    assert_eq!(restarted.model().tabs[&77].space_id, "work");
    assert!(restarted.model().tabs[&77].pinned);
    db.flush(&mut restarted);
    // A long-running GUI's refresh is bounded by its current identities too.
    first.refresh_storage();
    let refresh = first.take_storage_request(first.now()).unwrap();
    assert_eq!(refresh.operations.len(), 5);
    first.complete_storage(db.answer(&refresh)).unwrap();
    // Historical records remain available to a shell reattached later.
    discover(&mut restarted, 2, vec![tab(78, "macie", "tab:1")]);
    db.flush(&mut restarted);
    assert_eq!(restarted.model().tabs[&78].space_id, "docs");
    assert!(restarted.model().tabs[&78].pinned);
}

#[test]
fn many_active_tabs_batch_reads_and_writes_without_splitting_membership() {
    let db = Database::new();
    let mut first = app();
    let hosts = (0..1025)
        .map(|id| tab(id + 1, "macie", &format!("tab:{id:04}")))
        .collect::<Vec<_>>();
    discover(&mut first, 1, hosts.clone());
    first.dispatch(Intent::SelectSpace("work".into())).unwrap();
    first
        .dispatch(Intent::CreateFolder {
            name: "Boundary".into(),
        })
        .unwrap();
    let folder = first.model().folders[0].id.clone();
    // 31 complete identities plus profile fit in the first batch; identity 32 must wait.
    first
        .dispatch(Intent::AssignFolder {
            tab_id: 32,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    db.flush(&mut first);

    let mut restarted = app();
    discover(&mut restarted, 1, hosts);
    let mut read_batches = 0;
    for _ in 0..200 {
        if !restarted.storage_pending() {
            break;
        }
        let request = restarted
            .take_storage_request(restarted.storage_deadline().unwrap().max(restarted.now()))
            .unwrap();
        assert!(request.operations.len() <= MAX_OPERATIONS);
        let mut per_identity = std::collections::BTreeMap::<_, usize>::new();
        for operation in &request.operations {
            if let Operation::ReadKey { key } = operation {
                *per_identity.entry((&key.scope, &key.entity)).or_default() += 1;
            }
        }
        if !per_identity.is_empty() {
            read_batches += 1;
            assert!(per_identity.values().all(|count| *count == 4));
        }
        let response = db.answer(&request);
        assert!(
            response
                .records
                .iter()
                .filter(|record| matches!(record.key.scope, Scope::MuxSession { .. }))
                .count()
                <= MAX_OPERATIONS
        );
        restarted.complete_storage(response).unwrap();
    }
    assert!(!restarted.storage_pending());
    assert!(read_batches > 30);
    let boundary = &restarted.model().tabs[&32];
    assert_eq!(boundary.space_id, "work");
    assert!(boundary.pinned);
    assert_eq!(boundary.folder_id.as_deref(), Some(folder.as_str()));
}

#[test]
fn detached_pending_writes_retry_conflicts_with_exact_identity_reads() {
    let db = Database::new();
    let mut first = app();
    discover(&mut first, 1, vec![tab(1, "macie", "tab:7")]);
    db.flush(&mut first);
    first
        .dispatch(Intent::PinTab {
            id: 1,
            pinned: true,
        })
        .unwrap();
    let pending = first
        .take_storage_request(first.storage_deadline().unwrap())
        .unwrap();
    discover(&mut first, 2, vec![]);
    let mut other = app();
    discover(&mut other, 1, vec![tab(2, "macie", "tab:7")]);
    db.flush(&mut other);
    place(&mut other, 2, "docs");
    db.flush(&mut other);
    assert!(first.complete_storage(db.answer(&pending)).is_err());
    let reread = first
        .take_storage_request(first.storage_deadline().unwrap())
        .unwrap();
    assert_eq!(
        reread
            .operations
            .iter()
            .filter(|op| matches!(op, Operation::ReadKey { .. }))
            .count(),
        4
    );
    first.complete_storage(db.answer(&reread)).unwrap();
    db.flush(&mut first);
    other.refresh_storage();
    db.flush(&mut other);
    assert!(other.model().tabs[&2].pinned);
    assert_eq!(other.model().tabs[&2].space_id, "docs");
}

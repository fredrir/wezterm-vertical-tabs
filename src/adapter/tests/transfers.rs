use super::*;

fn discover(app: &mut WindowApp, revision: u64, ids: &[u64]) {
    app.update(app::HostSnapshot {
        revision,
        tabs: ids
            .iter()
            .map(|id| core::Tab {
                id: *id,
                ..Default::default()
            })
            .collect(),
        active_tab: ids.last().copied(),
        metrics: app::Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
}

#[test]
fn placement_waits_for_an_arrival_and_expires_before_unrelated_tabs() {
    let mut transfers = Transfers::default();
    let now = Instant::now();
    assert_eq!(transfers.observe_hosts(vec![1, 2], now), None);
    transfers.place_detached(Some(4), now);
    assert_eq!(
        transfers.observe_hosts(vec![2, 1], now + Duration::from_secs(1)),
        None
    );
    assert_eq!(
        transfers.observe_hosts(vec![2, 1, 3], now + Duration::from_secs(2)),
        Some((3, 4))
    );
    assert_eq!(
        transfers.observe_hosts(vec![2, 1, 3, 4], now + Duration::from_secs(2)),
        None
    );
    transfers.place_detached(Some(1), now);
    assert_eq!(
        transfers.observe_hosts(vec![2, 1, 3, 4, 5], now + PLACEMENT_TIMEOUT),
        None
    );
    assert_eq!(
        transfers.observe_hosts(vec![2, 1, 3, 4, 5, 6], now + PLACEMENT_TIMEOUT),
        None
    );
    transfers.place_detached(Some(1), now);
    transfers.place_detached(None, now);
    assert_eq!(transfers.observe_hosts(vec![7], now), None);
}

#[test]
fn folder_destination_survives_new_window_reservation_and_is_consumed_once() {
    let mut transfers = Transfers::default();
    let mut app = WindowApp::default();
    discover(&mut app, 1, &[1]);
    app.dispatch(core::Intent::CreateFolder {
        name: "Tools".into(),
    })
    .unwrap();
    let folder = app.model().folders[0].id.clone();
    transfers.spawn_into("home".into(), Some(folder.clone()));
    assert!(transfers.reserve_spawn(&mut app, true).is_none());
    app.dispatch(core::Intent::CreateSpace {
        name: "Other".into(),
    })
    .unwrap();
    let selected = app.model().selected_space.clone();
    let token = transfers.reserve_spawn(&mut app, false).unwrap();
    assert_eq!(token.space_id, "home");
    assert_eq!(token.folder_id, Some(folder));
    let next = transfers.reserve_spawn(&mut app, false).unwrap();
    assert_eq!(next.space_id, selected);
    assert_eq!(next.folder_id, None);
}

#[test]
fn deleted_destination_does_not_leak_into_the_next_spawn() {
    let mut transfers = Transfers::default();
    let mut app = WindowApp::default();
    transfers.spawn_into("deleted".into(), None);
    assert!(transfers.reserve_spawn(&mut app, false).is_none());
    assert_eq!(
        transfers.reserve_spawn(&mut app, false).unwrap().space_id,
        "home"
    );
}

#[test]
fn show_completion_waits_for_its_tab_and_is_consumed_once() {
    let mut transfers = Transfers::default();
    let mut app = WindowApp::default();
    discover(&mut app, 1, &[1]);
    assert_eq!(transfers.show_when_present(3, &app), None);
    assert_eq!(transfers.arrived_to_show(&app), None);
    discover(&mut app, 2, &[1, 2]);
    assert_eq!(transfers.arrived_to_show(&app), None);
    discover(&mut app, 3, &[1, 2, 3]);
    assert_eq!(transfers.arrived_to_show(&app), Some(3));
    assert_eq!(transfers.arrived_to_show(&app), None);
    assert_eq!(transfers.show_when_present(2, &app), Some(2));
}

#[test]
fn detached_record_uses_the_last_observed_remote_identity() {
    let mut transfers = Transfers::default();
    let mut app = WindowApp::default();
    discover(&mut app, 1, &[1, 2]);
    transfers.observe_remotes(HashMap::from([(
        1,
        RemoteTab {
            domain: 7,
            remote: 81,
        },
    )]));
    assert!(transfers.detached_record(1, &app, 5, |_| false).is_none());
    assert!(transfers.detached_record(2, &app, 5, |_| true).is_none());
    let record = transfers
        .detached_record(1, &app, 5, |domain| domain == 7)
        .unwrap();
    assert_eq!(record.tab.id, 1);
    assert_eq!(record.remote, 81);
    assert_eq!(record.domain, 7);
    transfers.observe_remotes(HashMap::new());
    assert!(transfers.detached_record(1, &app, 5, |_| true).is_none());
}

#[test]
fn adapter_spawn_completion_keeps_reserved_folder_without_stealing_new_selection() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9950);
    discover(&mut adapter.app, 1, &[1]);
    adapter
        .app
        .dispatch(core::Intent::CreateFolder {
            name: "Tools".into(),
        })
        .unwrap();
    let folder = adapter.app.model().folders[0].id.clone();
    adapter.command(app::Command::Host(core::HostCommand::Spawn {
        space_id: "home".into(),
        folder_id: Some(folder.clone()),
        launch: core::LaunchSpec::default(),
    }));
    let command = config::keyassignment::SpawnCommand {
        domain: config::keyassignment::SpawnTabDomain::DomainName("local".into()),
        ..Default::default()
    };
    let context = adapter.reserve_spawn(false, &command);
    adapter
        .app
        .dispatch(core::Intent::CreateSpace {
            name: "Later".into(),
        })
        .unwrap();
    let selected = adapter.app.model().selected_space.clone();
    discover(&mut adapter.app, 2, &[1, 2]);
    adapter.message(Message::Spawned { tab: 2, context });
    assert_eq!(adapter.app.model().tabs[&2].folder_id, Some(folder));
    assert_eq!(adapter.app.model().tabs[&2].space_id, "home");
    assert_eq!(adapter.app.model().selected_space, selected);
    assert_eq!(adapter.app.model().selected_tab, None);
}

#[test]
fn adapter_failed_spawn_rejects_a_late_completion() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9951);
    discover(&mut adapter.app, 1, &[1]);
    adapter
        .app
        .dispatch(core::Intent::CreateFolder {
            name: "Tools".into(),
        })
        .unwrap();
    let folder = adapter.app.model().folders[0].id.clone();
    adapter.command(app::Command::Host(core::HostCommand::Spawn {
        space_id: "home".into(),
        folder_id: Some(folder),
        launch: core::LaunchSpec::default(),
    }));
    let command = config::keyassignment::SpawnCommand {
        domain: config::keyassignment::SpawnTabDomain::DomainName("local".into()),
        ..Default::default()
    };
    let context = adapter.reserve_spawn(false, &command);
    adapter.message(Message::SpawnFailed(context.clone()));
    discover(&mut adapter.app, 2, &[1, 2]);
    adapter.message(Message::Spawned { tab: 2, context });
    assert_eq!(adapter.app.model().tabs[&2].folder_id, None);
    assert!(!adapter.app.model().tabs[&2].pinned);
}

use super::*;
use std::collections::BTreeMap;

fn host_tab(id: TabId) -> HostTab {
    HostTab {
        id,
        title: format!("Tab {id}"),
        ..HostTab::default()
    }
}

fn model() -> Model {
    let mut model = Model::default();
    model
        .reconcile(vec![host_tab(1), host_tab(2)], Some(1), true)
        .unwrap();
    model
}

#[test]
fn changed_metadata_updates_display_without_rebuilding_projection() {
    let mut model = model();
    let revision = model.revision;
    let projection = model.projection_revision();
    let changed = HostTab {
        id: 2,
        title: "Build output".into(),
        icon: "!".into(),
        cwd: "/work/project".into(),
        domain: "ssh".into(),
        host: "builder".into(),
        user: "developer".into(),
        process: "cargo".into(),
        remote: true,
        unread: true,
        bell: true,
        ..HostTab::default()
    };
    assert!(
        model
            .reconcile(vec![host_tab(1), changed.clone()], Some(1), false)
            .unwrap()
    );
    assert_eq!(model.revision, revision.wrapping_add(1));
    assert_eq!(model.projection_revision(), projection);
    assert_eq!(model.visible_ids(), &[1, 2]);
    assert_eq!(
        model.tabs[&2],
        Tab {
            space_id: DEFAULT_SPACE.into(),
            ..changed.into()
        }
    );
}

#[test]
fn updates_preserve_application_membership_titles_and_launch() {
    let mut model = model();
    model
        .dispatch(Intent::CreateFolder {
            name: "Pinned".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    model
        .dispatch(Intent::AssignFolder {
            tab_id: 2,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    model
        .dispatch(Intent::RenameTab {
            id: 2,
            title: "User title".into(),
        })
        .unwrap();
    model.apply_title_hook(2, "Hook title".into()).unwrap();
    let launch = LaunchSpec {
        args: vec!["ssh".into(), "server".into()],
        env: BTreeMap::from([("PROJECT".into(), "vertical-tabs".into())]),
        ..LaunchSpec::default()
    };
    model.set_launch(2, launch.clone()).unwrap();
    let mut incoming = host_tab(2);
    incoming.launch = Some(LaunchSpec {
        cwd: Some("/new/host_tab/cwd".into()),
        ..LaunchSpec::default()
    });
    assert!(
        !model
            .reconcile(vec![host_tab(1), incoming.clone()], Some(1), false)
            .unwrap()
    );
    incoming.title = "Changed host title".into();
    assert!(
        model
            .reconcile(vec![host_tab(1), incoming], Some(1), false)
            .unwrap()
    );
    let tab = &model.tabs[&2];
    assert_eq!(tab.space_id, DEFAULT_SPACE);
    assert_eq!(tab.folder_id.as_deref(), Some(folder.as_str()));
    assert!(tab.manual_assignment);
    assert!(tab.pinned);
    assert_eq!(tab.title, "Changed host title");
    assert_eq!(tab.title_override.as_deref(), Some("User title"));
    assert_eq!(tab.title_hook.as_deref(), Some("Hook title"));
    assert_eq!(tab.launch.as_ref(), Some(&launch));
}

#[test]
fn launch_is_adopted_only_until_application_has_one() {
    let mut model = model();
    let mut tab = host_tab(2);
    let launch = LaunchSpec {
        domain: Some("ssh".into()),
        cwd: Some("/work".into()),
        ..LaunchSpec::default()
    };
    tab.launch = Some(launch.clone());
    let projection = model.projection_revision();
    assert!(
        model
            .reconcile(vec![host_tab(1), tab], Some(1), false)
            .unwrap()
    );
    assert_eq!(model.tabs[&2].launch.as_ref(), Some(&launch));
    assert_eq!(model.projection_revision(), projection);
    assert!(
        !model
            .reconcile(vec![host_tab(1), host_tab(2)], Some(1), false)
            .unwrap()
    );
    assert_eq!(model.tabs[&2].launch.as_ref(), Some(&launch));
}

#[test]
fn metadata_routing_changes_projection_and_cleans_old_folder_membership() {
    let mut model = model();
    let mut work = Space::new("work", "Work");
    work.rules.push(RoutingRule {
        remote: None,
        fields: vec![(MatchField::Title, vec!["Work*".into()])],
    });
    model
        .load_catalog(vec![Space::new(DEFAULT_SPACE, "Home"), work], Vec::new())
        .unwrap();
    model
        .dispatch(Intent::CreateFolder {
            name: "Pinned".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    model
        .restore_tab_membership(2, DEFAULT_SPACE, false, true, Some(&folder))
        .unwrap();
    let projection = model.projection_revision();
    let mut tab = host_tab(2);
    tab.title = "Work project".into();
    assert!(
        model
            .reconcile(vec![host_tab(1), tab], Some(1), false)
            .unwrap()
    );
    assert_ne!(model.projection_revision(), projection);
    assert_eq!(model.visible_ids(), &[1]);
    assert_eq!(model.tabs[&2].space_id, "work");
    assert_eq!(model.tabs[&2].folder_id, None);
    assert!(model.tabs[&2].pinned);
}

#[test]
fn metadata_change_invalidates_route_hook_before_rerouting() {
    let mut model = model();
    let mut work = Space::new("work", "Work");
    work.rules.push(RoutingRule {
        remote: Some(true),
        fields: Vec::new(),
    });
    model
        .load_catalog(vec![Space::new(DEFAULT_SPACE, "Home"), work], Vec::new())
        .unwrap();
    model
        .apply_route_hook(2, Some(DEFAULT_SPACE.into()))
        .unwrap();
    let mut tab = host_tab(2);
    tab.remote = true;
    model
        .reconcile(vec![host_tab(1), tab], Some(1), false)
        .unwrap();
    assert_eq!(model.tabs[&2].space_id, "work");
    assert_eq!(model.visible_ids(), &[1]);
}

#[test]
fn duplicate_identity_is_rejected_before_any_mutation() {
    let mut model = model();
    let revision = model.revision;
    let projection = model.projection_revision();
    let tabs = model.tabs.clone();
    let mut changed = host_tab(1);
    changed.title = "Do not publish".into();
    assert!(
        model
            .reconcile(vec![changed, host_tab(1)], Some(1), true)
            .is_err()
    );
    assert_eq!(model.revision, revision);
    assert_eq!(model.projection_revision(), projection);
    assert_eq!(model.tabs, tabs);
    assert_eq!(model.tab_order(), &[1, 2]);
    assert_eq!(model.visible_ids(), &[1, 2]);
}

#[test]
fn new_host_tab_uses_selected_space_and_application_defaults() {
    let mut model = Model::default();
    model
        .dispatch(Intent::CreateSpace {
            name: "Work".into(),
        })
        .unwrap();
    let facts = HostTab {
        id: 7,
        title: "Host title".into(),
        icon: "!".into(),
        cwd: "/work/project".into(),
        repo_root: Some("/work".into()),
        home: Some("/home/user".into()),
        domain: "ssh:builder".into(),
        host: "builder".into(),
        user: "developer".into(),
        process: "cargo".into(),
        remote: true,
        os: "arch".into(),
        panes: vec![TabPane {
            id: 17,
            title: "Pane".into(),
            ..TabPane::default()
        }],
        unread: true,
        bell: true,
        launch: Some(LaunchSpec {
            domain: Some("ssh:builder".into()),
            ..LaunchSpec::default()
        }),
    };
    model.reconcile(vec![facts.clone()], None, false).unwrap();

    let tab = &model.tabs[&7];
    assert_eq!(tab.id, facts.id);
    assert_eq!(tab.title, facts.title);
    assert_eq!(tab.icon, facts.icon);
    assert_eq!(tab.cwd, facts.cwd);
    assert_eq!(tab.repo_root, facts.repo_root);
    assert_eq!(tab.home, facts.home);
    assert_eq!(tab.domain, facts.domain);
    assert_eq!(tab.host, facts.host);
    assert_eq!(tab.user, facts.user);
    assert_eq!(tab.process, facts.process);
    assert_eq!(tab.remote, facts.remote);
    assert_eq!(tab.os, facts.os);
    assert_eq!(tab.panes, facts.panes);
    assert_eq!(tab.launch, facts.launch);
    assert_eq!(tab.space_id, model.selected_space);
    assert!(!tab.pinned);
    assert!(!tab.manual_assignment);
    assert_eq!(tab.folder_id, None);
    assert_eq!(tab.title_override, None);
    assert_eq!(tab.title_hook, None);
    // Selecting the first discovered tab acknowledges its activity.
    assert!(!tab.unread);
    assert!(!tab.bell);
    assert_eq!(model.selected_tab, Some(7));
    assert_eq!(model.visible_ids(), &[7]);
}

#[test]
fn transferred_automatic_tab_routes_and_acknowledges_activity_on_activation() {
    let mut model = Model::default();
    let mut work = Space::new("work", "Work");
    work.rules.push(RoutingRule {
        remote: Some(true),
        fields: Vec::new(),
    });
    model
        .load_catalog(vec![Space::new(DEFAULT_SPACE, "Home"), work], Vec::new())
        .unwrap();
    let tab = Tab {
        id: 7,
        remote: true,
        unread: true,
        bell: true,
        pinned: true,
        title_override: Some("Application title".into()),
        title_hook: Some("Hook title".into()),
        ..Tab::default()
    };
    model.import_transferred_tab(tab).unwrap();
    assert_eq!(model.selected_space, "work");
    assert_eq!(model.selected_tab, Some(7));
    assert_eq!(model.tab_order(), &[7]);
    assert_eq!(model.visible_ids(), &[7]);
    let tab = &model.tabs[&7];
    assert_eq!(tab.space_id, "work");
    assert!(!tab.manual_assignment);
    assert!(tab.pinned);
    assert!(!tab.unread);
    assert!(!tab.bell);
    assert_eq!(tab.title_override.as_deref(), Some("Application title"));
    assert_eq!(tab.title_hook.as_deref(), Some("Hook title"));

    let revision = model.revision;
    let tabs = model.tabs.clone();
    assert!(
        model
            .import_transferred_tab(Tab {
                id: 8,
                ..Tab::default()
            })
            .is_err()
    );
    assert_eq!(model.revision, revision);
    assert_eq!(model.tabs, tabs);
    assert_eq!(model.tab_order(), &[7]);
}

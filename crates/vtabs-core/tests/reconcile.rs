use std::collections::BTreeMap;
use vtabs_core::*;

fn native(id: TabId) -> Tab {
    Tab {
        id,
        title: format!("Tab {id}"),
        ..Tab::default()
    }
}

fn model() -> Model {
    let mut model = Model::default();
    model
        .reconcile(vec![native(1), native(2)], Some(1), true)
        .unwrap();
    model
}

#[test]
fn changed_native_metadata_updates_display_without_rebuilding_projection() {
    let mut model = model();
    let revision = model.revision;
    let projection = model.projection_revision();
    let changed = Tab {
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
        ..Tab::default()
    };
    assert!(
        model
            .reconcile(vec![native(1), changed.clone()], Some(1), false)
            .unwrap()
    );
    assert_eq!(model.revision, revision.wrapping_add(1));
    assert_eq!(model.projection_revision(), projection);
    assert_eq!(model.visible_ids(), &[1, 2]);
    assert_eq!(
        model.tabs[&2],
        Tab {
            space_id: DEFAULT_SPACE.into(),
            ..changed
        }
    );
}

#[test]
fn native_updates_preserve_application_membership_titles_and_launch() {
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
    let mut incoming = native(2);
    incoming.space_id = "ignored-host-space".into();
    incoming.folder_id = Some("ignored-host-folder".into());
    incoming.title_override = Some("ignored-host-title".into());
    incoming.title_hook = Some("ignored-host-hook".into());
    incoming.launch = Some(LaunchSpec {
        cwd: Some("/new/native/cwd".into()),
        ..LaunchSpec::default()
    });
    assert!(
        !model
            .reconcile(vec![native(1), incoming.clone()], Some(1), false)
            .unwrap()
    );
    incoming.title = "Changed host title".into();
    assert!(
        model
            .reconcile(vec![native(1), incoming], Some(1), false)
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
fn native_launch_is_adopted_only_until_application_has_one() {
    let mut model = model();
    let mut tab = native(2);
    let launch = LaunchSpec {
        domain: Some("ssh".into()),
        cwd: Some("/work".into()),
        ..LaunchSpec::default()
    };
    tab.launch = Some(launch.clone());
    let projection = model.projection_revision();
    assert!(
        model
            .reconcile(vec![native(1), tab], Some(1), false)
            .unwrap()
    );
    assert_eq!(model.tabs[&2].launch.as_ref(), Some(&launch));
    assert_eq!(model.projection_revision(), projection);
    assert!(
        !model
            .reconcile(vec![native(1), native(2)], Some(1), false)
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
    let mut tab = native(2);
    tab.title = "Work project".into();
    assert!(
        model
            .reconcile(vec![native(1), tab], Some(1), false)
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
    let mut tab = native(2);
    tab.remote = true;
    model
        .reconcile(vec![native(1), tab], Some(1), false)
        .unwrap();
    assert_eq!(model.tabs[&2].space_id, "work");
    assert_eq!(model.visible_ids(), &[1]);
}

#[test]
fn duplicate_native_identity_is_rejected_before_any_mutation() {
    let mut model = model();
    let revision = model.revision;
    let projection = model.projection_revision();
    let tabs = model.tabs.clone();
    let mut changed = native(1);
    changed.title = "Do not publish".into();
    assert!(
        model
            .reconcile(vec![changed, native(1)], Some(1), true)
            .is_err()
    );
    assert_eq!(model.revision, revision);
    assert_eq!(model.projection_revision(), projection);
    assert_eq!(model.tabs, tabs);
    assert_eq!(model.tab_order(), &[1, 2]);
    assert_eq!(model.visible_ids(), &[1, 2]);
}

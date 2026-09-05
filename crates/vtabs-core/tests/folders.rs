use serde_json::json;
use vtabs_core::*;

fn tab(id: TabId) -> Tab {
    Tab {
        id,
        title: format!("Terminal {id}"),
        ..Tab::default()
    }
}

fn model() -> Model {
    let mut model = Model::default();
    model
        .load_catalog(
            vec![Space::new("home", "Home"), Space::new("work", "Work")],
            vec![],
        )
        .unwrap();
    model
        .reconcile((1..=6).map(tab).collect(), Some(1), true)
        .unwrap();
    model
}

fn create(model: &mut Model, name: &str) -> String {
    model
        .dispatch(Intent::CreateFolder { name: name.into() })
        .unwrap();
    model.folders.last().unwrap().id.clone()
}

fn assign(model: &mut Model, tab_id: TabId, folder_id: &str) {
    model
        .dispatch(Intent::AssignFolder {
            tab_id,
            folder_id: Some(folder_id.into()),
        })
        .unwrap();
}

#[test]
fn folders_partition_pinned_and_normal_tabs_in_catalog_order() {
    let mut model = model();
    let a = create(&mut model, "Applications");
    let b = create(&mut model, "Services");
    assign(&mut model, 6, &a);
    assign(&mut model, 2, &a);
    assign(&mut model, 3, &b);
    model
        .dispatch(Intent::PinTab {
            id: 4,
            pinned: true,
        })
        .unwrap();
    assert_eq!(model.visible_ids(), &[4, 2, 6, 3, 1, 5]);
    assert_eq!(
        model.folder_tabs(&a).map(|tab| tab.id).collect::<Vec<_>>(),
        [2, 6]
    );
    assert!(model.tabs[&6].pinned && model.tabs[&6].manual_assignment);
    model
        .dispatch(Intent::MoveFolder { id: b, index: 0 })
        .unwrap();
    assert_eq!(model.visible_ids(), &[4, 3, 2, 6, 1, 5]);
    model
        .validate_projection(model.visible_ids(), model.selected_tab)
        .unwrap();
}

#[test]
fn collapsed_folders_keep_native_selection_and_keyboard_navigation() {
    let mut model = model();
    let folder = create(&mut model, "Builds");
    assign(&mut model, 1, &folder);
    assign(&mut model, 2, &folder);
    let revision = model.projection_revision();
    let transition = model
        .dispatch(Intent::ToggleFolder(folder.clone()))
        .unwrap();
    assert!(transition.commands.is_empty());
    assert!(transition.durable_changed);
    assert_eq!(model.projection_revision(), revision);
    assert_eq!(model.selected_tab, Some(1));
    assert!(model.folders[0].collapsed);
    let activation = model
        .dispatch(Intent::ActivateRelative {
            delta: 1,
            wrap: false,
        })
        .unwrap();
    assert_eq!(model.selected_tab, Some(2));
    assert!(!model.folders[0].collapsed);
    assert!(activation.durable_changed);
    assert_eq!(model.projection_revision(), revision);
    model.dispatch(Intent::ToggleFolder(folder)).unwrap();
    assert!(model.folders[0].collapsed);
}

#[test]
fn deleting_folder_preserves_every_live_tab_and_pin() {
    let mut model = model();
    let folder = create(&mut model, "Builds");
    assign(&mut model, 1, &folder);
    assign(&mut model, 5, &folder);
    let transition = model.dispatch(Intent::DeleteFolder(folder)).unwrap();
    assert!(transition.commands.is_empty());
    assert!(model.folders.is_empty());
    assert_eq!(model.tabs.len(), 6);
    assert!(model.tabs[&1].pinned && model.tabs[&5].pinned);
    assert!(model.tabs.values().all(|tab| tab.folder_id.is_none()));
    assert_eq!(model.selected_tab, Some(1));
}

#[test]
fn native_metadata_refresh_preserves_membership_and_ignores_closed_ids() {
    let mut model = model();
    let folder = create(&mut model, "Remote");
    assign(&mut model, 2, &folder);
    model
        .reconcile((1..=6).map(tab).collect(), Some(1), false)
        .unwrap();
    assert_eq!(model.tabs[&2].folder_id.as_deref(), Some(folder.as_str()));
    model
        .reconcile(vec![tab(1), tab(3)], Some(1), false)
        .unwrap();
    assert_eq!(model.folder_tabs(&folder).count(), 0);
    model
        .reconcile(vec![tab(1), tab(2), tab(3)], Some(1), false)
        .unwrap();
    assert!(model.tabs[&2].folder_id.is_none());
}

#[test]
fn assigning_across_spaces_and_unpinning_clear_membership() {
    let mut model = model();
    let home = create(&mut model, "Home tools");
    assign(&mut model, 1, &home);
    model
        .dispatch(Intent::AssignTab {
            id: 1,
            space_id: "work".into(),
        })
        .unwrap();
    assert!(model.tabs[&1].folder_id.is_none());
    assert_eq!(model.selected_tab, Some(2));
    model.dispatch(Intent::SelectSpace("work".into())).unwrap();
    let work = create(&mut model, "Work tools");
    assign(&mut model, 2, &work);
    assert_eq!(model.tabs[&2].space_id, "work");
    assert!(!model.visible_ids().contains(&3));
    model
        .dispatch(Intent::PinTab {
            id: 2,
            pinned: false,
        })
        .unwrap();
    assert!(model.tabs[&2].folder_id.is_none());
    assert!(!model.tabs[&2].pinned);
    assign(&mut model, 2, &work);
    model.dispatch(Intent::ReturnToAuto(2)).unwrap();
    assert!(model.tabs[&2].folder_id.is_none());
}

#[test]
fn deleting_space_removes_its_folders_and_ungroups_moved_tabs() {
    let mut model = model();
    let folder = create(&mut model, "Builds");
    assign(&mut model, 1, &folder);
    model
        .dispatch(Intent::DeleteSpace {
            id: "home".into(),
            destination: Some("work".into()),
        })
        .unwrap();
    assert!(model.folders.is_empty());
    assert_eq!(model.tabs.len(), 6);
    assert!(
        model
            .tabs
            .values()
            .all(|tab| tab.folder_id.is_none() && tab.space_id == "work")
    );
    model
        .validate_projection(model.visible_ids(), model.selected_tab)
        .unwrap();
}

#[test]
fn reordering_inside_folders_preserves_other_spaces_and_group_boundaries() {
    let mut model = model();
    let a = create(&mut model, "A");
    let b = create(&mut model, "B");
    assign(&mut model, 1, &a);
    assign(&mut model, 5, &a);
    assign(&mut model, 3, &b);
    model
        .dispatch(Intent::AssignTab {
            id: 2,
            space_id: "work".into(),
        })
        .unwrap();
    let hidden_position = model.tab_order().iter().position(|id| *id == 2).unwrap();
    let transition = model.dispatch(Intent::MoveTab { id: 5, index: 0 }).unwrap();
    assert_eq!(model.visible_ids(), &[5, 1, 3, 4, 6]);
    assert!(
        matches!(&transition.commands[0], HostCommand::Reorder { visible_order } if visible_order == model.visible_ids())
    );
    model
        .dispatch(Intent::MoveTab {
            id: 5,
            index: usize::MAX,
        })
        .unwrap();
    assert_eq!(model.visible_ids(), &[1, 5, 3, 4, 6]);
    assert_eq!(model.tab_order()[hidden_position], 2);
    model.dispatch(Intent::MoveTab { id: 3, index: 0 }).unwrap();
    assert_eq!(model.visible_ids(), &[1, 5, 3, 4, 6]);
}

#[test]
fn moving_folders_uses_space_local_indices() {
    let mut model = model();
    let a = create(&mut model, "A");
    model.dispatch(Intent::SelectSpace("work".into())).unwrap();
    let work = create(&mut model, "Work");
    model.dispatch(Intent::SelectSpace("home".into())).unwrap();
    let b = create(&mut model, "B");
    model
        .dispatch(Intent::MoveFolder {
            id: b.clone(),
            index: 0,
        })
        .unwrap();
    assert_eq!(
        model
            .folders
            .iter()
            .map(|folder| folder.id.as_str())
            .collect::<Vec<_>>(),
        [&b, &work, &a]
    );
    assert_eq!(model.selected_folders().count(), 2);
}

#[test]
fn invalid_folder_operations_and_catalogs_are_atomic() {
    let mut model = model();
    let folder = create(&mut model, "Original");
    assign(&mut model, 1, &folder);
    let before = model.folders.clone();
    let revision = model.revision;
    for intent in [
        Intent::CreateFolder {
            name: " \n ".into(),
        },
        Intent::RenameFolder {
            id: folder.clone(),
            name: "\0".into(),
        },
        Intent::AssignFolder {
            tab_id: 1,
            folder_id: Some("missing".into()),
        },
        Intent::AssignFolder {
            tab_id: 99,
            folder_id: Some(folder.clone()),
        },
        Intent::MoveFolder {
            id: "missing".into(),
            index: 0,
        },
    ] {
        assert!(model.dispatch(intent).is_err());
        assert_eq!(model.revision, revision);
        assert_eq!(model.folders, before);
    }
    let mut bad = before.clone();
    bad[0].space_id = "missing".into();
    assert!(model.load_folders(bad).is_err());
    assert!(
        model
            .load_folders(vec![before[0].clone(), before[0].clone()])
            .is_err()
    );
    assert_eq!(model.folders, before);
    assert_eq!(model.tabs[&1].folder_id, Some(folder));
}

#[test]
fn restoring_old_or_stale_membership_never_creates_orphan_folders() {
    let mut model = model();
    let folder = create(&mut model, "Home tools");
    model
        .restore_tab_membership(1, "work", true, true, Some(&folder))
        .unwrap();
    assert!(model.tabs[&1].folder_id.is_none());
    model
        .restore_tab_membership(2, "home", true, false, Some(&folder))
        .unwrap();
    assert!(model.tabs[&2].pinned);
    model
        .load_catalog(vec![Space::new("work", "Work")], vec![])
        .unwrap();
    assert!(model.folders.is_empty());
    assert!(model.tabs.values().all(|tab| tab.folder_id.is_none()));
    let old: Tab = serde_json::from_value(json!({"id":99,"title":"Legacy"})).unwrap();
    assert!(old.folder_id.is_none());
}

#[test]
fn new_tabs_inherit_directory_only_within_the_same_domain() {
    let mut model = model();
    let mut remote = tab(1);
    remote.domain = "ssh:server".into();
    remote.cwd = "/srv/project".into();
    model.reconcile(vec![remote], Some(1), false).unwrap();
    assert_eq!(model.default_launch().cwd.as_deref(), Some("/srv/project"));
    model
        .dispatch(Intent::SetSetting {
            key: "default_domain".into(),
            value: json!("local"),
        })
        .unwrap();
    let launch = model.default_launch();
    assert_eq!(launch.domain.as_deref(), Some("local"));
    assert!(launch.cwd.is_none());
    model
        .dispatch(Intent::SetSetting {
            key: "default_domain".into(),
            value: json!("ssh:server"),
        })
        .unwrap();
    assert_eq!(model.default_launch().cwd.as_deref(), Some("/srv/project"));
}

#[test]
fn folder_spawn_command_captures_folder_and_space_without_mutating_selection() {
    let mut model = model();
    let folder = create(&mut model, "Tools");
    model.dispatch(Intent::SelectSpace("work".into())).unwrap();
    let transition = model
        .dispatch(Intent::NewTabInFolder(folder.clone()))
        .unwrap();
    assert!(
        matches!(&transition.commands[..], [HostCommand::Spawn { space_id, folder_id: Some(id), .. }] if space_id == "home" && id == &folder)
    );
    assert_eq!(model.selected_space, "work");
    assert!(!transition.model_changed);
    assert!(
        model
            .dispatch(Intent::NewTabInFolder("missing".into()))
            .is_err()
    );
}

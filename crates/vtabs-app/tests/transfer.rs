use serde_json::json;
use std::{collections::BTreeMap, time::Duration};
use vtabs_app::{core::*, store::*, *};

fn snapshot(id: u64) -> NativeSnapshot {
    NativeSnapshot {
        revision: 1,
        tabs: vec![Tab {
            id,
            title: "Host title".into(),
            ..Tab::default()
        }],
        active_tab: Some(id),
        metrics: Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    }
}

#[test]
fn window_transfer_preserves_private_profile_catalog_flags_and_launch() {
    let mut source = WindowApp::new("work-profile", true);
    source.update(snapshot(1)).unwrap();
    source
        .dispatch(Intent::CreateSpace {
            name: "Remote work".into(),
        })
        .unwrap();
    let space = source.model().selected_space.clone();
    source
        .dispatch(Intent::AssignTab {
            id: 1,
            space_id: space.clone(),
        })
        .unwrap();
    source
        .dispatch(Intent::PinTab {
            id: 1,
            pinned: true,
        })
        .unwrap();
    source
        .dispatch(Intent::CreateFolder {
            name: "Remote tools".into(),
        })
        .unwrap();
    let folder = source.model().folders[0].id.clone();
    source
        .dispatch(Intent::AssignFolder {
            tab_id: 1,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    source
        .dispatch(Intent::ToggleFolder(folder.clone()))
        .unwrap();
    source
        .dispatch(Intent::RenameTab {
            id: 1,
            title: "Explicit title".into(),
        })
        .unwrap();
    source
        .apply_config(BTreeMap::from([("width".into(), json!(310))]), 0)
        .unwrap();
    let launch = LaunchSpec {
        args: vec!["fish".into(), "--login".into()],
        ..LaunchSpec::default()
    };
    source.set_launch(1, launch.clone()).unwrap();
    let mut transfer = source.export_transfer(1).unwrap();
    transfer.tab.id = 77;
    let mut destination = WindowApp::default();
    destination
        .import_transfer_before_snapshot(transfer)
        .unwrap();
    destination.update(snapshot(77)).unwrap();
    assert_eq!(destination.model().profile, "work-profile");
    assert!(destination.model().private);
    assert_eq!(destination.model().selected_space, space);
    assert_eq!(destination.model().selected_tab, Some(77));
    assert_eq!(destination.model().settings.width, 310);
    let tab = &destination.model().tabs[&77];
    assert!(tab.pinned);
    assert!(tab.manual_assignment);
    assert_eq!(tab.folder_id.as_deref(), Some(folder.as_str()));
    assert_eq!(destination.model().folders[0].name, "Remote tools");
    assert!(destination.model().folders[0].collapsed);
    assert_eq!(tab.display_title(), "Explicit title");
    assert_eq!(tab.launch, Some(launch));
    let request = destination
        .take_storage_request(Duration::from_secs(1))
        .unwrap();
    assert!(request.operations.iter().all(|op| matches!(
        op,
        Operation::Read {
            scope: Scope::Profile { .. }
        }
    )));
}
#[test]
fn import_refuses_replacing_a_live_windows_application() {
    let mut source = WindowApp::default();
    source.update(snapshot(1)).unwrap();
    let transfer = source.export_transfer(1).unwrap();
    assert!(source.import_transfer_before_snapshot(transfer).is_err());
    assert_eq!(source.model().selected_tab, Some(1));
}

#[test]
fn old_window_transfer_without_folders_remains_compatible() {
    let mut source = WindowApp::default();
    source.update(snapshot(1)).unwrap();
    let mut value = serde_json::to_value(source.export_transfer(1).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("folders");
    value["tab"].as_object_mut().unwrap().remove("folder_id");
    let transfer = serde_json::from_value(value).unwrap();
    let mut destination = WindowApp::default();
    destination
        .import_transfer_before_snapshot(transfer)
        .unwrap();
    assert!(destination.model().folders.is_empty());
    assert!(destination.model().tabs[&1].folder_id.is_none());
}

#[test]
fn malformed_transferred_folder_catalog_does_not_replace_destination() {
    let mut source = WindowApp::default();
    source.update(snapshot(1)).unwrap();
    source
        .dispatch(Intent::CreateFolder {
            name: "Tools".into(),
        })
        .unwrap();
    let mut transfer = source.export_transfer(1).unwrap();
    transfer.folders[0].space_id = "missing".into();
    let mut destination = WindowApp::new("other", true);
    assert!(
        destination
            .import_transfer_before_snapshot(transfer)
            .is_err()
    );
    assert_eq!(destination.model().profile, "other");
    assert!(destination.model().private);
    assert!(destination.model().tabs.is_empty());
}

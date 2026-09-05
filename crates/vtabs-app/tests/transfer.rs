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

use serde_json::json;
use std::collections::BTreeMap;
use vtabs_core::*;

fn tab(id: u64) -> Tab {
    Tab {
        id,
        title: format!("tab {id}"),
        ..Tab::default()
    }
}
fn model() -> Model {
    let mut model = Model::default();
    model
        .load_catalog(
            vec![
                Space::new("home", "Home"),
                Space::new("work", "Work"),
                Space::new("empty", "Empty"),
            ],
            vec![],
        )
        .unwrap();
    model
        .reconcile(vec![tab(1), tab(2), tab(3)], Some(1), true)
        .unwrap();
    model
        .dispatch(Intent::AssignTab {
            id: 3,
            space_id: "work".into(),
        })
        .unwrap();
    model
}

#[test]
fn empty_space_stays_empty_through_unchanged_native_active() {
    let mut m = model();
    m.dispatch(Intent::SelectSpace("empty".into())).unwrap();
    assert_eq!(m.selected_tab, None);
    m.reconcile(vec![tab(1), tab(2), tab(3)], Some(1), false)
        .unwrap();
    assert_eq!(m.selected_space, "empty");
    assert!(m.visible_ids().is_empty());
    assert_eq!(m.selected_tab, None);
}
#[test]
fn external_id_activation_follows_space() {
    let mut m = model();
    m.reconcile(vec![tab(1), tab(2), tab(3)], Some(3), true)
        .unwrap();
    assert_eq!(m.selected_space, "work");
    assert_eq!(m.visible_ids(), &[3]);
    assert_eq!(m.selected_tab, Some(3));
}
#[test]
fn closing_visible_final_tab_keeps_empty_space_even_if_mux_selects_hidden() {
    let mut m = model();
    m.dispatch(Intent::SelectSpace("work".into())).unwrap();
    m.reconcile(vec![tab(1), tab(2)], Some(1), true).unwrap();
    assert_eq!(m.selected_space, "work");
    assert_eq!(m.selected_tab, None);
}
#[test]
fn closing_active_uses_visible_neighbor() {
    let mut m = model();
    m.reconcile(vec![tab(2), tab(3)], Some(3), true).unwrap();
    assert_eq!(m.selected_tab, Some(2));
    assert_eq!(m.selected_space, "home");
}
#[test]
fn native_index_negative_relative_and_mru_share_projection() {
    let mut m = model();
    m.dispatch(Intent::ActivateIndex(-1)).unwrap();
    assert_eq!(m.selected_tab, Some(2));
    m.dispatch(Intent::ActivateRelative {
        delta: 1,
        wrap: true,
    })
    .unwrap();
    assert_eq!(m.selected_tab, Some(1));
    m.dispatch(Intent::ActivateLast).unwrap();
    assert_eq!(m.selected_tab, Some(2));
    m.dispatch(Intent::ActivateIndex(20)).unwrap();
    assert_eq!(m.selected_tab, Some(2));
    m.dispatch(Intent::ActivateRelative {
        delta: 1,
        wrap: false,
    })
    .unwrap();
    assert_eq!(m.selected_tab, Some(2));
}
#[test]
fn reorder_preserves_hidden_slots_and_pin_partition() {
    let mut m = model();
    m.reconcile(vec![tab(1), tab(3), tab(2)], Some(1), false)
        .unwrap();
    m.dispatch(Intent::MoveTab { id: 2, index: 0 }).unwrap();
    assert_eq!(m.tab_order(), &[2, 3, 1]);
    m.dispatch(Intent::PinTab {
        id: 1,
        pinned: true,
    })
    .unwrap();
    assert_eq!(m.visible_ids(), &[1, 2]);
    m.dispatch(Intent::MoveTab { id: 2, index: 0 }).unwrap();
    assert_eq!(m.visible_ids(), &[1, 2]);
}
#[test]
fn deletion_requires_explicit_valid_destination_without_partial_mutation() {
    let mut m = model();
    assert!(
        m.dispatch(Intent::DeleteSpace {
            id: "work".into(),
            destination: None
        })
        .is_err()
    );
    assert!(m.spaces.iter().any(|s| s.id == "work"));
    m.dispatch(Intent::DeleteSpace {
        id: "work".into(),
        destination: Some("home".into()),
    })
    .unwrap();
    assert_eq!(m.tabs[&3].space_id, "home");
    assert!(m.tabs[&3].manual_assignment);
}
#[test]
fn stale_target_never_activates_reused_index() {
    let mut m = model();
    m.reconcile(vec![tab(2), tab(3)], Some(2), true).unwrap();
    assert!(m.dispatch(Intent::ActivateTab(1)).is_err());
    assert!(m.validate_projection(&[2, 2], Some(2)).is_err());
    assert!(m.validate_projection(&[2], Some(3)).is_err());
}
#[test]
fn routing_templates_and_manual_return_to_auto() {
    let mut m = model();
    m.load_catalog(
        m.spaces.clone(),
        vec![SpaceTemplate {
            id: "remote-$host".into(),
            name: "Host $host".into(),
            icon: "R".into(),
            accent: None,
            rules: vec![RoutingRule {
                remote: Some(true),
                fields: vec![],
            }],
        }],
    )
    .unwrap();
    let remote = Tab {
        id: 4,
        host: "server".into(),
        remote: true,
        ..tab(4)
    };
    m.reconcile(vec![tab(1), tab(2), tab(3), remote.clone()], Some(1), false)
        .unwrap();
    assert_eq!(m.tabs[&4].space_id, "remote-server");
    assert!(
        m.spaces
            .iter()
            .any(|s| s.id == "remote-server" && s.name == "Host server")
    );
    m.dispatch(Intent::AssignTab {
        id: 4,
        space_id: "home".into(),
    })
    .unwrap();
    m.reconcile(vec![tab(1), tab(2), tab(3), remote], Some(1), false)
        .unwrap();
    assert_eq!(m.tabs[&4].space_id, "home");
    m.dispatch(Intent::ReturnToAuto(4)).unwrap();
    assert_eq!(m.tabs[&4].space_id, "remote-server");
}
#[test]
fn private_windows_do_not_retain_reopen_launch_history() {
    let mut m = Model::new("default", true);
    let t = Tab {
        launch: Some(LaunchSpec::default()),
        ..tab(1)
    };
    m.reconcile(vec![t], Some(1), true).unwrap();
    m.reconcile(vec![], None, true).unwrap();
    assert!(!m.can_reopen());
    assert!(m.dispatch(Intent::Reopen).unwrap().commands.is_empty());
}
#[test]
fn settings_precedence_and_atomic_validation() {
    let mut m = model();
    m.load_preferences(BTreeMap::from([("width".into(), json!(300))]))
        .unwrap();
    m.apply_config(BTreeMap::from([("width".into(), json!(400))]))
        .unwrap();
    assert_eq!(m.settings.width, 400);
    assert!(
        m.dispatch(Intent::SetSetting {
            key: "width".into(),
            value: json!(450)
        })
        .is_err()
    );
    assert!(
        m.apply_config(BTreeMap::from([("width".into(), json!(-1))]))
            .is_err()
    );
    assert_eq!(m.settings.width, 400);
    m.apply_config(BTreeMap::new()).unwrap();
    assert_eq!(m.settings.width, 300);
}
#[test]
fn schema_defaults_validate_every_field() {
    let defaults = Settings::default();
    for d in settings::descriptors() {
        settings::validate_value(d.key, &defaults.get(d.key).unwrap()).unwrap();
    }
}
#[test]
fn metadata_updates_do_not_erase_exact_reopen_launch() {
    let mut m = model();
    let launch = LaunchSpec {
        args: vec!["fish".into(), "--login".into()],
        ..LaunchSpec::default()
    };
    m.set_launch(1, launch.clone()).unwrap();
    m.reconcile(
        vec![
            Tab {
                launch: Some(LaunchSpec {
                    cwd: Some("/newcwd".into()),
                    ..LaunchSpec::default()
                }),
                ..tab(1)
            },
            tab(2),
            tab(3),
        ],
        Some(1),
        false,
    )
    .unwrap();
    m.reconcile(vec![tab(2), tab(3)], Some(2), true).unwrap();
    let out = m.dispatch(Intent::Reopen).unwrap();
    assert!(matches!(&out.commands[0],HostCommand::Spawn{launch:actual,..}if actual==&launch));
}

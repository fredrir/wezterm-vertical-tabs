use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn tab(id: u64) -> HostTab {
    HostTab {
        id,
        title: format!("tab {id}"),
        ..HostTab::default()
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
fn empty_space_stays_empty_through_unchanged_active() {
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
fn closing_active_returns_to_last_used_visible_tab() {
    let mut m = model();
    m.reconcile(vec![tab(1), tab(2), tab(3), tab(4), tab(5)], Some(1), false)
        .unwrap();
    for id in [5, 3, 2] {
        m.dispatch(Intent::ActivateTab(id)).unwrap();
    }
    m.reconcile(vec![tab(1), tab(3), tab(4), tab(5)], Some(1), true)
        .unwrap();
    assert_eq!(m.selected_space, "home");
    assert_eq!(m.selected_tab, Some(5));
}
#[test]
fn index_negative_relative_and_mru_share_projection() {
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
fn close_below_spares_pinned_and_earlier_tabs() {
    let mut m = model();
    m.reconcile((1..=5).map(tab).collect(), Some(1), false)
        .unwrap();
    for id in [2, 5] {
        m.dispatch(Intent::PinTab { id, pinned: true }).unwrap();
    }
    assert_eq!(m.visible_ids(), &[2, 5, 1, 4]);
    let mut close_below = |id| m.dispatch(Intent::CloseBelow(id)).unwrap().commands;
    assert_eq!(
        close_below(2),
        [HostCommand::Close(1), HostCommand::Close(4)]
    );
    assert!(close_below(4).is_empty());
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
    let remote = HostTab {
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
fn settings_precedence_and_atomic_validation() {
    let mut m = model();
    m.load_managed_settings(BTreeMap::from([("width".into(), json!(300))]))
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
fn quick_terminal_sizes_are_window_fractions() {
    let mut settings = Settings::default();
    settings.set("quick_terminal_width", json!(1)).unwrap();
    settings.set("quick_terminal_height", json!(0.5)).unwrap();
    assert_eq!(
        (
            settings.quick_terminal_width,
            settings.quick_terminal_height
        ),
        (1., 0.5)
    );
    for value in [json!(0), json!(1.5), json!(-0.2), json!("0.5"), json!(null)] {
        assert!(settings.set("quick_terminal_width", value).is_err());
    }
}
#[test]
fn schema_defaults_validate_every_field() {
    let defaults = Settings::default();
    for d in settings::descriptors() {
        settings::validate_value(d.key, &defaults.get(d.key).unwrap()).unwrap();
    }
}

#[test]
fn quick_terminal_defaults_preserve_existing_sessions_and_appearance() {
    let settings: Settings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(settings.quick_terminal_opacity, 1.);
    assert_eq!(settings.quick_terminal_blur, 0);
    assert_eq!(settings.quick_terminal_border_color, None);
    assert_eq!(settings.quick_terminal_background_color, None);
    assert!(!settings.quick_terminal_auto_hide);
    assert!(settings.quick_terminal_preserve_shell);
    assert_eq!(
        settings.quick_terminal_hide_on_escape,
        settings::HideOnEscape::Auto
    );
}

#[test]
fn quick_terminal_options_validate_atomically_and_colors_can_follow_the_palette() {
    let mut settings = Settings::default();
    for value in [json!(0), json!(0.4), json!(1)] {
        settings.set("quick_terminal_opacity", value).unwrap();
    }
    for key in [
        "quick_terminal_border_color",
        "quick_terminal_background_color",
    ] {
        settings.set(key, json!("#012aFE")).unwrap();
        settings.set(key, json!(null)).unwrap();
        assert_eq!(settings.get(key), Some(json!(null)));
    }
    settings.set("quick_terminal_blur", json!(64)).unwrap();
    for (value, mode) in [
        ("never", settings::HideOnEscape::Never),
        ("always", settings::HideOnEscape::Always),
        ("auto", settings::HideOnEscape::Auto),
    ] {
        settings
            .set("quick_terminal_hide_on_escape", json!(value))
            .unwrap();
        assert_eq!(settings.quick_terminal_hide_on_escape, mode);
    }
    for key in ["quick_terminal_auto_hide", "quick_terminal_preserve_shell"] {
        settings.set(key, json!(true)).unwrap();
        settings.set(key, json!(false)).unwrap();
        assert!(settings.set(key, json!(1)).is_err());
    }
    for (key, values) in [
        (
            "quick_terminal_opacity",
            vec![json!(-0.1), json!(1.1), json!("0.5"), json!(null)],
        ),
        (
            "quick_terminal_blur",
            vec![json!(-1), json!(65), json!(1.5), json!(true)],
        ),
        (
            "quick_terminal_border_color",
            vec![json!("red"), json!("#12345g"), json!("#12345678")],
        ),
        (
            "quick_terminal_background_color",
            vec![json!(""), json!(42)],
        ),
        (
            "quick_terminal_hide_on_escape",
            vec![json!(false), json!("sometimes"), json!(null)],
        ),
        (
            "quick_terminal_window_padding",
            vec![
                json!(null),
                json!(8),
                json!({"middle": 8}),
                json!({"left": "1em"}),
                json!({"left": -1}),
                json!({"top": "-2px"}),
                json!({"bottom": "cell"}),
                json!({"right": "1cell "}),
            ],
        ),
    ] {
        let before = settings.clone();
        for value in values {
            assert!(settings.set(key, value).is_err());
            assert_eq!(settings, before);
        }
    }
}

#[test]
fn quick_terminal_border_can_be_hidden() {
    let mut settings = Settings::default();
    settings
        .set("quick_terminal_border_color", json!("none"))
        .unwrap();
    assert_eq!(
        settings.quick_terminal_border_color.as_deref(),
        Some("none")
    );
    assert!(
        settings
            .set("quick_terminal_background_color", json!("none"))
            .is_err()
    );
}

#[test]
fn quick_terminal_padding_reads_sides_like_wezterm() {
    use padding::{Dimension, Unit, WindowPadding};
    let mut settings = Settings::default();
    assert_eq!(
        settings.get("quick_terminal_window_padding"),
        Some(json!({}))
    );
    settings
        .set(
            "quick_terminal_window_padding",
            json!({"left": 8, "top": "0.5cell", "right": " 2 px", "bottom": "10%"}),
        )
        .unwrap();
    let padding = &settings.quick_terminal_window_padding;
    let parsed = ["left", "top", "right", "bottom"].map(|side| padding.side(side)?.parse());
    assert_eq!(
        parsed,
        [
            Some((8., Unit::Pixels)),
            Some((0.5, Unit::Cells)),
            Some((2., Unit::Pixels)),
            Some((10., Unit::Percent)),
        ]
    );
    assert_eq!(Dimension::from_text("12"), Ok(Dimension::Number(12.)));
    assert_eq!(
        Dimension::from_text("6pt").unwrap().parse(),
        Some((6., Unit::Points))
    );
    assert!(Dimension::from_text("6 em").is_err());
    for value in [
        json!({"left": true}),
        json!({"left": {}}),
        json!({"left": "2em"}),
    ] {
        let error = serde_json::from_value::<WindowPadding>(value).unwrap_err();
        assert!(
            error.to_string().starts_with("padding takes pixels"),
            "{error}"
        );
    }
    settings
        .set("quick_terminal_window_padding", json!({"top": 0}))
        .unwrap();
    assert_eq!(
        settings.get("quick_terminal_window_padding"),
        Some(json!({"top": 0}))
    );
}

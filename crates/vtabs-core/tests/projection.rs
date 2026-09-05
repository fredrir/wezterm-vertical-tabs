use vtabs_core::*;
fn model() -> Model {
    let mut model = Model::default();
    model
        .reconcile(
            (1..=3)
                .map(|id| Tab {
                    id,
                    title: format!("{id}"),
                    ..Tab::default()
                })
                .collect(),
            Some(1),
            true,
        )
        .unwrap();
    model
}
#[test]
fn same_space_activation_keeps_cached_projection() {
    let mut model = model();
    let revision = model.projection_revision();
    let ptr = model.visible_ids().as_ptr();
    model.dispatch(Intent::ActivateIndex(2)).unwrap();
    assert_eq!(model.projection_revision(), revision);
    assert_eq!(model.visible_ids().as_ptr(), ptr);
    assert_eq!(model.selected_tab, Some(3));
}
#[test]
fn unchanged_native_metadata_does_not_rebuild_projection() {
    let mut model = model();
    let revision = model.projection_revision();
    let metadata = model.tabs.values().cloned().collect();
    assert!(!model.reconcile(metadata, Some(1), false).unwrap());
    assert_eq!(model.projection_revision(), revision);
}
#[test]
fn filtering_active_selects_existing_visible_neighbor() {
    let mut model = model();
    model.apply_filter_hook(1, false).unwrap();
    assert_eq!(model.visible_ids(), &[2, 3]);
    assert_eq!(model.selected_tab, Some(2));
    model
        .validate_projection(model.visible_ids(), model.selected_tab)
        .unwrap();
}
#[test]
fn assigning_into_selected_empty_space_restores_native_selection() {
    let mut model = model();
    model
        .load_catalog(
            vec![Space::new("home", "Home"), Space::new("empty", "Empty")],
            Vec::new(),
        )
        .unwrap();
    model.dispatch(Intent::SelectSpace("empty".into())).unwrap();
    assert_eq!(model.selected_tab, None);
    model
        .dispatch(Intent::AssignTab {
            id: 2,
            space_id: "empty".into(),
        })
        .unwrap();
    assert_eq!(model.visible_ids(), &[2]);
    assert_eq!(model.selected_tab, Some(2));
    model
        .dispatch(Intent::AssignTab {
            id: 3,
            space_id: "empty".into(),
        })
        .unwrap();
    model.dispatch(Intent::ActivateIndex(1)).unwrap();
    assert_eq!(model.selected_tab, Some(3));
    model
        .validate_projection(model.visible_ids(), model.selected_tab)
        .unwrap();
}
#[test]
fn explicit_id_activation_of_filtered_tab_restores_valid_projection() {
    let mut model = model();
    model.apply_filter_hook(1, false).unwrap();
    model.dispatch(Intent::ActivateTab(1)).unwrap();
    assert_eq!(model.selected_tab, Some(1));
    model
        .validate_projection(model.visible_ids(), model.selected_tab)
        .unwrap();
}
#[test]
fn explicit_title_wins_hook_and_reload_removes_only_hook_cache() {
    let mut model = model();
    model.apply_title_hook(1, "Hook title".into()).unwrap();
    assert_eq!(model.tabs[&1].display_title(), "Hook title");
    model
        .dispatch(Intent::RenameTab {
            id: 1,
            title: "Chosen by user".into(),
        })
        .unwrap();
    model.apply_title_hook(1, "New hook".into()).unwrap();
    assert_eq!(model.tabs[&1].display_title(), "Chosen by user");
    model.apply_config(Default::default()).unwrap();
    assert_eq!(model.tabs[&1].display_title(), "Chosen by user");
    assert_eq!(model.tabs[&1].title_hook, None);
}

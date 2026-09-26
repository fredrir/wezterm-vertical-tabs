use crate::*;
use std::time::Duration;
use vtabs_core::{Model, Tab};

#[test]
fn activating_the_selected_space_from_the_keyboard_repaints_its_reset_list() {
    let mut model = Model::default();
    model.settings.animations = false;
    model.settings.cards = false;
    model
        .reconcile(
            (1..=30)
                .map(|id| Tab {
                    id,
                    title: format!("tab {id}"),
                    ..Tab::default()
                })
                .collect(),
            Some(1),
            true,
        )
        .unwrap();
    let area = Rect::new(0, 0, 30, 14);
    let first_tab = |ui: &SidebarUi| {
        ui.hit_regions()
            .iter()
            .find(|hit| matches!(hit.id, ElementId::Tab(_)))
            .map(|hit| hit.id.clone())
    };
    let mut ui = SidebarUi::new();
    ui.render(&model, area, Duration::ZERO);
    ui.event(
        &model,
        UiInput::Scroll {
            x: 4,
            y: 6,
            rows: 8,
        },
    );
    ui.render(&model, area, Duration::ZERO);
    assert_ne!(first_tab(&ui), Some(ElementId::Tab(1)));

    ui.focused = Some(ElementId::Space(model.selected_space.clone()));
    ui.event(&model, UiInput::key(Key::Enter));
    assert!(ui.render(&model, area, Duration::ZERO).is_some());
    assert_eq!(first_tab(&ui), Some(ElementId::Tab(1)));
}

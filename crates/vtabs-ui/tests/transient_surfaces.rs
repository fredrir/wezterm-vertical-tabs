use std::time::Duration;
use vtabs_core::{Model, RailMode, Side, Tab};
use vtabs_ui::{ElementId, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput};

fn model() -> Model {
    let mut model = Model::default();
    model
        .reconcile(
            vec![Tab {
                id: 7,
                title: "Editor".into(),
                ..Tab::default()
            }],
            Some(7),
            true,
        )
        .unwrap();
    model
}

fn assert_centered(rect: Rect, area: Rect) {
    assert_eq!(rect.intersection(area), rect);
    assert!(
        (i32::from(rect.x) * 2 + i32::from(rect.width)
            - i32::from(area.x) * 2
            - i32::from(area.width))
        .abs()
            <= 1
    );
    assert!(
        (i32::from(rect.y) * 2 + i32::from(rect.height)
            - i32::from(area.y) * 2
            - i32::from(area.height))
        .abs()
            <= 1
    );
}

fn dialog_rect(ui: &SidebarUi) -> Rect {
    ui.rounded_surfaces()
        .iter()
        .find(|surface| surface.rect.width == 64)
        .expect("centered dialog surface")
        .rect
}

#[test]
fn launcher_centers_in_window_without_expanding_either_sidebar() {
    let area = Rect::new(3, 5, 100, 32);
    for side in [Side::Left, Side::Right] {
        for (rail, columns) in [
            (RailMode::Expanded, 28),
            (RailMode::Collapsed, 4),
            (RailMode::Hidden, 0),
        ] {
            let mut model = model();
            model.settings.side = side;
            model.settings.rail = rail;
            let tabs = model.visible_ids().to_vec();
            let mut ui = SidebarUi::new();
            ui.set_layout(columns, 0);
            ui.open_tab_navigator(&model);
            assert!(ui.overlay_surface());
            assert!(!ui.content_page());
            assert!(!ui.needs_expanded_space());
            ui.render(&model, area, Duration::ZERO);
            assert_centered(dialog_rect(&ui), area);
            assert_eq!(model.settings.rail, rail);
            assert_eq!(model.visible_ids(), tabs);
            let editor = ui
                .hit_regions()
                .iter()
                .find(|hit| hit.id == ElementId::Editor)
                .unwrap();
            assert!(editor.rect.x > area.x + columns.min(4));
        }
    }
}

#[test]
fn centered_launcher_keeps_ime_and_pointer_coordinates_in_window_space() {
    let model = model();
    let area = Rect::new(3, 5, 100, 32);
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    ui.open_tab_navigator(&model);
    let start = ui
        .render(&model, area, Duration::ZERO)
        .unwrap()
        .cursor
        .unwrap();
    ui.event(&model, UiInput::Text("界x".into()));
    ui.event(
        &model,
        UiInput::ImePreedit {
            text: "é".into(),
            cursor: Some(2),
        },
    );
    let frame = ui.render(&model, area, Duration::from_millis(1)).unwrap();
    let editor = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Editor)
        .unwrap()
        .rect;
    let cursor = frame.cursor.unwrap();
    assert!(editor.contains(cursor));
    assert_eq!(cursor.x, start.x + 4);
    assert_eq!(frame.ime_rect.unwrap(), Rect::new(cursor.x, cursor.y, 1, 1));
    ui.event(
        &model,
        UiInput::PointerDown {
            x: editor.x,
            y: editor.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    let frame = ui.render(&model, area, Duration::from_millis(2)).unwrap();
    assert_eq!(frame.cursor.unwrap().x, start.x);
    assert!(
        ui.event(
            &model,
            UiInput::PointerDown {
                x: area.x,
                y: area.y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }
        )
        .is_empty()
    );
    assert!(!ui.overlay_surface());
    assert!(!ui.is_modal());
}

#[test]
fn edit_dialog_centers_across_window_and_returns_to_settings_page() {
    let model = model();
    let area = Rect::new(3, 5, 100, 32);
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    ui.open_settings();
    ui.render(&model, area, Duration::ZERO);
    ui.open_create_space();
    ui.render(&model, area, Duration::from_millis(1));
    assert!(ui.content_page());
    assert!(ui.overlay_surface());
    assert_centered(dialog_rect(&ui), area);
    assert_eq!(ui.focused(), Some(&ElementId::Editor));
    ui.event(&model, UiInput::key(Key::Escape));
    assert!(ui.content_page());
    assert!(!ui.overlay_surface());
}

#[test]
fn tooltip_centers_in_window_without_stealing_keyboard_or_pointer_targets() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    ui.render(&model, Rect::new(0, 0, 28, 32), Duration::ZERO);
    let hit = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Settings)
        .unwrap()
        .rect;
    ui.event(&model, UiInput::PointerMove { x: hit.x, y: hit.y });
    assert!(ui.overlay_surface());
    assert!(!ui.is_modal());
    assert!(!ui.has_focus());
    let area = Rect::new(0, 0, 100, 32);
    let frame = ui.render(&model, area, Duration::from_millis(601)).unwrap();
    let tooltip = ui.rounded_surfaces().last().unwrap().rect;
    assert_centered(tooltip, area);
    assert!(frame.cursor.is_none());
    assert!(frame.ime_rect.is_none());
    assert!(
        ui.hit_test(
            tooltip.x + tooltip.width / 2,
            tooltip.y + tooltip.height / 2
        )
        .is_none()
    );
    assert!(ui.hit_regions().iter().all(|hit| hit.rect.right() <= 28));
    ui.event(&model, UiInput::PointerMove { x: 99, y: 31 });
    assert!(!ui.overlay_surface());
    assert!(!ui.has_focus());
}

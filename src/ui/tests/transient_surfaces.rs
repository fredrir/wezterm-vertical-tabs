use std::time::Duration;
use vtabs_core::{Intent, Model, RailMode, Side, Tab};
use vtabs_ui::{ElementId, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent};

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

fn hit(ui: &SidebarUi, id: &ElementId) -> Rect {
    ui.hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap_or_else(|| panic!("missing {id:?}"))
        .rect
}

#[test]
fn launcher_drops_down_from_the_search_bar_without_expanding_either_sidebar() {
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
            ui.render(&model, area, Duration::ZERO);
            let search = (rail != RailMode::Hidden).then(|| hit(&ui, &ElementId::Search));
            ui.open_tab_navigator(&model);
            assert!(ui.overlay_surface());
            assert!(!ui.content_page());
            assert!(!ui.needs_expanded_space());
            ui.render(&model, area, Duration::from_millis(1));
            assert_eq!(model.settings.rail, rail);
            assert_eq!(model.visible_ids(), tabs);
            let field = hit(&ui, &ElementId::Editor);
            assert_eq!(field.intersection(area), field);
            match search {
                Some(search) => {
                    assert_eq!(field.y, search.y);
                    assert!(field.width >= search.width.max(34));
                    assert_eq!(field.x, search.x.min(area.right() - field.width));
                }
                None => assert_centered(dialog_rect(&ui), area),
            }
        }
    }
}

#[test]
fn context_menu_opens_below_the_pointer_and_stays_inside_the_window() {
    let window = Rect::new(0, 0, 100, 32);
    for side in [Side::Left, Side::Right] {
        let mut model = model();
        model.settings.side = side;
        let mut ui = SidebarUi::new();
        ui.set_layout(28, 0);
        // Right-clicks arrive on the sidebar-only grid; the menu composes on the viewport.
        let rail = Rect::new(0, 0, 28, 32);
        ui.render(&model, rail, Duration::ZERO);
        let tab = hit(&ui, &ElementId::Tab(7));
        let click = (tab.x + 2, tab.y);
        ui.event(
            &model,
            UiInput::PointerDown {
                x: click.0,
                y: click.1,
                button: MouseButton::Right,
                modifiers: Modifiers::default(),
            },
        );
        ui.render(&model, window, Duration::from_millis(1));
        let sidebar_x = if side == Side::Right {
            window.right() - 28
        } else {
            0
        };
        let first = hit(&ui, &ElementId::Menu("activate".into()));
        assert_eq!(first.y, click.1 + 2);
        // The frame starts at the pointer column unless the menu would leave the window.
        let frame_x = (sidebar_x + click.0).min(window.right() - (first.width + 2));
        assert_eq!(first.x, frame_x + 1);
        assert!(first.width < 60);
        assert!(
            ui.hit_regions()
                .iter()
                .all(|hit| hit.rect.intersection(window) == hit.rect)
        );
        ui.dismiss();

        ui.render(&model, rail, Duration::from_millis(2));
        let space = hit(&ui, &ElementId::Space(model.selected_space.clone()));
        ui.event(
            &model,
            UiInput::PointerDown {
                x: space.x,
                y: space.bottom() - 1,
                button: MouseButton::Right,
                modifiers: Modifiers::default(),
            },
        );
        ui.render(&model, window, Duration::from_millis(3));
        let menu: Vec<_> = ui
            .hit_regions()
            .iter()
            .filter(|hit| matches!(hit.id, ElementId::Menu(_)))
            .map(|hit| hit.rect)
            .collect();
        assert!(!menu.is_empty());
        assert!(menu.iter().all(|rect| rect.bottom() < space.bottom()));
        assert!(menu.iter().all(|rect| rect.intersection(window) == *rect));
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
fn tooltip_opens_below_the_hovered_control_without_stealing_keyboard_or_pointer_targets() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    ui.render(&model, Rect::new(0, 0, 28, 32), Duration::ZERO);
    let settings = hit(&ui, &ElementId::Settings);
    ui.event(
        &model,
        UiInput::PointerMove {
            x: settings.x,
            y: settings.y,
        },
    );
    assert!(ui.overlay_surface());
    assert!(!ui.is_modal());
    assert!(!ui.has_focus());
    let area = Rect::new(0, 0, 100, 32);
    let frame = ui.render(&model, area, Duration::from_millis(601)).unwrap();
    let tooltip = ui.rounded_surfaces().last().unwrap().rect;
    assert_eq!(tooltip.y, settings.bottom());
    assert_eq!(tooltip.x, settings.x);
    assert_eq!(tooltip.intersection(area), tooltip);
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

#[test]
fn submitted_folder_dialog_keeps_rail_targets_stable_until_viewport_contracts() {
    let area = Rect::new(3, 5, 100, 32);
    for side in [Side::Left, Side::Right] {
        let mut model = model();
        model.settings.side = side;
        let mut ui = SidebarUi::new();
        ui.set_layout(28, 0);
        ui.open_create_folder();
        ui.render(&model, area, Duration::ZERO);
        ui.event(&model, UiInput::Text("Project".into()));
        for intent in ui.event(&model, UiInput::key(Key::Enter)) {
            if let UiIntent::Domain(intent) = intent {
                model.dispatch(intent).unwrap();
            }
        }
        assert!(ui.overlay_surface());
        ui.render(&model, area, Duration::from_millis(1));
        assert!(!ui.overlay_surface());

        let sidebar = Rect::new(
            if side == Side::Right {
                area.right() - 28
            } else {
                area.x
            },
            area.y,
            28,
            area.height,
        );
        assert!(
            ui.hit_regions()
                .iter()
                .all(|hit| hit.rect.intersection(sidebar) == hit.rect)
        );
        let tab = ui
            .hit_regions()
            .iter()
            .find(|hit| hit.id == ElementId::Tab(7))
            .unwrap()
            .rect;
        let folder_id = model.folders[0].id.clone();
        let folder = ui
            .hit_regions()
            .iter()
            .find(|hit| hit.id == ElementId::Folder(folder_id.clone()))
            .unwrap()
            .rect;
        ui.event(
            &model,
            UiInput::PointerDown {
                x: tab.x + tab.width / 2,
                y: tab.y + tab.height / 2,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
        );
        ui.render(&model, sidebar, Duration::from_millis(2));
        assert!(
            ui.hit_regions().iter().any(|hit| {
                hit.id == ElementId::Folder(folder_id.clone()) && hit.rect == folder
            })
        );
        ui.event(
            &model,
            UiInput::PointerMove {
                x: folder.x + folder.width / 2,
                y: folder.y + folder.height / 2,
            },
        );
        let intents = ui.event(
            &model,
            UiInput::PointerUp {
                x: folder.x + folder.width / 2,
                y: folder.y + folder.height / 2,
                button: MouseButton::Left,
            },
        );
        assert!(matches!(
            intents.as_slice(),
            [UiIntent::Domain(Intent::AssignFolder { tab_id: 7, folder_id: Some(id) })]
                if id == &folder_id
        ));
    }
}

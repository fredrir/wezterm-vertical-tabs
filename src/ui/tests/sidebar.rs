use super::*;
use std::time::Duration;
use vtabs_core::{Space, Tab};

fn row_text(ui: &SidebarUi, rect: Rect, y: u16) -> String {
    (rect.x..rect.right())
        .map(|x| ui.buffer[(x, y)].symbol())
        .collect()
}

#[test]
fn sidebar_cache_preserves_order_numbers_hidden_counts_and_collapsed_reveal() {
    let mut model = Model::default();
    model
        .reconcile(
            (1..=6)
                .map(|id| Tab {
                    id,
                    ..Tab::default()
                })
                .collect(),
            Some(1),
            true,
        )
        .unwrap();
    for name in ["Applications", "Services", "Empty"] {
        model
            .dispatch(Intent::CreateFolder { name: name.into() })
            .unwrap();
    }
    let applications = model.folders[0].id.clone();
    let services = model.folders[1].id.clone();
    for (tab_id, folder_id) in [(6, &applications), (2, &applications), (3, &services)] {
        model
            .dispatch(Intent::AssignFolder {
                tab_id,
                folder_id: Some(folder_id.clone()),
            })
            .unwrap();
    }
    model
        .dispatch(Intent::PinTab {
            id: 4,
            pinned: true,
        })
        .unwrap();
    let mut ui = SidebarUi::new();
    ui.ensure_sidebar_entries(&model);
    assert_eq!(
        ui.sidebar_rows,
        [
            SidebarRow::Tab { id: 4, number: 1 },
            SidebarRow::Folder { index: 0, count: 2 },
            SidebarRow::Tab { id: 2, number: 2 },
            SidebarRow::Tab { id: 6, number: 3 },
            SidebarRow::Folder { index: 1, count: 1 },
            SidebarRow::Tab { id: 3, number: 4 },
            SidebarRow::Folder { index: 2, count: 0 },
            SidebarRow::NewTab,
            SidebarRow::Tab { id: 1, number: 5 },
            SidebarRow::Tab { id: 5, number: 6 },
        ]
    );
    model.apply_filter_hook(6, false).unwrap();
    ui.ensure_sidebar_entries(&model);
    assert_eq!(
        ui.sidebar_rows,
        [
            SidebarRow::Tab { id: 4, number: 1 },
            SidebarRow::Folder { index: 0, count: 2 },
            SidebarRow::Tab { id: 2, number: 2 },
            SidebarRow::Folder { index: 1, count: 1 },
            SidebarRow::Tab { id: 3, number: 3 },
            SidebarRow::Folder { index: 2, count: 0 },
            SidebarRow::NewTab,
            SidebarRow::Tab { id: 1, number: 4 },
            SidebarRow::Tab { id: 5, number: 5 },
        ]
    );
    let allocation = ui.sidebar_rows.as_ptr();
    model
        .dispatch(Intent::ToggleFolder(services.clone()))
        .unwrap();
    ui.tabs_rect = Rect::new(0, 0, 20, 2);
    ui.ensure_tab_visible(&model, 3);
    assert_eq!(ui.tab_scroll, 2);
    assert_eq!(ui.sidebar_rows.as_ptr(), allocation);
    assert!(
        !ui.sidebar_rows
            .iter()
            .any(|row| matches!(row, SidebarRow::Tab { id: 3, .. }))
    );
    model.apply_filter_hook(2, false).unwrap();
    model
        .dispatch(Intent::MoveFolder {
            id: services,
            index: 0,
        })
        .unwrap();
    ui.ensure_sidebar_entries(&model);
    assert_eq!(
        ui.sidebar_rows,
        [
            SidebarRow::Tab { id: 4, number: 1 },
            SidebarRow::Folder { index: 0, count: 1 },
            SidebarRow::Folder { index: 1, count: 2 },
            SidebarRow::Folder { index: 2, count: 0 },
            SidebarRow::NewTab,
            SidebarRow::Tab { id: 1, number: 3 },
            SidebarRow::Tab { id: 5, number: 4 },
        ]
    );
}

#[test]
fn metadata_keeps_the_tab_title_and_close_control_readable() {
    let mut model = Model::default();
    model.settings.show_metadata = true;
    model.settings.show_close = true;
    model.settings.cards = true;
    model
        .reconcile(
            vec![Tab {
                id: 1,
                title: "Editor with a deliberately long name".into(),
                cwd: "~/project".into(),
                domain: "local".into(),
                ..Tab::default()
            }],
            Some(1),
            true,
        )
        .unwrap();
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 24), Duration::ZERO);
    let tab = ui
        .hits
        .iter()
        .find(|hit| hit.id == ElementId::Tab(1))
        .unwrap()
        .rect;
    let close = ui
        .hits
        .iter()
        .find(|hit| hit.id == ElementId::CloseTab(1))
        .unwrap()
        .rect;
    assert!(row_text(&ui, tab, tab.y).contains("Editor"));
    assert!(row_text(&ui, tab, tab.y + 1).contains("~/project"));
    assert_eq!(row_text(&ui, close, close.y), " × ");
}

#[test]
fn compact_footer_keeps_the_selected_space_beside_new_space() {
    let mut model = Model::default();
    model.settings.rail = RailMode::Collapsed;
    model
        .spaces
        .extend((0..8).map(|index| Space::new(format!("space-{index}"), format!("Space {index}"))));
    let mut ui = SidebarUi::new();
    let area = Rect::new(0, 0, 6, 24);
    ui.render(&model, area, Duration::ZERO);
    model.selected_space = "space-7".into();
    model.revision += 1;
    ui.render(&model, area, Duration::from_millis(1));
    let selected = ui
        .hits
        .iter()
        .find(|hit| hit.id == ElementId::Space(model.selected_space.clone()))
        .unwrap()
        .rect;
    let create = ui
        .hits
        .iter()
        .find(|hit| hit.id == ElementId::CreateSpace)
        .unwrap()
        .rect;
    assert!(!selected.is_empty());
    assert!(selected.intersection(create).is_empty());
    assert_eq!(selected.bottom(), area.bottom());
}

#[test]
fn tooltips_stay_compact_when_settings_uses_the_content_pane() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.set_layout(32, 0);
    ui.open_settings();
    let area = Rect::new(0, 0, 120, 32);
    ui.render(&model, area, Duration::ZERO);
    let refresh = ui
        .hits
        .iter()
        .find(|hit| hit.id == ElementId::Refresh)
        .unwrap()
        .rect;
    ui.event(
        &model,
        UiInput::PointerMove {
            x: refresh.x,
            y: refresh.y,
        },
    );
    ui.render(&model, area, Duration::from_millis(700));
    let tooltip = ui.rounded_surfaces.last().unwrap().rect;
    assert!(tooltip.width <= 44);
    assert_eq!(tooltip.intersection(area), tooltip);
    assert!(row_text(&ui, tooltip, tooltip.y + 1).contains("Refresh"));
}

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
            SidebarRow::Gap,
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
            SidebarRow::Gap,
            SidebarRow::NewTab,
            SidebarRow::Tab { id: 1, number: 4 },
            SidebarRow::Tab { id: 5, number: 5 },
        ]
    );
    let allocation = ui.sidebar_rows.as_ptr();
    model
        .dispatch(Intent::ToggleFolder(services.clone()))
        .unwrap();
    ui.tabs_rect = Rect::new(0, 0, 20, 3);
    ui.ensure_tab_visible(&model, 3);
    assert_eq!(ui.tab_scroll, 1);
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
            SidebarRow::Gap,
            SidebarRow::NewTab,
            SidebarRow::Tab { id: 1, number: 3 },
            SidebarRow::Tab { id: 5, number: 4 },
        ]
    );
}

#[test]
fn metadata_keeps_the_tab_directory_and_close_control_readable() {
    let mut model = Model::default();
    model.settings.show_metadata = true;
    model.settings.show_close = true;
    model.settings.cards = true;
    model.set_home(Some("/home/me".into()));
    model
        .reconcile(
            vec![Tab {
                id: 1,
                title: "Editor with a deliberately long name".into(),
                cwd: "/home/me/project".into(),
                domain: "local".into(),
                ..Tab::default()
            }],
            Some(1),
            true,
        )
        .unwrap();
    let mut ui = SidebarUi::new();
    let area = Rect::new(0, 0, 32, 24);
    ui.render(&model, area, Duration::ZERO);
    let tab = ui
        .hits
        .iter()
        .find(|hit| hit.id == ElementId::Tab(1))
        .unwrap()
        .rect;
    assert!(ui.hits.iter().all(|hit| hit.id != ElementId::CloseTab(1)));
    ui.event(
        &model,
        UiInput::PointerMove {
            x: tab.x + 1,
            y: tab.y,
        },
    );
    ui.render(&model, area, Duration::ZERO);
    let close = ui
        .hits
        .iter()
        .find(|hit| hit.id == ElementId::CloseTab(1))
        .unwrap()
        .rect;
    let row = row_text(&ui, tab, tab.y);
    assert!(row.contains("~/project"), "{row}");
    assert!(!row.contains("Editor"), "{row}");
    assert!(row_text(&ui, tab, tab.y + 1).contains("local"));
    assert_eq!(row_text(&ui, close, close.y), format!("{}  ", icons::CLOSE));
}

#[test]
fn tab_rows_show_host_icon_index_and_directory() {
    let mut model = Model::default();
    model.set_home(Some("/home/me".into()));
    model
        .reconcile(
            vec![
                Tab {
                    id: 1,
                    cwd: "/home/me/dotfiles/scripts".into(),
                    repo_root: Some("/home/me/dotfiles".into()),
                    ..Tab::default()
                },
                Tab {
                    id: 2,
                    cwd: "/home/me/Downloads".into(),
                    ..Tab::default()
                },
                Tab {
                    id: 3,
                    cwd: "/etc".into(),
                    ..Tab::default()
                },
            ],
            Some(1),
            true,
        )
        .unwrap();
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 24), Duration::ZERO);
    let row = |id: u64| {
        let rect = ui
            .hits
            .iter()
            .find(|hit| hit.id == ElementId::Tab(id))
            .unwrap()
            .rect;
        row_text(&ui, rect, rect.y)
    };
    let local = icons::LOCAL;
    assert!(
        row(1)
            .trim_start()
            .starts_with(&format!("{local} ₁ dotfiles"))
    );
    assert!(
        row(2)
            .trim_start()
            .starts_with(&format!("{local} ₂ ~/Downloads"))
    );
    assert!(row(3).trim_start().starts_with(&format!("{local} ₃ /etc")));
}

#[test]
fn renamed_tabs_replace_the_directory_and_hidden_indexes_leave_no_gap() {
    let mut model = Model::default();
    model.settings.show_indexes = false;
    model.set_home(Some("/home/me".into()));
    model
        .reconcile(
            vec![
                Tab {
                    id: 1,
                    cwd: "/home/me/project".into(),
                    title_override: Some("Deploy logs".into()),
                    ..Tab::default()
                },
                Tab {
                    id: 2,
                    cwd: "/home/me/Downloads".into(),
                    ..Tab::default()
                },
            ],
            Some(1),
            true,
        )
        .unwrap();
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 24), Duration::ZERO);
    let row = |id: u64| {
        let rect = ui
            .hits
            .iter()
            .find(|hit| hit.id == ElementId::Tab(id))
            .unwrap()
            .rect;
        row_text(&ui, rect, rect.y)
    };
    let local = icons::LOCAL;
    assert_eq!(row(1).trim(), format!("{local}   Deploy logs"));
    assert_eq!(row(2).trim(), format!("{local}   ~/Downloads"));
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
    assert_eq!(tooltip.height, 2);
    assert_eq!(tooltip.x, 32 + 1);
    let text = row_text(&ui, tooltip, tooltip.y);
    assert!(text.starts_with(" Refresh configuration"), "{text:?}");
    assert!(
        !cfg!(target_os = "macos") || !text.contains("Cmd+"),
        "{text:?}"
    );
}

#[test]
fn rows_touch_within_a_group_and_groups_keep_a_gap() {
    let mut model = Model::default();
    model.set_home(Some("/home/me".into()));
    model
        .reconcile(
            (1..=3)
                .map(|id| Tab {
                    id,
                    cwd: format!("/home/me/p{id}"),
                    ..Tab::default()
                })
                .collect(),
            Some(1),
            true,
        )
        .unwrap();
    model
        .dispatch(Intent::CreateFolder {
            name: "Services".into(),
        })
        .unwrap();
    model
        .dispatch(Intent::AssignFolder {
            tab_id: 3,
            folder_id: Some(model.folders[0].id.clone()),
        })
        .unwrap();
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 40), Duration::ZERO);
    let rect = |id: ElementId| {
        ui.hits
            .iter()
            .find(|hit| hit.id == id)
            .unwrap_or_else(|| panic!("{id:?} is not reachable"))
            .rect
    };
    let folder = ElementId::Folder(model.folders[0].id.clone());
    for (above, below) in [
        (ElementId::Rail, ElementId::Search),
        (ElementId::Search, ElementId::SpaceTitle),
        (ElementId::Tab(3), ElementId::NewTab),
    ] {
        assert_eq!(
            rect(below.clone()).y - rect(above.clone()).bottom(),
            GROUP_GAP,
            "{above:?} to {below:?}"
        );
    }
    for (above, below) in [
        (ElementId::SpaceTitle, folder.clone()),
        (folder, ElementId::Tab(3)),
        (ElementId::NewTab, ElementId::Tab(1)),
        (ElementId::Tab(1), ElementId::Tab(2)),
    ] {
        assert_eq!(
            rect(below.clone()).y,
            rect(above.clone()).bottom(),
            "{above:?} to {below:?}"
        );
    }
}

fn tabs(model: &mut Model, tabs: Vec<Tab>) {
    model.set_home(Some("/home/me".into()));
    model.reconcile(tabs, Some(1), true).unwrap();
}

fn hit_rect(ui: &SidebarUi, id: &ElementId) -> Rect {
    ui.hits
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap_or_else(|| panic!("{id:?} is not reachable"))
        .rect
}

fn pointer(ui: &mut SidebarUi, model: &Model, rect: Rect, down: bool) -> Vec<UiIntent> {
    let (x, y) = (rect.x + rect.width / 2, rect.y);
    ui.event(
        model,
        if down {
            UiInput::PointerDown {
                x,
                y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            }
        } else {
            UiInput::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            }
        },
    )
}

#[test]
fn remote_tabs_show_their_operating_system_and_local_tabs_a_terminal() {
    let mut model = Model::default();
    tabs(
        &mut model,
        vec![
            Tab {
                id: 1,
                cwd: "/home/me".into(),
                os: "arch".into(),
                ..Tab::default()
            },
            Tab {
                id: 2,
                remote: true,
                os: "arch".into(),
                ..Tab::default()
            },
            Tab {
                id: 3,
                remote: true,
                os: "ubuntu".into(),
                ..Tab::default()
            },
            Tab {
                id: 4,
                remote: true,
                ..Tab::default()
            },
        ],
    );
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 32), Duration::ZERO);
    for (id, icon) in [
        (1, "\u{f120}"),
        (2, "\u{f303}"),
        (3, "\u{ef72}"),
        (4, icons::REMOTE),
    ] {
        let rect = hit_rect(&ui, &ElementId::Tab(id));
        assert!(
            row_text(&ui, rect, rect.y)
                .trim_start()
                .starts_with(&format!("{icon} {}", icons::badge(Some(id as usize)))),
            "tab {id}"
        );
    }
}

#[test]
fn space_title_swaps_its_icon_on_hover_and_collapses_pinned_tabs_and_folders() {
    let mut model = Model::default();
    tabs(
        &mut model,
        (1..=2)
            .map(|id| Tab {
                id,
                ..Tab::default()
            })
            .collect(),
    );
    model
        .dispatch(Intent::PinTab {
            id: 2,
            pinned: true,
        })
        .unwrap();
    model
        .dispatch(Intent::CreateFolder {
            name: "Services".into(),
        })
        .unwrap();
    let folder = ElementId::Folder(model.folders[0].id.clone());
    let area = Rect::new(0, 0, 32, 32);
    let mut ui = SidebarUi::new();
    ui.render(&model, area, Duration::ZERO);
    let title = hit_rect(&ui, &ElementId::SpaceTitle);
    assert_eq!(title.height, hit_rect(&ui, &ElementId::NewTab).height);
    assert!(row_text(&ui, title, title.y).contains(icons::SPACE));
    assert!(ui.hits.iter().all(|hit| hit.id != ElementId::CreateFolder));

    ui.event(
        &model,
        UiInput::PointerMove {
            x: title.x + 1,
            y: title.y,
        },
    );
    ui.render(&model, area, Duration::ZERO);
    assert!(row_text(&ui, title, title.y).contains(icons::EXPANDED));
    let hovered = |ui: &SidebarUi, rect: Rect| {
        ui.rounded_surfaces
            .iter()
            .find(|surface| surface.rect == rect)
            .unwrap()
            .fill
    };
    assert_eq!(hovered(&ui, title), ui.theme.card);
    hit_rect(&ui, &ElementId::CreateFolder);

    pointer(&mut ui, &model, title, true);
    let intents = pointer(&mut ui, &model, title, false);
    assert!(matches!(
        intents.as_slice(),
        [UiIntent::Domain(Intent::ToggleSpace(space))] if *space == model.selected_space
    ));
    model
        .dispatch(Intent::ToggleSpace(model.selected_space.clone()))
        .unwrap();
    ui.render(&model, area, Duration::ZERO);
    assert!(row_text(&ui, title, title.y).contains(icons::COLLAPSED));
    for hidden in [ElementId::Tab(2), folder] {
        assert!(ui.hits.iter().all(|hit| hit.id != hidden), "{hidden:?}");
    }
    let rect = hit_rect(&ui, &ElementId::Tab(1));
    assert!(row_text(&ui, rect, rect.y).contains("₂"));
}

#[test]
fn split_tabs_list_their_panes_side_by_side_at_the_same_height() {
    let mut model = Model::default();
    let pane = |id, cwd: &str, active| vtabs_core::TabPane {
        id,
        cwd: cwd.into(),
        active,
        ..Default::default()
    };
    tabs(
        &mut model,
        vec![
            Tab {
                id: 1,
                cwd: "/home/me/solo".into(),
                ..Tab::default()
            },
            Tab {
                id: 2,
                cwd: "/home/me/api".into(),
                panes: vec![
                    pane(7, "/home/me/api", false),
                    pane(8, "/home/me/web", true),
                    pane(9, "/home/me/ops", false),
                ],
                ..Tab::default()
            },
        ],
    );
    let area = Rect::new(0, 0, 48, 32);
    let mut ui = SidebarUi::new();
    ui.render(&model, area, Duration::ZERO);
    let solo = hit_rect(&ui, &ElementId::Tab(1));
    let split = hit_rect(&ui, &ElementId::Tab(2));
    assert_eq!(split.height, solo.height);
    let panes: Vec<Rect> = [7, 8, 9]
        .map(|id| hit_rect(&ui, &ElementId::Pane(2, id)))
        .into();
    for pair in panes.windows(2) {
        assert_eq!(pair[0].y, pair[1].y);
        assert_eq!(pair[0].right(), pair[1].x);
    }
    for (rect, label) in panes.iter().zip(["~/api", "~/web", "~/ops"]) {
        assert_eq!(rect.height, split.height);
        assert!(row_text(&ui, *rect, rect.y).contains(label), "{label}");
    }

    pointer(&mut ui, &model, panes[2], true);
    let intents = pointer(&mut ui, &model, panes[2], false);
    assert!(matches!(
        intents.as_slice(),
        [
            UiIntent::Domain(Intent::ActivateTab(2)),
            UiIntent::Host(HostAction::FocusPane(2, 9))
        ]
    ));
}

#[test]
fn pressing_a_tab_shrinks_its_surface_and_releasing_restores_it() {
    let mut model = Model::default();
    tabs(
        &mut model,
        vec![Tab {
            id: 1,
            ..Tab::default()
        }],
    );
    let area = Rect::new(0, 0, 32, 32);
    let mut ui = SidebarUi::new();
    ui.render(&model, area, Duration::ZERO);
    let rect = hit_rect(&ui, &ElementId::Tab(1));
    let inset = |ui: &SidebarUi| {
        ui.rounded_surfaces
            .iter()
            .find(|surface| surface.rect == rect)
            .unwrap()
            .inset
    };
    let rest = inset(&ui);
    pointer(&mut ui, &model, rect, true);
    let mut previous = rest;
    for millis in [10, 30, 50, 80] {
        ui.render(&model, area, Duration::from_millis(millis));
        assert!(inset(&ui) >= previous, "{millis}ms");
        previous = inset(&ui);
    }
    assert_eq!(previous, rest + PRESS_INSET);
    assert_eq!(ui.next_deadline(), None);

    pointer(&mut ui, &model, rect, false);
    ui.render(&model, area, Duration::from_millis(100));
    assert!(ui.has_animation());
    ui.render(&model, area, Duration::from_millis(130));
    assert!(inset(&ui) < rest + PRESS_INSET && inset(&ui) > rest);
    ui.render(&model, area, Duration::from_millis(400));
    assert_eq!(inset(&ui), rest);
    assert!(!ui.has_animation());
}

#[test]
fn double_clicking_a_title_edits_it_in_place_with_the_text_selected() {
    let mut model = Model::default();
    tabs(
        &mut model,
        vec![Tab {
            id: 1,
            cwd: "/home/me/project".into(),
            ..Tab::default()
        }],
    );
    let area = Rect::new(0, 0, 32, 32);
    let mut ui = SidebarUi::new();
    ui.render(&model, area, Duration::ZERO);
    let title = ui.title_rects[0].1;
    let click = |ui: &mut SidebarUi, at: u64| {
        ui.set_clock(Duration::from_millis(at));
        pointer(ui, &model, title, true);
        let intents = pointer(ui, &model, title, false);
        ui.render(&model, area, Duration::from_millis(at));
        intents
    };
    click(&mut ui, 0);
    click(&mut ui, 2_000);
    assert!(!ui.text_input_active(), "slow clicks are two activations");
    ui.set_clock(Duration::from_millis(2_200));
    pointer(&mut ui, &model, title, true);
    assert!(ui.text_input_active());
    assert!(pointer(&mut ui, &model, title, false).is_empty());
    ui.render(&model, area, Duration::from_millis(2_200));
    assert!(row_text(&ui, title, title.y).starts_with("~/project"));

    ui.event(&model, UiInput::Text("Deploy".into()));
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    assert!(matches!(
        intents.as_slice(),
        [UiIntent::Domain(Intent::RenameTab { id: 1, title })] if title == "Deploy"
    ));
    assert!(!ui.text_input_active());

    ui.render(&model, area, Duration::from_millis(2_300));
    click(&mut ui, 3_000);
    ui.set_clock(Duration::from_millis(3_100));
    pointer(&mut ui, &model, title, true);
    assert!(ui.text_input_active());
    assert!(ui.event(&model, UiInput::key(Key::Enter)).is_empty());
    click(&mut ui, 4_000);
    ui.set_clock(Duration::from_millis(4_100));
    pointer(&mut ui, &model, title, true);
    ui.event(&model, UiInput::Text("Scratch".into()));
    assert!(ui.event(&model, UiInput::key(Key::Escape)).is_empty());
    assert!(!ui.text_input_active());
}

#[test]
fn left_icons_share_one_column_and_one_gap_before_their_text() {
    let mut model = Model::default();
    tabs(
        &mut model,
        vec![Tab {
            id: 1,
            cwd: "/home/me/project".into(),
            ..Tab::default()
        }],
    );
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 32), Duration::ZERO);
    for (id, icon, badge, text) in [
        (ElementId::Search, icons::SEARCH, " ", "Search"),
        (ElementId::SpaceTitle, icons::SPACE, " ", "Home"),
        (ElementId::NewTab, icons::PLUS, " ", "New"),
        (ElementId::Tab(1), icons::LOCAL, "₁", "~/project"),
    ] {
        let rect = hit_rect(&ui, &id);
        assert!(
            row_text(&ui, rect, rect.y).starts_with(&format!(" {icon} {badge} {text}")),
            "{id:?}: {:?}",
            row_text(&ui, rect, rect.y)
        );
    }
}

#[test]
fn rows_keep_their_full_width_and_only_icon_buttons_are_squared() {
    let mut model = Model::default();
    tabs(
        &mut model,
        vec![Tab {
            id: 1,
            ..Tab::default()
        }],
    );
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 32), Duration::ZERO);
    let square = |ui: &SidebarUi, id: ElementId| {
        let rect = hit_rect(ui, &id);
        ui.rounded_surfaces
            .iter()
            .find(|surface| surface.rect.intersects(rect) && surface.rect.y == rect.y)
            .unwrap()
            .square
    };
    for row in [ElementId::Search, ElementId::Tab(1), ElementId::NewTab] {
        assert!(!square(&ui, row.clone()), "{row:?}");
    }
    assert!(square(&ui, ElementId::Rail));
}

#[test]
fn hovering_the_close_control_adds_no_surface_of_its_own() {
    let mut model = Model::default();
    tabs(
        &mut model,
        vec![Tab {
            id: 1,
            ..Tab::default()
        }],
    );
    let area = Rect::new(0, 0, 32, 32);
    let mut ui = SidebarUi::new();
    ui.render(&model, area, Duration::ZERO);
    let row = hit_rect(&ui, &ElementId::Tab(1));
    ui.event(
        &model,
        UiInput::PointerMove {
            x: row.x + 1,
            y: row.y,
        },
    );
    ui.render(&model, area, Duration::ZERO);
    let close = hit_rect(&ui, &ElementId::CloseTab(1));
    ui.event(
        &model,
        UiInput::PointerMove {
            x: close.x + 1,
            y: close.y,
        },
    );
    ui.render(&model, area, Duration::ZERO);
    assert!(
        ui.rounded_surfaces
            .iter()
            .all(|surface| surface.rect != close)
    );
}

#[test]
fn hovering_a_split_tab_floats_the_close_control_without_moving_its_panes() {
    let mut model = Model::default();
    let pane = |id, cwd: &str, remote, os: &str| vtabs_core::TabPane {
        id,
        cwd: cwd.into(),
        active: id == 7,
        remote,
        os: os.into(),
        ..Default::default()
    };
    tabs(
        &mut model,
        vec![Tab {
            id: 1,
            panes: vec![
                pane(7, "/home/me/api", false, ""),
                pane(8, "/srv/web", true, "arch"),
            ],
            ..Tab::default()
        }],
    );
    let area = Rect::new(0, 0, 48, 32);
    let mut ui = SidebarUi::new();
    ui.render(&model, area, Duration::ZERO);
    let panes = |ui: &SidebarUi| [7, 8].map(|id| hit_rect(ui, &ElementId::Pane(1, id)));
    let resting = panes(&ui);
    let local = row_text(&ui, resting[0], resting[0].y);
    let remote = row_text(&ui, resting[1], resting[1].y);
    assert!(
        local.contains(&format!("{} ~/api", icons::LOCAL)),
        "{local:?}"
    );
    assert!(remote.contains("\u{f303} /web"), "{remote:?}");

    let row = hit_rect(&ui, &ElementId::Tab(1));
    ui.event(
        &model,
        UiInput::PointerMove {
            x: row.x + 1,
            y: row.y,
        },
    );
    ui.render(&model, area, Duration::ZERO);
    assert_eq!(panes(&ui), resting);
    let close = hit_rect(&ui, &ElementId::CloseTab(1));
    assert!(close.intersects(resting[1]));
    assert!(row_text(&ui, close, close.y).starts_with(icons::CLOSE));
    assert_eq!(
        ui.hit_test(close.x, close.y).map(|hit| &hit.id),
        Some(&ElementId::CloseTab(1))
    );
}

#[test]
fn footer_space_icons_match_the_new_space_plus() {
    let mut model = Model::default();
    model.spaces.push(Space::new("work", "Work"));
    let mut ui = SidebarUi::new();
    ui.render(&model, Rect::new(0, 0, 32, 32), Duration::ZERO);
    let plus = hit_rect(&ui, &ElementId::CreateSpace);
    let glyph = |rect: Rect| {
        let text = row_text(&ui, rect, rect.y);
        (text.find(|c: char| !c.is_whitespace()), text.trim().width())
    };
    for space in &model.spaces {
        let rect = hit_rect(&ui, &ElementId::Space(space.id.clone()));
        assert_eq!(
            (rect.y, rect.width, rect.height),
            (plus.y, plus.width, plus.height)
        );
        assert_eq!(glyph(rect), glyph(plus), "{}", space.id);
    }
}

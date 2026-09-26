use super::*;
use crate::*;
use std::time::Duration;
use vtabs_core::{Intent, Model};

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 104, 36), Duration::ZERO);
}

fn click(ui: &mut SidebarUi, model: &Model, id: ElementId) -> Vec<UiIntent> {
    let rect = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == id)
        .unwrap_or_else(|| panic!("Missing target: {id:?}"))
        .rect;
    ui.event(
        model,
        UiInput::PointerDown {
            x: rect.x,
            y: rect.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    ui.event(
        model,
        UiInput::PointerUp {
            x: rect.x,
            y: rect.y,
            button: MouseButton::Left,
        },
    )
}

fn search(ui: &mut SidebarUi, model: &Model, text: &str) {
    ui.event(
        model,
        UiInput::Key {
            key: Key::Character('f'),
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::default()
            },
        },
    );
    ui.event(model, UiInput::Text(text.into()));
    draw(ui, model);
}

#[test]
fn settings_occupy_content_area_beside_the_sidebar() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    ui.open_settings();
    draw(&mut ui, &model);
    assert!(ui.content_page());
    let search = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::SettingsSearch)
        .unwrap();
    assert!(search.rect.x >= 28);
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| { hit.id == ElementId::Settings && hit.rect.right() <= 28 })
    );
    click(&mut ui, &model, ElementId::CloseSettings);
    assert!(!ui.content_page());
}

#[test]
fn description_search_edits_and_resets_the_matching_preference() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    search(&mut ui, &model, "Suppress");
    let keys: Vec<_> = ui
        .hit_regions()
        .iter()
        .filter_map(|hit| match &hit.id {
            ElementId::Setting(key) => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(keys, ["reduced_motion"]);
    ui.event(&model, UiInput::key(Key::Enter));
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    assert!(
        matches!(intents.as_slice(), [UiIntent::Domain(Intent::SetSetting { key, value })]
        if key == "reduced_motion" && value == &serde_json::Value::Bool(true))
    );
    let intents = ui.event(&model, UiInput::key(Key::Delete));
    assert!(
        matches!(intents.as_slice(), [UiIntent::Domain(Intent::ResetSetting(key))]
        if key == "reduced_motion")
    );
}

#[test]
fn category_navigation_and_end_reveal_the_last_setting() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    click(&mut ui, &model, ElementId::SettingsCategory("theme".into()));
    ui.event(&model, UiInput::key(Key::End));
    draw(&mut ui, &model);
    assert_eq!(
        ui.focused(),
        Some(&ElementId::Setting("distro_colors".into()))
    );
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Setting("distro_colors".into()))
    );
    assert!(ui.hit_regions().iter().all(|hit| match &hit.id {
        ElementId::Setting(key) => vtabs_core::settings::descriptor(key).unwrap().group == "theme",
        _ => true,
    }));
}

#[test]
fn canceling_an_editor_returns_to_the_settings_page() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    click(&mut ui, &model, ElementId::Setting("width".into()));
    draw(&mut ui, &model);
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Editor)
    );
    ui.event(&model, UiInput::key(Key::Escape));
    draw(&mut ui, &model);
    assert!(ui.content_page());
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::SettingsSearch)
    );
    ui.event(&model, UiInput::key(Key::Escape));
    assert!(!ui.content_page());
}

#[test]
fn managed_settings_cannot_be_changed_or_reset_from_search_results() {
    let mut model = Model::default();
    model.config_owned.insert("animations".into());
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    search(&mut ui, &model, "animations");
    ui.event(&model, UiInput::key(Key::Enter));
    assert!(ui.event(&model, UiInput::key(Key::Enter)).is_empty());
    assert!(ui.event(&model, UiInput::key(Key::Delete)).is_empty());
    assert!(ui.content_page());
}

#[test]
fn empty_search_results_accept_navigation_and_escape() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    search(&mut ui, &model, "不存在");
    assert!(
        !ui.hit_regions()
            .iter()
            .any(|hit| matches!(hit.id, ElementId::Setting(_)))
    );
    for key in [Key::Down, Key::Up, Key::PageDown, Key::Enter] {
        assert!(ui.event(&model, UiInput::key(key)).is_empty());
    }
    ui.event(&model, UiInput::key(Key::Escape));
    assert!(ui.content_page());
    ui.event(&model, UiInput::key(Key::Escape));
    assert!(!ui.content_page());
}

#[test]
fn settings_render_and_close_on_small_and_offset_surfaces() {
    let model = Model::default();
    for width in 1..=36 {
        for height in 1..=24 {
            let mut ui = SidebarUi::new();
            let area = Rect::new(3, 2, width, height);
            ui.open_settings();
            ui.render(&model, area, Duration::ZERO);
            assert!(
                ui.hit_regions()
                    .iter()
                    .all(|hit| hit.rect.intersection(area) == hit.rect)
            );
            assert!(
                ui.hit_regions()
                    .iter()
                    .any(|hit| hit.id == ElementId::CloseSettings)
            );
            ui.event(&model, UiInput::key(Key::Escape));
            assert!(!ui.content_page());
        }
    }
}

#[test]
fn selecting_search_text_preserves_the_query_and_exposes_the_caret() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    ui.open_settings();
    draw(&mut ui, &model);
    search(&mut ui, &model, "Sidebar width");
    ui.event(
        &model,
        UiInput::Key {
            key: Key::Character('a'),
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::default()
            },
        },
    );
    let frame = ui
        .render(&model, Rect::new(0, 0, 104, 36), Duration::ZERO)
        .unwrap();
    assert!(frame.cursor.is_some());
    assert!(frame.ime_rect.is_some());
    let search = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::SettingsSearch)
        .unwrap()
        .rect;
    let text: String = (search.y..search.bottom())
        .flat_map(|y| (search.x..search.right()).map(move |x| (x, y)))
        .map(|position| ui.buffer()[position].symbol())
        .collect();
    assert!(
        text.contains("Sidebar width"),
        "Selected query disappeared: {text}"
    );
}

#[test]
fn delete_on_a_category_does_not_reset_an_unfocused_setting() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    click(
        &mut ui,
        &model,
        ElementId::SettingsCategory("layout".into()),
    );
    assert!(ui.event(&model, UiInput::key(Key::Delete)).is_empty());
}

#[test]
fn compact_categories_remain_reachable_by_mouse() {
    let model = Model::default();
    for (width, height) in [(14, 16), (32, 24), (50, 24)] {
        let mut ui = SidebarUi::new();
        ui.open_settings();
        let mut visited = std::collections::BTreeSet::new();
        for _ in 0..6 {
            ui.render(&model, Rect::new(0, 0, width, height), Duration::ZERO);
            let categories: Vec<_> = ui
                .hit_regions()
                .iter()
                .filter_map(|hit| match &hit.id {
                    ElementId::SettingsCategory(category) => Some(category.clone()),
                    _ => None,
                })
                .collect();
            visited.extend(categories.iter().cloned());
            if let Some(next) = categories.last() {
                click(&mut ui, &model, ElementId::SettingsCategory(next.clone()));
            }
        }
        assert_eq!(
            visited.len(),
            6,
            "Unreachable category at {width}x{height}: {visited:?}"
        );
    }
}

fn model_with_tabs() -> Model {
    let mut model = Model::default();
    model
        .reconcile(
            [(1, "Shell"), (2, "Editor")]
                .into_iter()
                .map(|(id, title)| vtabs_core::Tab {
                    id,
                    title: title.into(),
                    ..vtabs_core::Tab::default()
                })
                .collect(),
            Some(1),
            true,
        )
        .unwrap();
    model
}

fn has(ui: &SidebarUi, id: ElementId) -> bool {
    ui.hit_regions().iter().any(|hit| hit.id == id)
}

fn hover(ui: &mut SidebarUi, model: &Model, id: ElementId) {
    let rect = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == id)
        .unwrap_or_else(|| panic!("Missing target: {id:?}"))
        .rect;
    ui.event(
        model,
        UiInput::PointerMove {
            x: rect.x + 1,
            y: rect.y,
        },
    );
    draw(ui, model);
}

#[test]
fn settings_stay_listed_as_a_tab_until_closed() {
    let model = model_with_tabs();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    draw(&mut ui, &model);
    assert!(!has(&ui, ElementId::SettingsTab));
    ui.open_settings();
    draw(&mut ui, &model);
    assert!(ui.content_page());
    let row = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::SettingsTab)
        .unwrap();
    assert!(row.rect.right() <= 28);
    assert!(
        row.rect.y
            > ui.hit_regions()
                .iter()
                .find(|hit| hit.id == ElementId::Tab(2))
                .unwrap()
                .rect
                .y
    );
    let text: String = (row.rect.x..row.rect.right())
        .map(|x| ui.buffer()[(x, row.rect.y)].symbol())
        .collect();
    assert!(
        text.contains(&format!("{}  Settings", icons::SETTINGS)),
        "{text}"
    );
    assert!(!has(&ui, ElementId::CloseSettingsTab));

    let intents = click(&mut ui, &model, ElementId::Tab(2));
    assert!(matches!(
        intents.as_slice(),
        [UiIntent::Domain(Intent::ActivateTab(2))]
    ));
    assert!(!ui.content_page());
    assert!(!ui.is_modal());
    draw(&mut ui, &model);
    assert!(has(&ui, ElementId::SettingsTab));
    assert!(!has(&ui, ElementId::SettingsSearch));

    click(&mut ui, &model, ElementId::SettingsTab);
    draw(&mut ui, &model);
    assert!(ui.content_page());
    assert!(has(&ui, ElementId::SettingsSearch));

    hover(&mut ui, &model, ElementId::SettingsTab);
    click(&mut ui, &model, ElementId::CloseSettingsTab);
    draw(&mut ui, &model);
    assert!(!ui.content_page());
    assert!(!has(&ui, ElementId::SettingsTab));
}

#[test]
fn settings_tab_answers_to_its_index_like_any_tab() {
    let model = model_with_tabs();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    let chord = |ui: &mut SidebarUi, key: Key, modifiers: Modifiers| {
        ui.event(&model, UiInput::Key { key, modifiers })
    };
    let command = Modifiers {
        super_key: true,
        ..Modifiers::default()
    };
    let control = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    assert!(matches!(
        chord(&mut ui, Key::Character('3'), command).as_slice(),
        [UiIntent::Domain(Intent::ActivateIndex(2))]
    ));
    ui.open_settings();
    ui.hide_settings();
    for key in ['3', '9'] {
        assert!(chord(&mut ui, Key::Character(key), command).is_empty());
        assert!(ui.content_page(), "Cmd+{key}");
        ui.hide_settings();
    }
    assert!(matches!(
        chord(&mut ui, Key::Tab, control).as_slice(),
        [UiIntent::Domain(Intent::ActivateIndex(1))]
    ));
    ui.open_settings();
    assert!(matches!(
        chord(&mut ui, Key::Tab, control).as_slice(),
        [UiIntent::Domain(Intent::ActivateIndex(0))]
    ));
    assert!(!ui.content_page());
    let back = Modifiers {
        shift: true,
        ..control
    };
    assert!(chord(&mut ui, Key::Tab, back).is_empty());
    assert!(ui.content_page());
}

#[test]
fn keyboard_paths_hide_or_close_the_settings_tab() {
    let model = model_with_tabs();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    let command = |ui: &mut SidebarUi, character: char| {
        ui.event(
            &model,
            UiInput::Key {
                key: Key::Character(character),
                modifiers: Modifiers {
                    super_key: true,
                    ..Modifiers::default()
                },
            },
        )
    };
    command(&mut ui, ',');
    draw(&mut ui, &model);
    assert!(ui.content_page());
    assert!(matches!(
        command(&mut ui, '2').as_slice(),
        [UiIntent::Domain(Intent::ActivateIndex(1))]
    ));
    assert!(!ui.content_page());
    draw(&mut ui, &model);
    assert!(has(&ui, ElementId::SettingsTab));
    command(&mut ui, ',');
    draw(&mut ui, &model);
    assert!(ui.content_page());
    assert!(matches!(
        command(&mut ui, 'b').as_slice(),
        [UiIntent::Domain(Intent::SetRail(
            vtabs_core::RailMode::Collapsed
        ))]
    ));
    assert!(ui.content_page());
    assert!(command(&mut ui, 'w').is_empty());
    assert!(!ui.content_page());
    draw(&mut ui, &model);
    assert!(!has(&ui, ElementId::SettingsTab));
}

#[test]
fn settings_keeps_its_index_when_later_tabs_open_and_earlier_tabs_close() {
    let mut model = model_with_tabs();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    ui.open_settings();
    draw(&mut ui, &model);
    // Hovering a row swaps its icon for its index glyph; read every row's that way.
    let numbers = |ui: &mut SidebarUi, model: &Model| -> Vec<String> {
        let mut rows: Vec<_> = ui
            .hit_regions()
            .iter()
            .filter(|hit| matches!(hit.id, ElementId::Tab(_) | ElementId::SettingsTab))
            .map(|hit| (hit.rect, hit.id == ElementId::SettingsTab))
            .collect();
        rows.sort_by_key(|(rect, _)| rect.y);
        rows.into_iter()
            .map(|(rect, settings)| {
                ui.event(
                    model,
                    UiInput::PointerMove {
                        x: rect.x + 1,
                        y: rect.y,
                    },
                );
                draw(ui, model);
                let glyph = ui.buffer()[(rect.x + 1, rect.y)].symbol().to_owned();
                let number = (1..=9)
                    .find(|n| icons::index(Some(*n)) == Some(glyph.as_str()))
                    .expect("hovered row shows its index");
                format!("{number}{}", if settings { " Settings" } else { "" })
            })
            .collect()
    };
    assert_eq!(numbers(&mut ui, &model), ["1", "2", "3 Settings"]);

    let tab = |id| vtabs_core::Tab {
        id,
        ..vtabs_core::Tab::default()
    };
    model
        .reconcile(vec![tab(1), tab(2), tab(3)], Some(3), true)
        .unwrap();
    ui.hide_settings();
    draw(&mut ui, &model);
    assert_eq!(numbers(&mut ui, &model), ["1", "2", "3 Settings", "4"]);

    let command = Modifiers {
        super_key: true,
        ..Modifiers::default()
    };
    let chord = |ui: &mut SidebarUi, key: char| {
        ui.event(
            &model,
            UiInput::Key {
                key: Key::Character(key),
                modifiers: command,
            },
        )
    };
    assert!(matches!(
        chord(&mut ui, '4').as_slice(),
        [UiIntent::Domain(Intent::ActivateIndex(2))]
    ));
    assert!(matches!(
        chord(&mut ui, '9').as_slice(),
        [UiIntent::Domain(Intent::ActivateIndex(2))]
    ));
    assert!(chord(&mut ui, '3').is_empty());
    assert!(ui.content_page());

    model
        .reconcile(vec![tab(2), tab(3)], Some(3), true)
        .unwrap();
    draw(&mut ui, &model);
    assert_eq!(numbers(&mut ui, &model), ["1", "2 Settings", "3"]);
}

#[test]
fn command_w_closes_the_settings_tab_and_never_the_tab_beneath_it() {
    let model = model_with_tabs();
    let mut ui = SidebarUi::new();
    ui.set_layout(28, 0);
    draw(&mut ui, &model);
    click(&mut ui, &model, ElementId::Settings);
    draw(&mut ui, &model);
    let intents = ui.event(
        &model,
        UiInput::Key {
            key: Key::Character('w'),
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::default()
            },
        },
    );
    assert!(intents.is_empty());
    assert!(!ui.content_page());
    draw(&mut ui, &model);
    assert!(!has(&ui, ElementId::SettingsTab));
}

#[test]
fn copying_search_text_keeps_the_selected_setting() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    search(&mut ui, &model, "a");
    let press = |ui: &mut SidebarUi, key: Key, super_key: bool| {
        ui.event(
            &model,
            UiInput::Key {
                key,
                modifiers: Modifiers {
                    super_key,
                    ..Modifiers::default()
                },
            },
        )
    };
    press(&mut ui, Key::Escape, false);
    press(&mut ui, Key::Down, false);
    press(&mut ui, Key::Down, false);
    press(&mut ui, Key::Character('f'), true);
    assert_eq!(ui.settings.selected, 2);

    let intents = press(&mut ui, Key::Character('c'), true);
    assert!(matches!(
        intents.as_slice(),
        [UiIntent::SetClipboard(text)] if text == "a"
    ));
    assert_eq!(ui.settings.selected, 2);
}

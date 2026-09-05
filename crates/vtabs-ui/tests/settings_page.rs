use std::time::Duration;
use vtabs_core::{Intent, Model};
use vtabs_ui::{ElementId, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent};

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
        Some(&ElementId::Setting("private_accent".into()))
    );
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Setting("private_accent".into()))
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

use serde_json::{Value, json};
use std::time::Duration;
use vtabs_core::{Intent, Model};
use vtabs_ui::{ElementId, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent};

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 110, 70), Duration::ZERO);
}

fn rect(ui: &SidebarUi, id: &ElementId) -> Rect {
    ui.hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap_or_else(|| panic!("missing {id:?}"))
        .rect
}

fn click(ui: &mut SidebarUi, model: &Model, id: ElementId) -> Vec<UiIntent> {
    draw(ui, model);
    let rect = rect(ui, &id);
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

fn settings(category: &str) -> (SidebarUi, Model) {
    let mut model = Model::default();
    model.settings.animations = false;
    let mut ui = SidebarUi::new();
    ui.open_settings();
    click(
        &mut ui,
        &model,
        ElementId::SettingsCategory(category.into()),
    );
    (ui, model)
}

fn set(intents: &[UiIntent], key: &str) -> Option<Value> {
    intents.iter().find_map(|intent| match intent {
        UiIntent::Domain(Intent::SetSetting { key: actual, value }) if actual == key => {
            Some(value.clone())
        }
        _ => None,
    })
}

fn selected_rows(ui: &SidebarUi) -> Vec<String> {
    ui.hit_regions()
        .iter()
        .filter_map(|hit| match &hit.id {
            ElementId::Setting(key) => Some(key.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn numbers_take_typed_values_in_display_units_and_stay_in_range() {
    for (key, typed, expected) in [
        ("quick_terminal_opacity", "0", 0.),
        ("quick_terminal_opacity", "35", 0.35),
        ("quick_terminal_opacity", "250", 1.),
        ("quick_terminal_width", "60", 0.6),
        ("quick_terminal_blur", "16", 16.),
        ("quick_terminal_blur", "99", 64.),
    ] {
        let (mut ui, model) = settings("quick_terminal");
        click(&mut ui, &model, ElementId::Setting(key.into()));
        assert!(
            ui.text_input_active(),
            "{key}: activating a number edits it in place"
        );
        ui.event(&model, UiInput::key(Key::Backspace));
        ui.event(&model, UiInput::Text(typed.into()));
        let intents = ui.event(&model, UiInput::key(Key::Enter));
        let value = set(&intents, key).unwrap_or_else(|| panic!("{key}: {intents:?}"));
        assert_eq!(value.as_f64(), Some(expected), "{key} typed {typed}");
        let mut applied = model.settings.clone();
        applied.set(key, value).unwrap();
    }
}

#[test]
fn values_below_the_minimum_apply_the_minimum_and_escape_discards() {
    let (mut ui, model) = settings("layout");
    click(&mut ui, &model, ElementId::Setting("width".into()));
    ui.event(&model, UiInput::Text("5".into()));
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    assert_eq!(set(&intents, "width"), Some(json!(32)));

    click(&mut ui, &model, ElementId::Setting("width".into()));
    ui.event(&model, UiInput::Text("900".into()));
    assert!(ui.event(&model, UiInput::key(Key::Escape)).is_empty());
    assert!(!ui.text_input_active());
    assert!(ui.content_page());
}

#[test]
fn up_and_down_step_a_value_being_edited_and_enter_applies_it() {
    let (mut ui, model) = settings("quick_terminal");
    click(
        &mut ui,
        &model,
        ElementId::Setting("quick_terminal_blur".into()),
    );
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    for (key, modifiers) in [
        (Key::Up, Modifiers::default()),
        (Key::Up, shift),
        (Key::Down, Modifiers::default()),
    ] {
        assert!(
            ui.event(&model, UiInput::Key { key, modifiers }).is_empty(),
            "stepping previews until Enter"
        );
    }
    assert!(ui.text_input_active());
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    assert_eq!(set(&intents, "quick_terminal_blur"), Some(json!(10)));

    click(
        &mut ui,
        &model,
        ElementId::Setting("quick_terminal_blur".into()),
    );
    ui.event(&model, UiInput::key(Key::Down));
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    assert_eq!(
        set(&intents, "quick_terminal_blur"),
        Some(json!(0)),
        "already at the minimum"
    );
}

#[test]
fn dragging_a_slider_previews_and_applies_once_on_release() {
    let (mut ui, model) = settings("layout");
    draw(&mut ui, &model);
    let track = rect(&ui, &ElementId::Slider("width".into()));
    ui.event(
        &model,
        UiInput::PointerDown {
            x: track.x,
            y: track.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    let end = track.right() - 1;
    assert!(
        ui.event(&model, UiInput::PointerMove { x: end, y: track.y })
            .is_empty()
    );
    let intents = ui.event(
        &model,
        UiInput::PointerUp {
            x: end,
            y: track.y,
            button: MouseButton::Left,
        },
    );
    assert_eq!(set(&intents, "width"), Some(json!(1024)));
}

#[test]
fn color_entry_accepts_hex_and_empty_restores_automatic() {
    for (key, text, expected) in [
        (
            "quick_terminal_background_color",
            "#123456",
            json!("#123456"),
        ),
        ("quick_terminal_border_color", "", json!(null)),
    ] {
        let (mut ui, mut model) = settings("quick_terminal");
        model.settings.quick_terminal_border_color = Some("#ffffff".into());
        click(&mut ui, &model, ElementId::Setting(key.into()));
        ui.event(&model, UiInput::key(Key::Backspace));
        ui.event(&model, UiInput::Text(text.into()));
        let intents = ui.event(&model, UiInput::key(Key::Enter));
        assert_eq!(set(&intents, key), Some(expected), "{key}");
    }
}

#[test]
fn color_palette_previews_until_apply_and_cancel_discards() {
    let (mut ui, model) = settings("theme");
    click(&mut ui, &model, ElementId::Setting("accent".into()));
    draw(&mut ui, &model);
    assert_eq!(
        ui.hit_regions()
            .iter()
            .filter(|hit| matches!(hit.id, ElementId::ColorSwatch(_)))
            .count(),
        30
    );
    assert!(click(&mut ui, &model, ElementId::ColorSwatch(17)).is_empty());
    draw(&mut ui, &model);
    let field = rect(&ui, &ElementId::Editor);
    let text: String = (field.x..field.right())
        .map(|x| ui.buffer()[(x, field.y)].symbol())
        .collect();
    assert!(text.contains("#3b82f6"), "{text}");
    assert_eq!(
        ui.theme().accent,
        vtabs_ui::Theme::parse_color(&model.settings.accent).unwrap()
    );
    let intents = click(&mut ui, &model, ElementId::Submit);
    assert_eq!(set(&intents, "accent"), Some(json!("#3b82f6")));

    ui.dismiss();
    click(&mut ui, &model, ElementId::Setting("accent".into()));
    click(&mut ui, &model, ElementId::ColorSwatch(11));
    assert!(click(&mut ui, &model, ElementId::Cancel).is_empty());
    draw(&mut ui, &model);
    assert!(
        !ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::ColorPlane)
    );
    assert!(ui.content_page());
}

#[test]
fn color_reset_and_original_restore_the_right_values() {
    for (category, key, original, reset) in [
        (
            "theme",
            "accent",
            "#112233",
            json!(vtabs_core::Settings::default().accent),
        ),
        (
            "quick_terminal",
            "quick_terminal_border_color",
            "#abcdef",
            json!(null),
        ),
    ] {
        let (mut ui, mut model) = settings(category);
        model.settings.set(key, json!(original)).unwrap();
        click(&mut ui, &model, ElementId::Setting(key.into()));
        click(&mut ui, &model, ElementId::ColorSwatch(11));
        click(&mut ui, &model, ElementId::ColorOriginal);
        assert_eq!(
            set(&click(&mut ui, &model, ElementId::Submit), key),
            Some(json!(original))
        );
        ui.dismiss();
        click(&mut ui, &model, ElementId::Setting(key.into()));
        click(&mut ui, &model, ElementId::ColorReset);
        assert_eq!(
            set(&click(&mut ui, &model, ElementId::Submit), key),
            Some(reset)
        );
    }
}

#[test]
fn color_drag_clamps_outside_the_control_and_only_apply_saves() {
    let (mut ui, model) = settings("theme");
    click(&mut ui, &model, ElementId::Setting("accent".into()));
    draw(&mut ui, &model);
    for (id, end, expected) in [
        (ElementId::ColorPlane, (0, 0), "#ffffff"),
        (ElementId::ColorPlane, (109, 69), "#000000"),
    ] {
        let area = rect(&ui, &id);
        for event in [
            UiInput::PointerDown {
                x: area.x,
                y: area.y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            UiInput::PointerMove { x: end.0, y: end.1 },
            UiInput::PointerUp {
                x: end.0,
                y: end.1,
                button: MouseButton::Left,
            },
        ] {
            assert!(ui.event(&model, event).is_empty());
        }
        assert_eq!(
            set(&click(&mut ui, &model, ElementId::Submit), "accent"),
            Some(json!(expected))
        );
    }
    // Saturation and value at full, hue at zero produce pure red.
    let plane = rect(&ui, &ElementId::ColorPlane);
    ui.event(
        &model,
        UiInput::PointerDown {
            x: plane.right() - 1,
            y: plane.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    ui.event(
        &model,
        UiInput::PointerUp {
            x: plane.right() - 1,
            y: plane.y,
            button: MouseButton::Left,
        },
    );
    assert!(click(&mut ui, &model, ElementId::ColorHue).is_empty());
    assert_eq!(
        set(&click(&mut ui, &model, ElementId::Submit), "accent"),
        Some(json!("#ff0000"))
    );
}

#[test]
fn color_picker_can_be_operated_with_only_the_keyboard() {
    let (mut ui, model) = settings("theme");
    click(&mut ui, &model, ElementId::Setting("accent".into()));
    draw(&mut ui, &model);
    for expected in [
        ElementId::ColorFormat,
        ElementId::ColorPlane,
        ElementId::ColorHue,
        ElementId::ColorSwatch(0),
    ] {
        ui.event(&model, UiInput::key(Key::Tab));
        assert_eq!(ui.focused(), Some(&expected));
    }
    assert!(ui.event(&model, UiInput::key(Key::Right)).is_empty());
    assert!(ui.event(&model, UiInput::key(Key::Down)).is_empty());
    assert_eq!(ui.focused(), Some(&ElementId::ColorSwatch(11)));
    assert_eq!(
        set(&ui.event(&model, UiInput::key(Key::Enter)), "accent"),
        Some(json!("#f43f5e"))
    );
    ui.event(&model, UiInput::key(Key::Escape));
    assert!(ui.content_page());
}

#[test]
fn color_picker_enter_applies_from_any_focus_except_cancel() {
    for id in [
        ElementId::ColorOriginal,
        ElementId::ColorReset,
        ElementId::ColorFormat,
        ElementId::ColorSwatch(11),
        ElementId::Submit,
    ] {
        let (mut ui, model) = settings("theme");
        click(&mut ui, &model, ElementId::Setting("accent".into()));
        click(&mut ui, &model, ElementId::ColorSwatch(11));
        click(&mut ui, &model, id.clone());
        assert!(
            set(&ui.event(&model, UiInput::key(Key::Enter)), "accent").is_some(),
            "{id:?}"
        );
    }
    let (mut ui, model) = settings("theme");
    click(&mut ui, &model, ElementId::Setting("accent".into()));
    draw(&mut ui, &model);
    ui.event(
        &model,
        UiInput::Key {
            key: Key::Tab,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        },
    );
    assert_eq!(ui.focused(), Some(&ElementId::Cancel));
    assert!(ui.event(&model, UiInput::key(Key::Enter)).is_empty());
    draw(&mut ui, &model);
    assert!(ui.content_page());
}

#[test]
fn color_picker_keeps_keyboard_focus_before_its_first_frame() {
    let (mut ui, model) = settings("theme");
    click(&mut ui, &model, ElementId::Setting("accent".into()));
    // An input event can arrive before the newly opened overlay is painted.
    ui.event(&model, UiInput::key(Key::Tab));
    assert!(ui.text_input_active());
    assert!(ui.event(&model, UiInput::key(Key::Escape)).is_empty());
    draw(&mut ui, &model);
    assert!(
        !ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::ColorPlane)
    );
}

#[test]
fn color_picker_resizes_without_overlapping_or_escaping_its_controls() {
    let (mut ui, model) = settings("theme");
    click(&mut ui, &model, ElementId::Setting("accent".into()));
    for (width, height) in [
        (1, 1),
        (3, 3),
        (10, 8),
        (24, 12),
        (40, 20),
        (80, 24),
        (110, 70),
    ] {
        let area = Rect::new(2, 3, width, height);
        ui.render(&model, area, Duration::ZERO);
        for (index, hit) in ui.hit_regions().iter().enumerate() {
            assert_eq!(
                hit.rect.intersection(area),
                hit.rect,
                "{width}x{height}: {hit:?}"
            );
            for other in &ui.hit_regions()[index + 1..] {
                assert!(
                    hit.rect.intersection(other.rect).is_empty(),
                    "{width}x{height}: {hit:?} overlaps {other:?}"
                );
            }
        }
    }
}

#[test]
fn escape_keeps_the_settings_page_open_like_any_tab() {
    let (mut ui, model) = settings("all");
    for _ in 0..3 {
        ui.event(&model, UiInput::key(Key::Escape));
    }
    assert!(ui.content_page());
}

#[test]
fn wheel_scrolls_the_list_without_changing_the_selection() {
    let (mut short, model) = settings("all");
    let render = |ui: &mut SidebarUi| ui.render(&model, Rect::new(0, 0, 110, 30), Duration::ZERO);
    render(&mut short);
    let top = selected_rows(&short);
    assert!(
        short
            .hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Scrollbar(vtabs_ui::ScrollArea::Settings))
    );
    let page = rect(&short, &ElementId::Setting(top[1].clone()));
    short.event(&model, UiInput::key(Key::Down));
    render(&mut short);
    short.event(
        &model,
        UiInput::Scroll {
            x: page.x,
            y: page.y,
            rows: 3,
        },
    );
    render(&mut short);
    let scrolled = selected_rows(&short);
    assert_eq!(scrolled.first(), top.get(3));
    assert!(
        !scrolled.contains(&top[1]),
        "the selected row may leave the view"
    );

    short.event(&model, UiInput::key(Key::Down));
    render(&mut short);
    assert!(
        selected_rows(&short).contains(&top[2]),
        "keys bring the selection back"
    );
}

#[test]
fn settings_controls_carry_no_tooltips() {
    let (mut ui, model) = settings("all");
    draw(&mut ui, &model);
    let page = ui
        .hit_regions()
        .iter()
        .find(|hit| matches!(hit.id, ElementId::SettingsSearch))
        .unwrap()
        .rect;
    assert!(
        ui.hit_regions()
            .iter()
            .filter(|hit| hit.rect.x >= page.x)
            .all(|hit| hit.tooltip.is_empty())
    );
}

fn focused_row(ui: &SidebarUi) -> Option<String> {
    match ui.focused() {
        Some(ElementId::Setting(key)) => Some(key.clone()),
        _ => None,
    }
}

#[test]
fn arrows_wrap_within_the_list() {
    let (mut ui, model) = settings("keybinds");
    draw(&mut ui, &model);
    let tab = ElementId::SettingsCategory("keybinds".into());
    assert_eq!(ui.focused(), Some(&tab));
    ui.event(&model, UiInput::key(Key::Down));
    assert_eq!(focused_row(&ui).as_deref(), Some("shortcut.settings"));
    ui.event(&model, UiInput::key(Key::Up));
    assert_eq!(focused_row(&ui).as_deref(), Some("shortcut.next_space"));
    ui.event(&model, UiInput::key(Key::Down));
    assert_eq!(focused_row(&ui).as_deref(), Some("shortcut.settings"));
    ui.event(&model, UiInput::key(Key::Right));
    assert_eq!(focused_row(&ui).as_deref(), Some("width"));
}

#[test]
fn left_and_right_switch_tabs_from_the_list_unless_text_is_being_edited() {
    let (mut ui, model) = settings("layout");
    ui.event(&model, UiInput::key(Key::Down));
    assert_eq!(focused_row(&ui).as_deref(), Some("width"));
    assert!(
        ui.event(&model, UiInput::key(Key::Right)).is_empty(),
        "sliders do not step"
    );
    assert_eq!(
        focused_row(&ui).as_deref(),
        Some("quick_terminal_width"),
        "the list keeps focus on the next tab"
    );
    ui.event(&model, UiInput::key(Key::Left));
    ui.event(&model, UiInput::key(Key::Left));
    assert_eq!(
        focused_row(&ui).as_deref(),
        Some("width"),
        "All lists Layout first"
    );

    click(&mut ui, &model, ElementId::Setting("width".into()));
    ui.event(&model, UiInput::key(Key::Left));
    ui.event(&model, UiInput::key(Key::Right));
    assert!(
        ui.text_input_active(),
        "arrows move the caret while a value is edited"
    );
    assert_eq!(focused_row(&ui).as_deref(), Some("width"));
}

#[test]
fn choices_open_a_dropdown_beneath_their_value() {
    let (mut ui, model) = settings("layout");
    draw(&mut ui, &model);
    let row = rect(&ui, &ElementId::Setting("side".into()));
    click(&mut ui, &model, ElementId::Setting("side".into()));
    draw(&mut ui, &model);
    let left = rect(&ui, &ElementId::Menu("left".into()));
    let right = rect(&ui, &ElementId::Menu("right".into()));
    assert!(
        left.y >= row.bottom() && right.y > left.y,
        "{row:?} {left:?}"
    );
    assert!(left.right() <= row.right() && left.x > row.x + row.width / 2);
    assert!(
        ui.hit_regions()
            .iter()
            .filter(|hit| matches!(hit.id, ElementId::Menu(_)))
            .all(|hit| hit.tooltip.is_empty())
    );
    ui.event(&model, UiInput::key(Key::Down));
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    assert_eq!(set(&intents, "side"), Some(json!("right")));
}

#[test]
fn changed_settings_draw_their_label_in_the_accent() {
    let (mut ui, mut model) = settings("layout");
    model.settings.width = 300;
    ui.event(&model, UiInput::key(Key::Escape));
    draw(&mut ui, &model);
    let label = |ui: &SidebarUi, key: &str| {
        let row = rect(ui, &ElementId::Setting(key.into()));
        ui.buffer()[(row.x + 2, row.y)].fg
    };
    let accent = ui.theme().accent;
    assert_eq!(label(&ui, "width"), accent);
    assert_ne!(label(&ui, "side"), accent);
}

#[test]
fn quick_terminal_colors_also_list_under_colors_with_their_own_labels() {
    let text = |ui: &SidebarUi, key: &str| {
        let row = rect(ui, &ElementId::Setting(key.into()));
        (row.x + 2..row.right())
            .map(|x| ui.buffer()[(x, row.y)].symbol().to_owned())
            .collect::<String>()
    };
    let keys = [
        "quick_terminal_border_color",
        "quick_terminal_background_color",
    ];

    let (mut ui, model) = settings("theme");
    draw(&mut ui, &model);
    assert!(selected_rows(&ui).ends_with(&keys.map(String::from)));
    assert!(text(&ui, keys[0]).starts_with("Quick terminal border "));
    assert!(text(&ui, keys[1]).starts_with("Quick terminal background "));

    let (mut ui, model) = settings("quick_terminal");
    draw(&mut ui, &model);
    assert!(text(&ui, keys[0]).starts_with("Border color "));

    let (mut ui, model) = settings("all");
    draw(&mut ui, &model);
    let rows = selected_rows(&ui);
    for key in keys {
        assert_eq!(rows.iter().filter(|row| *row == key).count(), 1);
    }
}

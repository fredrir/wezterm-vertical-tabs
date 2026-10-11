use serde_json::json;
use std::time::Duration;
use vtabs_core::{Intent, Model};
use vtabs_ui::{ElementId, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent};

fn click(ui: &mut SidebarUi, model: &Model, id: ElementId) -> Vec<UiIntent> {
    ui.render(model, Rect::new(0, 0, 110, 70), Duration::ZERO);
    let rect = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == id)
        .unwrap()
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

#[test]
fn quick_terminal_editors_accept_zero_opacity_and_restore_automatic_colors() {
    for (key, text, expected) in [
        ("quick_terminal_opacity", "0", json!(0)),
        ("quick_terminal_opacity", "0.35", json!(0.35)),
        ("quick_terminal_blur", "16", json!(16)),
        (
            "quick_terminal_background_color",
            "#123456",
            json!("#123456"),
        ),
        ("quick_terminal_border_color", "", json!(null)),
    ] {
        let mut model = Model::default();
        model.settings.animations = false;
        model.settings.quick_terminal_border_color = Some("#ffffff".into());
        let mut ui = SidebarUi::new();
        ui.open_settings();
        click(
            &mut ui,
            &model,
            ElementId::SettingsCategory("quick_terminal".into()),
        );
        click(&mut ui, &model, ElementId::Setting(key.into()));
        ui.event(&model, UiInput::key(Key::Backspace));
        ui.event(&model, UiInput::Text(text.into()));
        let intents = ui.event(&model, UiInput::key(Key::Enter));
        assert!(intents.iter().any(|intent| matches!(intent,
            UiIntent::Domain(Intent::SetSetting { key: actual, value }) if actual == key && value == &expected
        )), "{key}: {intents:?}");
    }
}

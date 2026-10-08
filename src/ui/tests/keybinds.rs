use std::time::Duration;
use vtabs_core::{Intent, Model, keybinds};
use vtabs_ui::{
    ElementId, HostAction, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent,
};

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 110, 42), Duration::ZERO);
}
fn click(ui: &mut SidebarUi, model: &mut Model, id: ElementId) {
    draw(ui, model);
    let rect = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == id)
        .unwrap_or_else(|| panic!("missing {id:?}"))
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
    let intents = ui.event(
        model,
        UiInput::PointerUp {
            x: rect.x,
            y: rect.y,
            button: MouseButton::Left,
        },
    );
    apply(model, intents);
}
fn apply(model: &mut Model, intents: Vec<UiIntent>) {
    for intent in intents {
        if let UiIntent::Domain(Intent::SetSetting { key, value }) = intent {
            model.settings.set(&key, value).unwrap();
            model.revision += 1;
        }
    }
}
fn menu(ui: &mut SidebarUi, model: &mut Model, id: &str) {
    click(ui, model, ElementId::Menu(id.into()));
}
fn setup() -> (SidebarUi, Model) {
    let mut model = Model::default();
    model.settings.animations = false;
    let mut ui = SidebarUi::new();
    ui.open_settings();
    click(
        &mut ui,
        &mut model,
        ElementId::SettingsCategory("keybinds".into()),
    );
    (ui, model)
}
fn commands(ui: &mut SidebarUi, model: &mut Model) {
    click(ui, model, ElementId::Setting("shortcut.commands".into()));
}
fn record(ui: &mut SidebarUi, model: &Model, chord: &str) {
    assert!(ui.recording_shortcut());
    assert!(
        ui.event(model, UiInput::RecordShortcut(chord.into()))
            .is_empty()
    );
    ui.event(model, UiInput::key(Key::Escape));
    assert!(!ui.recording_shortcut());
}

#[test]
fn settings_can_add_replace_remove_and_reset_a_plugin_shortcut() {
    let (mut ui, mut model) = setup();
    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "add");
    record(&mut ui, &model, "F12");
    menu(&mut ui, &mut model, "save-keybind");
    assert!(keybinds::bindings(&model.settings, "commands").contains(&"F12".into()));
    assert!(matches!(
        ui.event(&model, UiInput::key(Key::Function(12))).as_slice(),
        [UiIntent::Host(HostAction::OpenCommands)]
    ));

    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "binding/0");
    menu(&mut ui, &mut model, "change");
    record(&mut ui, &model, "Ctrl+Alt+p");
    menu(&mut ui, &mut model, "save-keybind");
    assert_eq!(
        keybinds::bindings(&model.settings, "commands")[0],
        "Ctrl+Alt+p"
    );
    assert_eq!(
        keybinds::action(&model.settings, &keybinds::defaults("commands")[0]),
        None
    );

    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "binding/0");
    menu(&mut ui, &mut model, "remove");
    assert!(!keybinds::bindings(&model.settings, "commands").contains(&"Ctrl+Alt+p".into()));
    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "reset");
    assert_eq!(
        keybinds::bindings(&model.settings, "commands"),
        keybinds::defaults("commands")
    );
    assert!(!model.settings.keybinds.contains_key("commands"));
}

#[test]
fn conflicting_shortcuts_and_cancel_leave_saved_bindings_unchanged() {
    let (mut ui, mut model) = setup();
    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "add");
    record(&mut ui, &model, &keybinds::defaults("settings")[0]);
    menu(&mut ui, &mut model, "save-keybind");
    assert!(model.settings.keybinds.is_empty());
    assert!(ui.has_overlay());
    menu(&mut ui, &mut model, "cancel-keybind");
    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "add");
    record(&mut ui, &model, "F12");
    menu(&mut ui, &mut model, "cancel-keybind");
    assert!(model.settings.keybinds.is_empty());
}

#[test]
fn lua_owned_bindings_cannot_be_edited_from_settings() {
    for host_owned in [false, true] {
        let (mut ui, mut model) = setup();
        if host_owned {
            ui.set_config_owned(["keybinds".into()]);
        } else {
            model.config_owned.insert("keybinds".into());
        }
        commands(&mut ui, &mut model);
        assert!(!ui.has_overlay());
        assert!(ui.event(&model, UiInput::key(Key::Delete)).is_empty());
    }
}

#[test]
fn recorded_shortcut_can_be_saved_with_enter_and_ignores_pasted_text() {
    let (mut ui, mut model) = setup();
    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "add");
    record(&mut ui, &model, "Super+Alt+p");
    ui.event(&model, UiInput::Paste("unrelated clipboard text".into()));
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    apply(&mut model, intents);
    assert!(keybinds::bindings(&model.settings, "commands").contains(&"Super+Alt+p".into()));
    assert!(!ui.has_overlay());
}

#[test]
fn restoring_a_default_does_not_overwrite_another_actions_shortcut() {
    let (mut ui, mut model) = setup();
    let reassigned = keybinds::defaults("commands")[0].clone();
    model
        .settings
        .set(
            "keybinds",
            serde_json::json!({"commands": [], "settings": [reassigned]}),
        )
        .unwrap();
    commands(&mut ui, &mut model);
    menu(&mut ui, &mut model, "reset");
    assert!(keybinds::bindings(&model.settings, "commands").is_empty());
    assert_eq!(
        keybinds::action(&model.settings, &reassigned),
        Some("settings")
    );
    assert!(ui.has_overlay());
}

#[test]
fn removing_a_shortcut_uses_current_bindings_after_a_reload() {
    for current in [vec!["F11", "F12"], vec!["F11"]] {
        let (mut ui, mut model) = setup();
        model
            .settings
            .set("keybinds", serde_json::json!({"commands": ["F12", "F11"]}))
            .unwrap();
        commands(&mut ui, &mut model);
        menu(&mut ui, &mut model, "binding/0");

        // Another window reorders or removes the selected binding and edits another action.
        model
            .settings
            .set(
                "keybinds",
                serde_json::json!({"commands": current, "settings": ["F10"]}),
            )
            .unwrap();
        model.revision += 1;
        menu(&mut ui, &mut model, "remove");

        assert_eq!(keybinds::bindings(&model.settings, "commands"), ["F11"]);
        assert_eq!(keybinds::bindings(&model.settings, "settings"), ["F10"]);
    }
}

#[test]
fn removing_a_shortcut_rechecks_lua_ownership_after_menu_open() {
    for host_owned in [false, true] {
        let (mut ui, mut model) = setup();
        commands(&mut ui, &mut model);
        menu(&mut ui, &mut model, "binding/0");
        if host_owned {
            ui.set_config_owned(["keybinds".into()]);
        } else {
            model.config_owned.insert("keybinds".into());
        }
        menu(&mut ui, &mut model, "remove");
        assert!(model.settings.keybinds.is_empty());
    }
}

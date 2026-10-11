use std::time::Duration;
use vtabs_core::{Intent, Model, keybinds};
use vtabs_ui::{
    ElementId, HostAction, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent,
};

const COMMANDS: &str = "shortcut.commands";

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 110, 42), Duration::ZERO);
}
fn rect(ui: &SidebarUi, id: &ElementId) -> Rect {
    ui.hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap_or_else(|| panic!("missing {id:?}"))
        .rect
}
fn press(ui: &mut SidebarUi, model: &mut Model, at: Rect) {
    ui.event(
        model,
        UiInput::PointerDown {
            x: at.x,
            y: at.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    let intents = ui.event(
        model,
        UiInput::PointerUp {
            x: at.x,
            y: at.y,
            button: MouseButton::Left,
        },
    );
    apply(model, intents);
}
fn click(ui: &mut SidebarUi, model: &mut Model, id: ElementId) {
    draw(ui, model);
    let at = rect(ui, &id);
    press(ui, model, at);
}
fn hover(ui: &mut SidebarUi, model: &Model, id: ElementId) {
    draw(ui, model);
    let at = rect(ui, &id);
    ui.event(model, UiInput::PointerMove { x: at.x, y: at.y });
    draw(ui, model);
}
fn apply(model: &mut Model, intents: Vec<UiIntent>) {
    for intent in intents {
        if let UiIntent::Domain(Intent::SetSetting { key, value }) = intent {
            model.settings.set(&key, value).unwrap();
            model.revision += 1;
        }
    }
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
    click(&mut ui, &mut model, ElementId::Setting(COMMANDS.into()));
    (ui, model)
}
fn record(ui: &mut SidebarUi, model: &mut Model, chord: &str) {
    assert!(ui.recording_keybind());
    let intents = ui.event(model, UiInput::RecordShortcut(chord.into()));
    apply(model, intents);
}
fn remove(chord: &str) -> ElementId {
    ElementId::RemoveChord(COMMANDS.into(), chord.into())
}

#[test]
fn chord_pills_add_replace_remove_and_reset_a_plugin_shortcut() {
    let (mut ui, mut model) = setup();
    click(&mut ui, &mut model, ElementId::AddChord(COMMANDS.into()));
    record(&mut ui, &mut model, "F12");
    assert!(!ui.recording_shortcut());
    assert!(keybinds::bindings(&model.settings, "commands").contains(&"F12".into()));
    assert!(matches!(
        ui.event(&model, UiInput::key(Key::Function(12))).as_slice(),
        [UiIntent::Host(HostAction::OpenCommands)]
    ));

    click(&mut ui, &mut model, ElementId::Chord(COMMANDS.into(), 0));
    record(&mut ui, &mut model, "Ctrl+Alt+p");
    assert_eq!(
        keybinds::bindings(&model.settings, "commands")[0],
        "Ctrl+Alt+p"
    );
    assert_eq!(
        keybinds::action(&model.settings, &keybinds::defaults("commands")[0]),
        None
    );

    hover(&mut ui, &model, ElementId::Chord(COMMANDS.into(), 0));
    click(&mut ui, &mut model, remove("Ctrl+Alt+p"));
    assert!(!keybinds::bindings(&model.settings, "commands").contains(&"Ctrl+Alt+p".into()));

    ui.event(&model, UiInput::key(Key::Escape));
    let intents = ui.event(&model, UiInput::key(Key::Delete));
    apply(&mut model, intents);
    assert_eq!(
        keybinds::bindings(&model.settings, "commands"),
        keybinds::defaults("commands")
    );
    assert!(!model.settings.keybinds.contains_key("commands"));
}

#[test]
fn keyboard_reaches_each_chord_and_the_add_pill() {
    let (mut ui, mut model) = setup();
    let count = keybinds::bindings(&model.settings, "commands").len();
    ui.event(&model, UiInput::key(Key::Left));
    ui.event(&model, UiInput::key(Key::Enter));
    record(&mut ui, &mut model, "F12");
    assert_eq!(
        keybinds::bindings(&model.settings, "commands").last(),
        Some(&"F12".to_owned())
    );

    for _ in 0..=count {
        ui.event(&model, UiInput::key(Key::Left));
    }
    let intents = ui.event(&model, UiInput::key(Key::Delete));
    apply(&mut model, intents);
    assert_eq!(
        keybinds::bindings(&model.settings, "commands").len(),
        count,
        "Delete on a focused chord removes only that chord"
    );
    assert!(
        !keybinds::bindings(&model.settings, "commands")
            .contains(&keybinds::defaults("commands")[0])
    );
}

#[test]
fn conflicting_shortcuts_and_cancel_leave_saved_bindings_unchanged() {
    let (mut ui, mut model) = setup();
    click(&mut ui, &mut model, ElementId::AddChord(COMMANDS.into()));
    record(&mut ui, &mut model, &keybinds::defaults("settings")[0]);
    assert!(model.settings.keybinds.is_empty());
    assert!(
        ui.recording_keybind(),
        "a conflict keeps recording for another try"
    );
    record(&mut ui, &mut model, "");
    assert!(!ui.recording_shortcut());

    click(&mut ui, &mut model, ElementId::AddChord(COMMANDS.into()));
    ui.event(&model, UiInput::key(Key::Escape));
    assert!(!ui.recording_shortcut());
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
        draw(&mut ui, &model);
        assert!(ui.hit_regions().iter().all(|hit| !matches!(
            hit.id,
            ElementId::AddChord(_) | ElementId::Chord(..) | ElementId::RemoveChord(..)
        )));
        ui.event(&model, UiInput::key(Key::Enter));
        assert!(!ui.recording_shortcut());
        assert!(ui.event(&model, UiInput::key(Key::Delete)).is_empty());
    }
}

#[test]
fn recording_ignores_pasted_text() {
    let (mut ui, mut model) = setup();
    click(&mut ui, &mut model, ElementId::AddChord(COMMANDS.into()));
    assert!(
        ui.event(&model, UiInput::Paste("unrelated clipboard text".into()))
            .is_empty()
    );
    record(&mut ui, &mut model, "Super+Alt+p");
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
    draw(&mut ui, &model);
    let intents = ui.event(&model, UiInput::key(Key::Delete));
    apply(&mut model, intents);
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
        hover(&mut ui, &model, ElementId::Chord(COMMANDS.into(), 0));
        let at = rect(&ui, &remove("F12"));

        // Another window reorders or removes the pointed binding and edits another action.
        model
            .settings
            .set(
                "keybinds",
                serde_json::json!({"commands": current, "settings": ["F10"]}),
            )
            .unwrap();
        model.revision += 1;
        press(&mut ui, &mut model, at);

        assert_eq!(keybinds::bindings(&model.settings, "commands"), ["F11"]);
        assert_eq!(keybinds::bindings(&model.settings, "settings"), ["F10"]);
    }
}

#[test]
fn removing_a_shortcut_rechecks_lua_ownership_after_it_was_drawn() {
    for host_owned in [false, true] {
        let (mut ui, mut model) = setup();
        hover(&mut ui, &model, ElementId::Chord(COMMANDS.into(), 0));
        let at = rect(&ui, &remove(&keybinds::defaults("commands")[0]));
        if host_owned {
            ui.set_config_owned(["keybinds".into()]);
        } else {
            model.config_owned.insert("keybinds".into());
        }
        press(&mut ui, &mut model, at);
        assert!(model.settings.keybinds.is_empty());
    }
}

#[test]
fn recording_in_search_lists_the_actions_bound_to_a_chord() {
    let (mut ui, mut model) = setup();
    click(&mut ui, &mut model, ElementId::SettingsRecord);
    record(&mut ui, &mut model, &keybinds::defaults("tabs")[0]);
    draw(&mut ui, &model);
    let rows: Vec<_> = ui
        .hit_regions()
        .iter()
        .filter_map(|hit| match &hit.id {
            ElementId::Setting(key) => Some(key.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rows, ["shortcut.tabs"]);
    assert!(ui.recording_keybind(), "recording continues until Escape");
    record(&mut ui, &mut model, "");
    assert!(!ui.recording_shortcut());

    ui.event(&model, UiInput::key(Key::Character('f')));
    draw(&mut ui, &model);
    assert!(
        ui.hit_regions()
            .iter()
            .all(|hit| hit.id != ElementId::Setting("shortcut.tabs".into())),
        "typing returns to a text search"
    );
}

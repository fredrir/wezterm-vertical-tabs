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
    (ui, model)
}
fn open(ui: &mut SidebarUi, model: &mut Model) {
    click(ui, model, ElementId::Setting(COMMANDS.into()));
    assert!(ui.recording_keybind(), "the launcher opens recording");
}
fn record(ui: &mut SidebarUi, model: &mut Model, chord: &str) {
    assert!(ui.recording_keybind());
    let intents = ui.event(model, UiInput::RecordShortcut(chord.into()));
    apply(model, intents);
    draw(ui, model);
}
fn rows(ui: &SidebarUi) -> Vec<String> {
    ui.hit_regions()
        .iter()
        .filter_map(|hit| match &hit.id {
            ElementId::Menu(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}
fn chord(chord: &str) -> ElementId {
    ElementId::Menu(format!("chord/{chord}"))
}
fn commands(model: &Model) -> Vec<String> {
    keybinds::bindings(&model.settings, "commands")
}

fn text(ui: &SidebarUi, rect: Rect) -> String {
    (rect.x..rect.right())
        .map(|x| ui.buffer()[(x, rect.y)].symbol())
        .collect()
}

fn caps(ui: &SidebarUi, row: Rect) -> Vec<Rect> {
    ui.rounded_surfaces()
        .iter()
        .map(|surface| surface.rect)
        .filter(|rect| *rect != row && rect.y == row.y && rect.intersection(row) == *rect)
        .collect()
}

#[test]
fn settings_and_editor_box_each_key_and_preserve_a_literal_plus() {
    let (mut ui, mut model) = setup();
    model.settings.keybinds.insert(
        "commands".into(),
        vec!["Ctrl+Alt+Left".into(), "Ctrl++".into()],
    );
    model.revision += 1;
    ui.set_chord_label(|chord| chord.replace("Left", "←"));
    draw(&mut ui, &model);
    let row = rect(&ui, &ElementId::Setting(COMMANDS.into()));
    let cards = caps(&ui, row);
    let labels: Vec<_> = cards.iter().map(|rect| text(&ui, *rect)).collect();
    assert_eq!(labels, [" Ctrl ", " Alt ", " ← ", " Ctrl ", " + "]);
    assert!(cards[3].x > cards[2].right(), "separate alternative chords");
    assert_eq!(text(&ui, row).matches('+').count(), 1);

    open(&mut ui, &mut model);
    draw(&mut ui, &model);
    let row = rect(&ui, &chord("Ctrl+Alt+Left"));
    let labels: Vec<_> = caps(&ui, row).iter().map(|rect| text(&ui, *rect)).collect();
    assert_eq!(labels, [" Ctrl ", " Alt ", " ← "]);
}

#[test]
fn narrow_settings_keep_chords_whole_and_expose_hidden_bindings_in_the_tooltip() {
    let (mut ui, mut model) = setup();
    model.settings.keybinds.insert(
        "commands".into(),
        vec!["Ctrl+Alt+Left".into(), "Ctrl+Shift+Right".into()],
    );
    model.revision += 1;
    ui.set_chord_label(str::to_owned);
    for width in [1, 8, 24, 40, 60, 80, 110] {
        let area = Rect::new(2, 3, width, 42);
        ui.render(&model, area, Duration::ZERO);
        let row = rect(&ui, &ElementId::Setting(COMMANDS.into()));
        let cards = caps(&ui, row);
        assert_eq!(cards.len() % 3, 0, "{width}: never show a partial chord");
        for card in &cards {
            assert_eq!(card.intersection(area), *card);
        }
        if cards.len() < 6 && width >= 8 {
            assert!(text(&ui, row).contains('…'), "{width}: indicate overflow");
        }
        let hit = ui.hit_regions().iter().find(|hit| hit.rect == row).unwrap();
        assert!(hit.tooltip.contains("Ctrl+Alt+Left"));
        assert!(hit.tooltip.contains("Ctrl+Shift+Right"));
    }
}

#[test]
fn launcher_adds_replaces_removes_and_resets_a_plugin_shortcut() {
    let (mut ui, mut model) = setup();
    let first = keybinds::defaults("commands")[0].clone();
    open(&mut ui, &mut model);
    record(&mut ui, &mut model, "F12");
    assert!(commands(&model).contains(&"F12".into()));
    assert!(ui.has_overlay(), "the launcher stays open for more changes");
    assert!(rows(&ui).contains(&"chord/F12".into()));
    record(&mut ui, &mut model, "");
    assert!(!ui.has_overlay());
    assert!(matches!(
        ui.event(&model, UiInput::key(Key::Function(12))).as_slice(),
        [UiIntent::Host(HostAction::OpenCommands)]
    ));

    open(&mut ui, &mut model);
    click(&mut ui, &mut model, chord(&first));
    record(&mut ui, &mut model, "Ctrl+Alt+p");
    assert_eq!(commands(&model)[0], "Ctrl+Alt+p");
    assert_eq!(keybinds::action(&model.settings, &first), None);

    hover(&mut ui, &model, chord("Ctrl+Alt+p"));
    click(
        &mut ui,
        &mut model,
        ElementId::MenuRemove("chord/Ctrl+Alt+p".into()),
    );
    assert!(!commands(&model).contains(&"Ctrl+Alt+p".into()));

    click(&mut ui, &mut model, ElementId::Menu("reset".into()));
    assert_eq!(commands(&model), keybinds::defaults("commands"));
    assert!(!model.settings.keybinds.contains_key("commands"));
}

#[test]
fn plain_navigation_keys_steer_the_list_while_recording() {
    let (mut ui, mut model) = setup();
    let defaults = keybinds::defaults("commands");
    open(&mut ui, &mut model);
    for key in ["Down", "Up", "Down", "Up"] {
        record(&mut ui, &mut model, key);
    }
    assert!(model.settings.keybinds.is_empty(), "navigation never binds");
    record(&mut ui, &mut model, "Enter");
    record(&mut ui, &mut model, "F11");
    assert_eq!(commands(&model)[0], "F11", "Enter on a chord re-records it");

    record(&mut ui, &mut model, "Down");
    record(&mut ui, &mut model, "Delete");
    let mut expected = vec!["F11".to_owned()];
    expected.extend_from_slice(&defaults[2..]);
    assert_eq!(commands(&model), expected);
}

#[test]
fn a_clash_shows_its_owner_and_leaves_bindings_unchanged() {
    let (mut ui, mut model) = setup();
    open(&mut ui, &mut model);
    record(&mut ui, &mut model, &keybinds::defaults("settings")[0]);
    assert!(model.settings.keybinds.is_empty());
    assert_eq!(rows(&ui).first().map(String::as_str), Some("rejected"));
    record(&mut ui, &mut model, "");
    assert!(ui.has_overlay(), "Escape first clears the clash");
    assert!(!rows(&ui).contains(&"rejected".into()));
    record(&mut ui, &mut model, "");
    assert!(!ui.has_overlay());
    assert!(model.settings.keybinds.is_empty());
}

#[test]
fn lua_owned_bindings_cannot_be_edited_from_settings() {
    let (mut ui, mut model) = setup();
    model.config_owned.insert("keybinds".into());
    click(&mut ui, &mut model, ElementId::Setting(COMMANDS.into()));
    assert!(!ui.has_overlay());
    assert!(!ui.recording_shortcut());
    assert!(ui.event(&model, UiInput::key(Key::Delete)).is_empty());
}

#[test]
fn recording_ignores_pasted_text() {
    let (mut ui, mut model) = setup();
    open(&mut ui, &mut model);
    assert!(
        ui.event(&model, UiInput::Paste("unrelated clipboard text".into()))
            .is_empty()
    );
    record(&mut ui, &mut model, "Super+Alt+p");
    assert!(commands(&model).contains(&"Super+Alt+p".into()));
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
    open(&mut ui, &mut model);
    record(&mut ui, &mut model, "");
    let intents = ui.event(&model, UiInput::key(Key::Delete));
    apply(&mut model, intents);
    draw(&mut ui, &model);
    assert!(commands(&model).is_empty());
    assert_eq!(
        keybinds::action(&model.settings, &reassigned),
        Some("settings")
    );
    assert!(
        ui.recording_keybind(),
        "the clash opens where it can be resolved"
    );
    assert!(rows(&ui).contains(&"rejected".into()));
}

#[test]
fn removing_a_shortcut_uses_current_bindings_after_a_reload() {
    for current in [vec!["F11", "F12"], vec!["F11"]] {
        let (mut ui, mut model) = setup();
        model
            .settings
            .set("keybinds", serde_json::json!({"commands": ["F12", "F11"]}))
            .unwrap();
        open(&mut ui, &mut model);
        hover(&mut ui, &model, chord("F12"));
        let at = rect(&ui, &ElementId::MenuRemove("chord/F12".into()));

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

        assert_eq!(commands(&model), ["F11"]);
        assert_eq!(keybinds::bindings(&model.settings, "settings"), ["F10"]);
    }
}

#[test]
fn removing_a_shortcut_rechecks_lua_ownership_after_it_was_drawn() {
    let (mut ui, mut model) = setup();
    let first = keybinds::defaults("commands")[0].clone();
    open(&mut ui, &mut model);
    hover(&mut ui, &model, chord(&first));
    let at = rect(&ui, &ElementId::MenuRemove(format!("chord/{first}")));
    model.config_owned.insert("keybinds".into());
    press(&mut ui, &mut model, at);
    assert!(model.settings.keybinds.is_empty());
}

#[test]
fn recording_in_search_lists_the_actions_bound_to_a_chord() {
    let (mut ui, mut model) = setup();
    click(&mut ui, &mut model, ElementId::SettingsRecord);
    record(&mut ui, &mut model, &keybinds::defaults("tabs")[0]);
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

fn action_model() -> Model {
    let mut model = Model::default();
    model.settings.animations = false;
    model
        .reconcile(
            (1..=3)
                .map(|id| vtabs_core::HostTab {
                    id,
                    title: format!("Tab {id}"),
                    ..Default::default()
                })
                .collect(),
            Some(2),
            true,
        )
        .unwrap();
    model
}

fn bind_and_run(ui: &mut SidebarUi, model: &mut Model, action: &str) -> Vec<UiIntent> {
    model
        .settings
        .set("keybinds", serde_json::json!({action: ["F12"]}))
        .unwrap();
    ui.event(model, UiInput::key(Key::Function(12)))
}

fn domain(intents: &[UiIntent]) -> Vec<Intent> {
    intents
        .iter()
        .filter_map(|intent| match intent {
            UiIntent::Domain(intent) => Some(intent.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn assigned_tab_actions_target_the_current_tab_and_toggle_pin_state() {
    let mut model = action_model();
    let mut ui = SidebarUi::new();
    for (action, expected) in [
        (
            "pin_tab",
            Intent::PinTab {
                id: 2,
                pinned: true,
            },
        ),
        ("move_tab_top", Intent::MoveTab { id: 2, index: 0 }),
        ("move_tab_bottom", Intent::MoveTab { id: 2, index: 3 }),
        ("move_tab_window", Intent::MoveTabToNewWindow(2)),
        ("return_tab_auto", Intent::ReturnToAuto(2)),
    ] {
        assert_eq!(
            domain(&bind_and_run(&mut ui, &mut model, action)),
            [expected],
            "{action}"
        );
    }
    model
        .dispatch(Intent::PinTab {
            id: 2,
            pinned: true,
        })
        .unwrap();
    assert_eq!(
        domain(&bind_and_run(&mut ui, &mut model, "pin_tab")),
        [Intent::PinTab {
            id: 2,
            pinned: false
        }]
    );
    assert!(bind_and_run(&mut ui, &mut model, "rename_tab").is_empty());
    assert!(ui.text_input_active());
    ui.event(&model, UiInput::Text("Named by shortcut".into()));
    assert_eq!(
        domain(&ui.event(&model, UiInput::key(Key::Enter))),
        [Intent::RenameTab {
            id: 2,
            title: "Named by shortcut".into()
        }]
    );
}

#[test]
fn bulk_close_shortcuts_keep_confirmation_and_disabled_action_checks() {
    for (action, expected) in [
        ("close_tabs_below", Intent::CloseBelow(2)),
        ("close_other_tabs", Intent::CloseOthers(2)),
    ] {
        let mut model = action_model();
        let mut ui = SidebarUi::new();
        assert!(bind_and_run(&mut ui, &mut model, action).is_empty());
        assert!(ui.has_overlay());
        assert_eq!(
            domain(&ui.event(&model, UiInput::key(Key::Enter))),
            [expected]
        );
    }
    let mut model = action_model();
    model.dispatch(Intent::ActivateTab(3)).unwrap();
    let mut ui = SidebarUi::new();
    assert!(bind_and_run(&mut ui, &mut model, "close_tabs_below").is_empty());
    assert!(!ui.has_overlay());
}

#[test]
fn move_tab_shortcuts_choose_a_space_or_folder_before_dispatching() {
    let mut model = action_model();
    let original = model.selected_space.clone();
    model
        .dispatch(Intent::CreateSpace {
            name: "Work".into(),
        })
        .unwrap();
    let destination = model.selected_space.clone();
    model.dispatch(Intent::SelectSpace(original)).unwrap();
    model
        .dispatch(Intent::CreateFolder {
            name: "Projects".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    for (action, expected) in [
        (
            "move_tab_space",
            Intent::AssignTab {
                id: 2,
                space_id: destination,
            },
        ),
        (
            "move_tab_folder",
            Intent::AssignFolder {
                tab_id: 2,
                folder_id: Some(folder),
            },
        ),
    ] {
        let mut ui = SidebarUi::new();
        assert!(bind_and_run(&mut ui, &mut model, action).is_empty());
        assert!(ui.has_overlay());
        assert_eq!(
            domain(&ui.event(&model, UiInput::key(Key::Enter))),
            [expected]
        );
    }
}

#[test]
fn numbered_space_shortcuts_follow_order_and_ignore_missing_spaces() {
    let mut model = Model::default();
    let mut ui = SidebarUi::new();
    for number in 2..=9 {
        model
            .dispatch(Intent::CreateSpace {
                name: format!("Space {number}"),
            })
            .unwrap();
    }
    let last = model.spaces[8].id.clone();
    model
        .dispatch(Intent::MoveSpace { id: last, index: 0 })
        .unwrap();
    for number in 1..=9 {
        let expected = Intent::SelectSpace(model.spaces[number - 1].id.clone());
        assert_eq!(
            domain(&bind_and_run(
                &mut ui,
                &mut model,
                &format!("space_{number}")
            )),
            [expected]
        );
    }
    let mut empty = Model::default();
    assert!(bind_and_run(&mut ui, &mut empty, "space_2").is_empty());
    assert!(!ui.has_overlay());
}

#[test]
fn space_editing_shortcuts_open_the_existing_editors() {
    for (action, text, expected) in [
        (
            "rename_space",
            "Renamed",
            Intent::RenameSpace {
                id: "home".into(),
                name: "Renamed".into(),
            },
        ),
        (
            "edit_space_icon",
            "W",
            Intent::EditSpace {
                id: "home".into(),
                icon: "W".into(),
                accent: None,
                rules: vec![],
            },
        ),
        (
            "edit_space_accent",
            "#123456",
            Intent::EditSpace {
                id: "home".into(),
                icon: "◉".into(),
                accent: Some("#123456".into()),
                rules: vec![],
            },
        ),
        (
            "edit_space_rules",
            "[]",
            Intent::EditSpace {
                id: "home".into(),
                icon: "◉".into(),
                accent: None,
                rules: vec![],
            },
        ),
    ] {
        let mut model = Model::default();
        let mut ui = SidebarUi::new();
        assert!(bind_and_run(&mut ui, &mut model, action).is_empty());
        assert!(ui.text_input_active(), "{action}");
        ui.event(&model, UiInput::Text(text.into()));
        assert_eq!(
            domain(&ui.event(&model, UiInput::key(Key::Enter))),
            [expected],
            "{action}"
        );
    }
}

#[test]
fn folder_shortcuts_use_the_current_tabs_folder() {
    let mut model = action_model();
    model
        .dispatch(Intent::CreateFolder {
            name: "Work".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    model
        .dispatch(Intent::AssignFolder {
            tab_id: 2,
            folder_id: Some(folder.clone()),
        })
        .unwrap();
    let mut ui = SidebarUi::new();
    for (action, expected) in [
        ("toggle_folder", Intent::ToggleFolder(folder.clone())),
        ("new_tab_in_folder", Intent::NewTabInFolder(folder.clone())),
        ("delete_folder", Intent::DeleteFolder(folder)),
    ] {
        assert_eq!(
            domain(&bind_and_run(&mut ui, &mut model, action)),
            [expected]
        );
    }
    assert!(bind_and_run(&mut ui, &mut model, "move_folder_up").is_empty());
    assert!(bind_and_run(&mut ui, &mut model, "move_folder_down").is_empty());
}

#[test]
fn tab_actions_are_inert_without_a_tab_and_while_settings_are_open() {
    for action in [
        "rename_tab",
        "pin_tab",
        "move_tab_top",
        "move_tab_bottom",
        "move_tab_space",
        "move_tab_folder",
        "move_tab_window",
        "close_tabs_below",
        "close_other_tabs",
        "return_tab_auto",
    ] {
        for (mut model, settings) in [(Model::default(), false), (action_model(), true)] {
            let mut ui = SidebarUi::new();
            if settings {
                ui.open_settings();
            }
            assert!(
                bind_and_run(&mut ui, &mut model, action).is_empty(),
                "{action}"
            );
            assert!(!ui.has_overlay(), "{action}");
            assert_eq!(ui.content_page(), settings);
        }
    }
}

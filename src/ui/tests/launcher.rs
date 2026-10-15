use std::time::Duration;
use vtabs_core::{HostTab, Intent, Model, Tab};
use vtabs_ui::{
    AdoptablePane, CommandEntry, ElementId, HostAction, Key, Modifiers, MouseButton, Rect,
    SidebarUi, UiInput, UiIntent,
};

fn model() -> Model {
    let mut model = Model::default();
    model.settings.animations = false;
    model
}

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 80, 24), Duration::ZERO);
}

fn click(ui: &mut SidebarUi, model: &Model, id: &ElementId) {
    let rect = ui
        .hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .expect("hit region")
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
    );
}

fn with_tabs(count: u64, selected: u64) -> Model {
    let mut model = model();
    let tabs = (1..=count)
        .map(|id| HostTab {
            id,
            title: format!("Tab {id}"),
            ..HostTab::default()
        })
        .collect();
    model.reconcile(tabs, Some(selected), true).unwrap();
    model
}

const TAB_ACTIONS: [&str; 10] = [
    "rename",
    "pin",
    "move-top",
    "move-bottom",
    "move-space",
    "move-folder",
    "move-window",
    "close",
    "close-below",
    "close-others",
];

fn open_tab_actions(ui: &mut SidebarUi, model: &Model) {
    ui.event(
        model,
        UiInput::Key {
            key: Key::Character('e'),
            modifiers: Modifiers {
                super_key: true,
                shift: true,
                ..Modifiers::default()
            },
        },
    );
    ui.render(model, Rect::new(0, 0, 110, 42), Duration::ZERO);
}

fn command(label: &str, shortcut: &str) -> CommandEntry {
    CommandEntry {
        label: label.into(),
        shortcuts: vec![shortcut.into()],
        description: String::new(),
    }
}

fn results(ui: &SidebarUi) -> Vec<String> {
    ui.hit_regions()
        .iter()
        .filter_map(|hit| match &hit.id {
            ElementId::Menu(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn field_text(ui: &SidebarUi, id: &ElementId) -> String {
    let rect = ui
        .hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .expect("hit region")
        .rect;
    (rect.x..rect.right())
        .map(|x| ui.buffer()[(x, rect.y)].symbol())
        .collect()
}

#[test]
fn escape_leaves_recording_and_keeps_the_captured_shortcut_as_the_query() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_commands(vec![command("Alpha", "Ctrl+A"), command("Beta", "Ctrl+B")]);
    draw(&mut ui, &model);
    assert_eq!(results(&ui).len(), 2);

    click(&mut ui, &model, &ElementId::RecordShortcut);
    draw(&mut ui, &model);
    assert!(ui.recording_shortcut());

    ui.event(&model, UiInput::RecordShortcut("Ctrl+A".into()));
    draw(&mut ui, &model);
    assert_eq!(results(&ui), ["command/0"]);

    // Text typed while recording never reaches the query.
    ui.event(&model, UiInput::Text("X".into()));
    draw(&mut ui, &model);
    assert_eq!(results(&ui), ["command/0"]);

    ui.event(&model, UiInput::key(Key::Escape));
    draw(&mut ui, &model);
    assert!(!ui.recording_shortcut());
    // The captured shortcut stays in the field and filters as a normal query.
    assert!(field_text(&ui, &ElementId::Editor).contains("Ctrl+A"));
    assert_eq!(results(&ui), ["command/0"]);

    // Typing now edits the query, proving normal search mode is restored.
    ui.event(&model, UiInput::Text("X".into()));
    draw(&mut ui, &model);
    assert!(results(&ui).is_empty());
}

#[test]
fn unadopted_panes_join_an_open_tab_search_and_enter_adopts() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model);
    assert!(results(&ui).is_empty());

    let tab = Tab {
        title: "zsh".into(),
        cwd: "/home/me/notes".into(),
        home: Some("/home/me".into()),
        remote: true,
        os: "arch".into(),
        ..Default::default()
    };
    ui.set_adoptable_panes(vec![AdoptablePane::new("remote", 7, &tab, None, "archie")]);
    draw(&mut ui, &model);
    assert_eq!(results(&ui), ["adopt/remote/7"]);

    ui.event(&model, UiInput::Text("archie".into()));
    draw(&mut ui, &model);
    assert_eq!(results(&ui), ["adopt/remote/7"]);
    let intents = ui.event(&model, UiInput::key(Key::Enter));
    assert!(intents.iter().any(|intent| matches!(
        intent,
        UiIntent::Host(HostAction::AdoptPane { domain, pane: 7 }) if domain == "remote"
    )));
}

#[test]
fn wheel_scrolls_results_without_changing_the_selection() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_commands(
        (0..30)
            .map(|index| command(&format!("Command {index:02}"), ""))
            .collect(),
    );
    draw(&mut ui, &model);
    let first = results(&ui);
    ui.event(
        &model,
        UiInput::Scroll {
            x: 40,
            y: 12,
            rows: 5,
        },
    );
    draw(&mut ui, &model);
    let scrolled = results(&ui);
    assert_eq!(scrolled.first(), first.get(5));
    assert!(matches!(
        ui.event(&model, UiInput::key(Key::Enter)).as_slice(),
        [UiIntent::Host(HostAction::RunCommand(0))]
    ));
}

#[test]
fn tab_actions_shortcut_lists_actions_and_digits_run_them() {
    let model = with_tabs(3, 2);
    let mut ui = SidebarUi::new();
    open_tab_actions(&mut ui, &model);
    assert_eq!(results(&ui), TAB_ACTIONS);
    assert!(matches!(
        ui.event(&model, UiInput::key(Key::Character('4')))
            .as_slice(),
        [UiIntent::Domain(Intent::MoveTab { id: 2, index: 3 })]
    ));
    assert!(!ui.has_overlay());
}

#[test]
fn finishing_an_inline_rename_returns_the_keyboard_to_the_terminal() {
    let model = with_tabs(3, 2);
    for (key, renamed) in [(Key::Enter, true), (Key::Escape, false)] {
        let mut ui = SidebarUi::new();
        ui.set_layout(32, 0);
        draw(&mut ui, &model);
        open_tab_actions(&mut ui, &model);
        ui.event(&model, UiInput::key(Key::Character('1')));
        assert_eq!(ui.focused(), Some(&ElementId::Editor));
        ui.event(&model, UiInput::Text("Renamed".into()));
        let intents = ui.event(&model, UiInput::key(key));
        assert_eq!(
            matches!(
                intents.as_slice(),
                [UiIntent::Domain(Intent::RenameTab { id: 2, title })] if title == "Renamed"
            ),
            renamed
        );
        assert_eq!(ui.focused(), None);
    }
}

#[test]
fn clicking_a_tab_leaves_the_keyboard_with_the_terminal() {
    let model = with_tabs(3, 1);
    let mut ui = SidebarUi::new();
    ui.set_layout(32, 0);
    draw(&mut ui, &model);
    ui.event(&model, UiInput::key(Key::Tab));
    assert!(ui.focused().is_some());
    click(&mut ui, &model, &ElementId::Tab(2));
    assert_eq!(ui.focused(), None);
}

#[test]
fn disabled_tab_action_digits_leave_the_query_untouched() {
    let model = with_tabs(3, 3);
    let mut ui = SidebarUi::new();
    open_tab_actions(&mut ui, &model);
    assert!(
        ui.event(&model, UiInput::key(Key::Character('9')))
            .is_empty()
    );
    ui.render(&model, Rect::new(0, 0, 110, 42), Duration::ZERO);
    assert_eq!(results(&ui), TAB_ACTIONS);
}

#[test]
fn tab_context_menu_matches_tab_actions_and_takes_digits() {
    let model = with_tabs(3, 1);
    let mut ui = SidebarUi::new();
    ui.set_layout(32, 0);
    draw(&mut ui, &model);
    let tab = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Tab(2))
        .expect("tab row")
        .rect;
    ui.event(
        &model,
        UiInput::PointerDown {
            x: tab.x,
            y: tab.y,
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        },
    );
    draw(&mut ui, &model);
    assert_eq!(results(&ui), TAB_ACTIONS);
    assert!(matches!(
        ui.event(&model, UiInput::key(Key::Character('2')))
            .as_slice(),
        [UiIntent::Domain(Intent::PinTab {
            id: 2,
            pinned: true
        })]
    ));
}

const SPACE_ACTIONS: [&str; 8] = [
    "select", "rename", "icon", "accent", "rules", "up", "down", "delete",
];

fn open_space_actions(ui: &mut SidebarUi, model: &Model) {
    ui.event(
        model,
        UiInput::Key {
            key: Key::Character('j'),
            modifiers: Modifiers {
                super_key: true,
                shift: true,
                ..Default::default()
            },
        },
    );
    ui.render(model, Rect::new(0, 0, 110, 42), Duration::ZERO);
}

#[test]
fn space_actions_shortcut_opens_in_an_empty_space_and_filters_actions() {
    let model = model();
    let mut ui = SidebarUi::new();
    open_space_actions(&mut ui, &model);
    assert_eq!(results(&ui), SPACE_ACTIONS);
    ui.event(&model, UiInput::Text("accent".into()));
    draw(&mut ui, &model);
    assert_eq!(results(&ui), ["accent"]);
    assert!(ui.event(&model, UiInput::key(Key::Enter)).is_empty());
    assert!(ui.text_input_active());
    ui.event(&model, UiInput::Text("#123456".into()));
    assert!(
        matches!(ui.event(&model, UiInput::key(Key::Enter)).as_slice(),
        [UiIntent::Domain(Intent::EditSpace { id, accent: Some(accent), .. })]
            if id == &model.selected_space && accent == "#123456")
    );
}

#[test]
fn disabled_space_action_digits_do_not_edit_the_query_or_delete_the_final_space() {
    let model = model();
    let mut ui = SidebarUi::new();
    open_space_actions(&mut ui, &model);
    for key in ['6', '7', '8'] {
        assert!(
            ui.event(&model, UiInput::key(Key::Character(key)))
                .is_empty()
        );
        draw(&mut ui, &model);
        assert_eq!(results(&ui), SPACE_ACTIONS);
    }
    ui.event(&model, UiInput::key(Key::Character('2')));
    ui.event(&model, UiInput::Text("Renamed".into()));
    assert!(
        matches!(ui.event(&model, UiInput::key(Key::Enter)).as_slice(),
        [UiIntent::Domain(Intent::RenameSpace { id, name })]
            if id == &model.selected_space && name == "Renamed")
    );
}

#[test]
fn space_context_menu_and_launcher_share_actions_and_target_their_own_space() {
    let mut model = model();
    let original = model.selected_space.clone();
    model
        .dispatch(Intent::CreateSpace {
            name: "Work".into(),
        })
        .unwrap();
    let mut ui = SidebarUi::new();
    ui.set_layout(32, 0);
    open_space_actions(&mut ui, &model);
    assert_eq!(results(&ui), SPACE_ACTIONS);
    assert!(
        matches!(ui.event(&model, UiInput::key(Key::Character('6'))).as_slice(),
        [UiIntent::Domain(Intent::MoveSpace { id, index: 0 })] if id == &model.selected_space)
    );
    draw(&mut ui, &model);
    let space = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Space(original.clone()))
        .unwrap()
        .rect;
    ui.event(
        &model,
        UiInput::PointerDown {
            x: space.x,
            y: space.y,
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        },
    );
    draw(&mut ui, &model);
    assert_eq!(results(&ui), SPACE_ACTIONS);
    assert!(
        matches!(ui.event(&model, UiInput::key(Key::Character('7'))).as_slice(),
        [UiIntent::Domain(Intent::MoveSpace { id, index: 1 })] if id == &original)
    );
}

#[test]
fn space_launcher_obeys_custom_binding_and_global_shortcut_switch() {
    let mut model = model();
    model
        .settings
        .set("keybinds", serde_json::json!({"space_actions": ["F12"]}))
        .unwrap();
    let mut ui = SidebarUi::new();
    open_space_actions(&mut ui, &model);
    assert!(!ui.has_overlay());
    ui.event(&model, UiInput::key(Key::Function(12)));
    draw(&mut ui, &model);
    assert_eq!(results(&ui), SPACE_ACTIONS);
    ui.event(&model, UiInput::key(Key::Escape));
    model.settings.keyboard_shortcuts = false;
    ui.event(&model, UiInput::key(Key::Function(12)));
    assert!(!ui.has_overlay());
}

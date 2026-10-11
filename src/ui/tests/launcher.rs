use std::time::Duration;
use vtabs_core::{Intent, Model, Tab};
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
        .map(|id| Tab {
            id,
            title: format!("Tab {id}"),
            ..Tab::default()
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

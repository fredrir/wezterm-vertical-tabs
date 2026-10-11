use std::time::Duration;
use vtabs_core::{Model, Tab};
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

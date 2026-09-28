use super::input_tests::{geometry, logical_key, raw};
use super::*;
use config::keyassignment::KeyAssignment;
use window::Modifiers;

fn command(brief: &str, doc: &str, action: KeyAssignment) -> crate::commands::ExpandedCommand {
    crate::commands::ExpandedCommand {
        brief: brief.to_owned().into(),
        doc: doc.to_owned().into(),
        action,
        keys: Vec::new(),
        menubar: &[],
        icon: None,
    }
}

#[test]
fn filtered_commands_execute_the_original_assignment_and_dismiss_the_palette() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9891);
    adapter.open_command_palette(vec![
        command("New tab", "local", KeyAssignment::SpawnWindow),
        command(
            "New tab",
            "build server",
            KeyAssignment::EmitEvent("build".into()),
        ),
    ]);
    adapter.ui_input(ui::UiInput::Text("build server".into()));
    adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
    assert!(!adapter.app.is_modal());
    let commands = adapter.commands();
    assert!(
        matches!(commands.as_slice(), [Command::RunPaletteCommand(command)]
        if command.action == KeyAssignment::EmitEvent("build".into()))
    );
    adapter.command(app::Command::RunCommand(1));
    assert!(adapter.commands().is_empty());
}

#[test]
fn canceling_commands_does_not_execute_an_assignment() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9892);
    adapter.open_command_palette(vec![command("Quit", "", KeyAssignment::QuitApplication)]);
    adapter.ui_input(ui::UiInput::key(ui::Key::Escape));
    assert!(!adapter.app.is_modal());
    assert!(adapter.commands().is_empty());
}

fn click_recording(adapter: &mut Adapter) {
    adapter.render(geometry(), Instant::now());
    let rect = adapter
        .app
        .ui()
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ui::ElementId::RecordShortcut)
        .unwrap()
        .rect;
    adapter.ui_input(ui::UiInput::PointerDown {
        x: rect.x,
        y: rect.y,
        button: ui::MouseButton::Left,
        modifiers: ui::Modifiers::default(),
    });
    adapter.ui_input(ui::UiInput::PointerUp {
        x: rect.x,
        y: rect.y,
        button: ui::MouseButton::Left,
    });
}

fn results(adapter: &mut Adapter) -> Vec<String> {
    adapter.render(geometry(), Instant::now());
    adapter
        .app
        .ui()
        .hit_regions()
        .iter()
        .filter_map(|hit| match &hit.id {
            ui::ElementId::Menu(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn recording_captures_raw_keys_before_shortcuts_clipboard_or_host_bindings() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9894);
    let bindings = [
        (KeyCode::Char('p'), Modifiers::SUPER),
        (KeyCode::Char('t'), Modifiers::SUPER),
        (KeyCode::Char('v'), Modifiers::CTRL),
        (KeyCode::Char('c'), Modifiers::CTRL | Modifiers::SHIFT),
        (KeyCode::Function(12), Modifiers::ALT),
        (KeyCode::LeftArrow, Modifiers::CTRL | Modifiers::ALT),
        (KeyCode::Char('\r'), Modifiers::NONE),
        (KeyCode::Char('\t'), Modifiers::CTRL),
        (KeyCode::Char('\u{7f}'), Modifiers::NONE),
        (KeyCode::Char(' '), Modifiers::SUPER),
    ];
    adapter.app.ui_mut().open_commands(
        bindings
            .iter()
            .enumerate()
            .map(|(index, (key, mods))| ui::CommandEntry {
                label: format!("Command {index}"),
                shortcuts: vec![commands::shortcut_label(key, *mods)],
                description: String::new(),
            })
            .collect(),
    );
    click_recording(&mut adapter);
    for (index, (key, mods)) in bindings.iter().enumerate() {
        assert!(adapter.input(Input::RawKey(&raw(key.clone(), *mods, true))));
        assert_eq!(results(&mut adapter), [format!("command/{index}")]);
        assert!(adapter.input(Input::RawKey(&raw(
            KeyCode::LeftShift,
            Modifiers::SHIFT,
            true
        ))));
        assert!(adapter.input(Input::RawKey(&raw(key.clone(), *mods, false))));
        assert_eq!(results(&mut adapter), [format!("command/{index}")]);
        assert!(adapter.commands().is_empty());
        assert!(adapter.app.ui().recording_shortcut());
    }
    // Escape leaves recording and keeps the captured shortcut as the query, so the
    // matching command stays listed under the normal search.
    assert!(adapter.input(Input::RawKey(&raw(
        KeyCode::Char('\u{1b}'),
        Modifiers::NONE,
        true
    ))));
    assert!(!adapter.app.ui().recording_shortcut());
    assert_eq!(results(&mut adapter), ["command/9"]);
    // A second Escape now dismisses the palette.
    assert!(adapter.input(Input::Key(&logical_key(
        KeyCode::Char('\u{1b}'),
        Modifiers::NONE
    ))));
    assert!(!adapter.app.is_modal());
}

#[test]
fn recording_finds_alternate_configured_bindings_and_logical_control_keys() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9895);
    adapter.open_command_palette(vec![
        command(
            "Copy",
            "",
            KeyAssignment::CopyTo(config::keyassignment::ClipboardCopyDestination::Clipboard),
        ),
        command("Quit", "", KeyAssignment::QuitApplication),
    ]);
    click_recording(&mut adapter);
    for (key, mods) in [
        (
            KeyCode::Physical(window::PhysKeyCode::C),
            Modifiers::CTRL | Modifiers::SHIFT,
        ),
        (KeyCode::Char('C'), Modifiers::CTRL),
        (KeyCode::Char('\u{3}'), Modifiers::CTRL | Modifiers::SHIFT),
    ] {
        assert!(adapter.input(Input::Key(&logical_key(key, mods))));
        assert_eq!(results(&mut adapter), ["command/0"]);
        assert!(adapter.commands().is_empty());
    }
    assert!(adapter.input(Input::Key(&logical_key(
        KeyCode::Char('c'),
        Modifiers::CTRL
    ))));
    assert!(results(&mut adapter).is_empty());
}

#[test]
fn recording_cmd_digit_filters_commands_without_activating_the_tab() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9896);
    adapter
        .app
        .update(app::HostSnapshot {
            revision: 1,
            tabs: [1, 2]
                .iter()
                .copied()
                .map(|id| core::Tab {
                    id,
                    title: format!("Tab {id}"),
                    ..Default::default()
                })
                .collect(),
            active_tab: Some(2),
            metrics: app::Metrics::default(),
            focused: true,
            configuration_epoch: 0,
        })
        .unwrap();
    adapter.open_command_palette(vec![
        command("First tab", "", KeyAssignment::ActivateTab(0)),
        command("Second tab", "", KeyAssignment::ActivateTab(1)),
    ]);
    click_recording(&mut adapter);
    for key in [
        KeyCode::Char('1'),
        KeyCode::Physical(window::PhysKeyCode::K1),
    ] {
        assert!(adapter.input(Input::RawKey(&raw(key, Modifiers::SUPER, true))));
        assert_eq!(results(&mut adapter), ["command/0"]);
        assert_eq!(adapter.app.model().selected_tab, Some(2));
        assert!(adapter.commands().is_empty());
        assert!(adapter.app.ui().recording_shortcut());
    }
    click_recording(&mut adapter);
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('1'),
        Modifiers::SUPER,
        true,
    )));
    assert_eq!(adapter.app.model().selected_tab, Some(1));
}

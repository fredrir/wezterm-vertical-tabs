use super::*;
use config::keyassignment::KeyAssignment;

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

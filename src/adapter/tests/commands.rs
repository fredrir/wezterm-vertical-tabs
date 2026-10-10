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

fn launcher_args(
    flags: config::keyassignment::LauncherFlags,
) -> crate::overlay::launcher::LauncherArgs {
    crate::overlay::launcher::LauncherArgs {
        flags,
        domains: Vec::new(),
        tabs: Vec::new(),
        domain_id_of_current_tab: 0,
        active_workspace: "current".into(),
        workspaces: vec!["current".into(), "other".into()],
    }
}

fn launcher_action(
    flags: config::keyassignment::LauncherFlags,
) -> config::keyassignment::LauncherActionArgs {
    config::keyassignment::LauncherActionArgs {
        flags,
        title: Some("My tools".into()),
        help_text: Some("Choose a tool".into()),
        fuzzy_help_text: Some("Find a tool".into()),
        alphabet: Some("xy".into()),
    }
}

#[test]
fn launcher_flags_scope_entries() {
    use config::keyassignment::LauncherFlags as F;
    let config = config::ConfigHandle::default_config();
    assert!(
        launcher_args(F::FUZZY)
            .build_entries(&config, 0)
            .entries
            .is_empty()
    );
    let entries = launcher_args(F::WORKSPACES)
        .build_entries(&config, 0)
        .entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].action,
        KeyAssignment::SwitchToWorkspace {
            name: Some("other".into()),
            spawn: None
        }
    );
    assert_eq!(
        entries[1].action,
        KeyAssignment::SwitchToWorkspace {
            name: None,
            spawn: None
        }
    );
    for flag in [F::KEY_ASSIGNMENTS, F::COMMANDS] {
        let entries = launcher_args(flag).build_entries(&config, 0).entries;
        assert!(!entries.is_empty());
        assert!(!entries.iter().any(|entry| matches!(
            entry.action,
            KeyAssignment::ActivateTab(_) | KeyAssignment::ActivateTabRelative(_)
        )));
    }
}

fn launch_menu(
    commands: &[config::keyassignment::SpawnCommand],
) -> crate::overlay::launcher::LauncherEntries {
    crate::overlay::launcher::LauncherEntries {
        entries: commands
            .iter()
            .map(|command| crate::overlay::launcher::Entry {
                label: command.label.clone().unwrap(),
                action: KeyAssignment::SpawnCommandInNewTab(command.clone()),
                tab_id: None,
            })
            .collect(),
        selected: 0,
    }
}

#[test]
fn launcher_keeps_domain_preselection_and_stable_tab_identity() {
    use crate::overlay::launcher::{LauncherDomainEntry, LauncherTabEntry};
    use config::keyassignment::{LauncherFlags as F, SpawnCommand, SpawnTabDomain};
    let mut args = launcher_args(F::DOMAINS | F::TABS);
    args.domains = vec![
        LauncherDomainEntry {
            domain_id: 1,
            name: "remote".into(),
            label: "Remote".into(),
            state: mux::domain::DomainState::Detached,
        },
        LauncherDomainEntry {
            domain_id: 2,
            name: "local".into(),
            label: "Local".into(),
            state: mux::domain::DomainState::Attached,
        },
    ];
    args.domain_id_of_current_tab = 2;
    args.tabs = vec![LauncherTabEntry {
        title: "Shell".into(),
        tab_id: 42,
        pane_count: Some(1),
    }];
    let entries = args.build_entries(&config::ConfigHandle::default_config(), 0);
    assert_eq!(entries.selected, 1);
    assert_eq!(
        entries.entries[0].action,
        KeyAssignment::AttachDomain("remote".into())
    );
    assert_eq!(
        entries.entries[1].action,
        KeyAssignment::SpawnCommandInNewTab(SpawnCommand {
            domain: SpawnTabDomain::DomainName("local".into()),
            ..SpawnCommand::default()
        })
    );
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9897);
    adapter.open_launcher(launcher_action(args.flags), entries);
    adapter.ui_input(ui::UiInput::key(ui::Key::Down));
    adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
    assert!(
        matches!(adapter.commands().as_slice(), [Command::RunLauncherEntry(entry)] if entry.tab_id == Some(42))
    );
}

#[test]
fn launcher_uses_native_fuzzy_matching_and_executes_the_filtered_entry() {
    use config::keyassignment::{LauncherFlags as F, SpawnCommand};
    config::designate_this_as_the_main_thread();
    let commands = vec![
        SpawnCommand {
            label: Some("Zulu shell".into()),
            ..SpawnCommand::default()
        },
        SpawnCommand {
            label: Some("Alpha build server".into()),
            args: Some(vec!["build".into()]),
            ..SpawnCommand::default()
        },
    ];
    let flags = F::LAUNCH_MENU_ITEMS | F::FUZZY;
    let mut adapter = Adapter::new(9898);
    adapter.open_launcher(launcher_action(flags), launch_menu(&commands));
    assert_eq!(
        results(&mut adapter),
        ["command/0", "command/1"],
        "Lua entry order is preserved"
    );
    adapter.ui_input(ui::UiInput::Text("absv".into()));
    assert_eq!(results(&mut adapter), ["command/1"]);
    adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
    assert!(!adapter.app.is_modal());
    assert!(
        matches!(adapter.commands().as_slice(), [Command::RunLauncherEntry(entry)] if entry.action == KeyAssignment::SpawnCommandInNewTab(commands[1].clone()))
    );
    adapter.command(app::Command::RunCommand(1));
    assert!(adapter.commands().is_empty());
}

#[test]
fn launcher_honors_custom_alphabet_and_can_enter_and_leave_filtering() {
    use config::keyassignment::{LauncherFlags as F, SpawnCommand};
    config::designate_this_as_the_main_thread();
    let commands = vec![
        SpawnCommand {
            label: Some("First".into()),
            ..SpawnCommand::default()
        },
        SpawnCommand {
            label: Some("Second".into()),
            ..SpawnCommand::default()
        },
        SpawnCommand {
            label: Some("Third".into()),
            ..SpawnCommand::default()
        },
    ];
    let flags = F::LAUNCH_MENU_ITEMS;
    let mut adapter = Adapter::new(9899);
    adapter.open_launcher(launcher_action(flags), launch_menu(&commands));
    results(&mut adapter);
    assert!(!adapter.text_input_active());
    adapter.input(Input::Key(&logical_key(
        KeyCode::Char('/'),
        Modifiers::NONE,
    )));
    assert!(adapter.text_input_active());
    adapter.input(Input::Key(&logical_key(
        KeyCode::Char('\u{8}'),
        Modifiers::NONE,
    )));
    assert!(!adapter.text_input_active());
    adapter.input(Input::Key(&logical_key(
        KeyCode::Char('y'),
        Modifiers::NONE,
    )));
    assert!(
        adapter.commands().is_empty(),
        "wait for the second label character"
    );
    adapter.input(Input::Key(&logical_key(
        KeyCode::Char('x'),
        Modifiers::NONE,
    )));
    assert!(
        matches!(adapter.commands().as_slice(), [Command::RunLauncherEntry(entry)] if entry.label == "Second")
    );
}

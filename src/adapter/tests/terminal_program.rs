use super::super::support::raw;
use super::*;
use window::KeyCode;

#[test]
fn terminal_spaces_follow_selection_and_keep_their_identity_when_renamed() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9932);
    let home = adapter.selected_space().to_owned();
    adapter.dispatch(core::Intent::CreateSpace {
        name: "Work".into(),
    });
    let work = adapter.selected_space().to_owned();
    assert_ne!(home, work);
    assert!(adapter.space_exists(&home));
    adapter.dispatch(core::Intent::RenameSpace {
        id: work.clone(),
        name: "Renamed".into(),
    });
    assert_eq!(adapter.selected_space(), work);
    adapter.dispatch(core::Intent::DeleteSpace {
        id: work.clone(),
        destination: Some(home.clone()),
    });
    assert_eq!(adapter.selected_space(), home);
    assert!(!adapter.space_exists(&work));
}

#[test]
fn quick_programs_toggle_by_their_command() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9917);
    adapter.app.ui_mut().open_settings();
    let program = serde_json::json!({"QuickTerminal": {
        "args": ["lazygit"],
        "set_environment_variables": {"THEME": "dark"},
        "width": 0.9,
    }});
    adapter.message(Message::Action(serde_json::from_value(program).unwrap()));
    let commands = adapter.commands();
    let [Command::ToggleTerminalProgram(program)] = commands.as_slice() else {
        panic!("expected a quick program");
    };
    assert_eq!(program.spawn.args, Some(vec!["lazygit".to_string()]));
    assert_eq!(program.spawn.cwd, None);
    assert_eq!(
        program.spawn.set_environment_variables,
        HashMap::from([("THEME".to_string(), "dark".to_string())])
    );
    assert_eq!(
        program.spawn.domain,
        config::keyassignment::SpawnTabDomain::CurrentPaneDomain
    );
    assert_eq!((program.width, program.height), (Some(0.9), None));
    assert!(
        adapter.content_page(),
        "settings stay open beneath the program"
    );

    let toggle = raw(KeyCode::Char('`'), window::Modifiers::CTRL, true);
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&toggle), HideOnEscape::Auto),
        TerminalKey::Handled
    );
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::ToggleTerminalOverlay]
    ));
    let close = raw(KeyCode::Char('w'), window::Modifiers::SUPER, true);
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&close), HideOnEscape::Auto),
        TerminalKey::Handled
    );
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::HideTerminalOverlay]
    ));
}

#[test]
fn quick_terminal_configuration_maps_settings_and_preserves_defaults() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9931);
    let defaults = adapter.quick_terminal_config();
    assert_eq!(defaults.size, (0.75, 0.75));
    assert_eq!(defaults.opacity, 1.);
    assert_eq!(defaults.blur, 0.);
    assert!(defaults.preserve_shell);
    assert!(!defaults.auto_hide);
    assert_eq!(defaults.hide_on_escape, HideOnEscape::Auto);
    assert_eq!(defaults.padding, TerminalPadding::default());
    assert_eq!(defaults.border, TerminalBorder::Palette);
    assert!(defaults.background_color.is_none());
    for (key, value) in [
        ("quick_terminal_opacity", serde_json::json!(0.4)),
        ("quick_terminal_blur", serde_json::json!(12)),
        ("quick_terminal_border_color", serde_json::json!("#ff0000")),
        (
            "quick_terminal_background_color",
            serde_json::json!("#001122"),
        ),
        ("quick_terminal_auto_hide", serde_json::json!(true)),
        ("quick_terminal_preserve_shell", serde_json::json!(false)),
        (
            "quick_terminal_window_padding",
            serde_json::json!({"left": 4, "top": "1cell", "bottom": "50%"}),
        ),
    ] {
        adapter
            .app
            .dispatch(core::Intent::SetSetting {
                key: key.into(),
                value,
            })
            .unwrap();
    }
    let translucent = adapter.quick_terminal_config();
    assert_eq!((translucent.opacity, translucent.blur), (0.4, 12.));
    assert!(translucent.auto_hide && !translucent.preserve_shell);
    assert_eq!(
        translucent.border,
        TerminalBorder::Color(window::color::LinearRgba(1., 0., 0., 1.))
    );
    assert!(translucent.background_color.is_some());
    assert_eq!(
        translucent.padding,
        TerminalPadding {
            left: Some(config::Dimension::Pixels(4.)),
            top: Some(config::Dimension::Cells(1.)),
            right: None,
            bottom: Some(config::Dimension::Percent(0.5)),
        }
    );
    adapter
        .app
        .dispatch(core::Intent::SetSetting {
            key: "quick_terminal_border_color".into(),
            value: serde_json::json!("none"),
        })
        .unwrap();
    assert_eq!(adapter.quick_terminal_config().border, TerminalBorder::None);
}

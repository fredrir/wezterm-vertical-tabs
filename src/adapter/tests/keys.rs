use super::super::support::{copied_text, geometry, logical_key, paste_token, raw};
use super::*;
use window::KeyCode;

#[test]
fn shortcuts_precede_bindings_and_do_not_fire_on_release() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9841);
    let mods = if cfg!(target_os = "macos") {
        window::Modifiers::SUPER
    } else {
        window::Modifiers::CTRL | window::Modifiers::SHIFT
    };
    assert!(!adapter.keyboard_focus());
    assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char(','), mods, true))));
    assert!(adapter.content_page());
    assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char(','), mods, false))));
    assert!(adapter.content_page());
    adapter.render(geometry(), Instant::now());
    assert_eq!(adapter.surface.columns, 132);
    assert_eq!(adapter.reservation().width, 256.);
    assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char(','), mods, true))));
    assert!(!adapter.content_page());
    assert!(!adapter.input(Input::RawKey(&raw(
        KeyCode::Char('t'),
        window::Modifiers::CTRL,
        true
    ))));
}
#[test]
fn shifted_and_control_encoded_keys_resolve_shortcuts() {
    let mods = ui::Modifiers {
        control: true,
        shift: true,
        ..Default::default()
    };
    assert_eq!(
        shortcut_key(&KeyCode::Char('<'), mods),
        Some(ui::Key::Character(','))
    );
    assert_eq!(
        shortcut_key(&KeyCode::Char('!'), mods),
        Some(ui::Key::Character('1'))
    );
    assert_eq!(
        logical_shortcut_key(&KeyCode::Char('\u{7}'), mods),
        Some(ui::Key::Character('g'))
    );
    assert_eq!(shortcut_key(&KeyCode::Char('\t'), mods), Some(ui::Key::Tab));
    for (shifted, base) in [('+', '='), ('?', '/'), ('_', '-'), ('~', '`')] {
        assert_eq!(
            shortcut_key(&KeyCode::Char(shifted), mods),
            shortcut_key(&KeyCode::Char(base), mods)
        );
    }
}

#[test]
fn command_palette_shortcut_precedes_bindings_and_only_opens_on_key_down() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9893);
    let mods = if cfg!(target_os = "macos") {
        window::Modifiers::SUPER
    } else {
        window::Modifiers::CTRL | window::Modifiers::SHIFT
    };
    assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('p'), mods, true))));
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::OpenCommandPalette]
    ));
    assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('p'), mods, false))));
    assert!(adapter.commands().is_empty());
    assert!(!adapter.input(Input::RawKey(&raw(
        KeyCode::Char('p'),
        window::Modifiers::CTRL,
        true
    ))));
    adapter
        .app
        .config(serde_json::json!({"settings": {"keyboard_shortcuts": false}}))
        .unwrap();
    assert!(!adapter.input(Input::RawKey(&raw(KeyCode::Char('p'), mods, true))));
    assert!(adapter.commands().is_empty());
}

#[test]
fn editors_decode_clipboard_control_bytes_with_global_shortcuts_disabled() {
    config::designate_this_as_the_main_thread();
    for open_search in [false, true] {
        let mut adapter = Adapter::new(9843);
        adapter
            .app
            .config(serde_json::json!({"settings":{"keyboard_shortcuts":false}}))
            .unwrap();
        if open_search {
            adapter.app.open_tab_navigator();
        } else {
            adapter.app.open_create_space();
        }
        adapter.ui_input(ui::UiInput::Text("Copy 界".into()));
        assert!(adapter.input(Input::Key(&logical_key(
            KeyCode::Char('\u{1}'),
            window::Modifiers::CTRL
        ))));
        assert!(adapter.input(Input::Key(&logical_key(
            KeyCode::Char('\u{3}'),
            window::Modifiers::CTRL
        ))));
        assert_eq!(copied_text(&mut adapter), "Copy 界");
        assert!(adapter.input(Input::Key(&logical_key(
            KeyCode::Char('\u{16}'),
            window::Modifiers::CTRL
        ))));
        let token = paste_token(&mut adapter);
        adapter.message(Message::Paste {
            text: Some("Pasted 界".into()),
            token,
        });
        adapter.input(Input::Key(&logical_key(
            KeyCode::Char('\u{1}'),
            window::Modifiers::CTRL,
        )));
        adapter.input(Input::Key(&logical_key(
            KeyCode::Char('\u{3}'),
            window::Modifiers::CTRL,
        )));
        assert_eq!(copied_text(&mut adapter), "Pasted 界");
    }
}

#[test]
fn raw_clipboard_commands_are_scoped_to_text_focus() {
    config::designate_this_as_the_main_thread();
    for mods in [
        window::Modifiers::CTRL,
        window::Modifiers::CTRL | window::Modifiers::SHIFT,
        window::Modifiers::SUPER,
    ] {
        let mut adapter = Adapter::new(9844);
        assert!(!adapter.input(Input::RawKey(&raw(KeyCode::Char('c'), mods, true))));
        adapter.app.open_create_space();
        adapter.ui_input(ui::UiInput::Text("copy".into()));
        assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('a'), mods, true))));
        assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('c'), mods, true))));
        assert_eq!(copied_text(&mut adapter), "copy");
        assert!(adapter.input(Input::RawKey(&raw(KeyCode::Char('c'), mods, false))));
        assert!(adapter.commands().is_empty());
    }
}

#[test]
fn composed_release_does_not_duplicate_text_and_caret_is_visible() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9842);
    adapter.app.ui_mut().open_create_space();
    let key = window::KeyEvent {
        key: KeyCode::Composed("Hello".into()),
        modifiers: window::Modifiers::NONE,
        leds: Default::default(),
        repeat_count: 1,
        key_is_down: true,
        raw: None,
        #[cfg(windows)]
        win32_uni_char: None,
    };
    assert!(adapter.input(Input::Key(&key)));
    let mut release = key.clone();
    release.key_is_down = false;
    assert!(adapter.input(Input::Key(&release)));
    adapter.render(geometry(), Instant::now());
    let text = adapter
        .app
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Hello"));
    assert!(!text.contains("HelloHello"));
    assert!(
        adapter
            .primitives
            .iter()
            .any(|shape| shape.bounds.width == 0.12)
    );
    adapter.pointer_captured = true;
    adapter.input(Input::Focus(false));
    assert!(!adapter.pointer_captured);
    adapter.render(geometry(), Instant::now());
    assert!(
        !adapter
            .primitives
            .iter()
            .any(|shape| shape.bounds.width == 0.12)
    );
}

#[test]
fn processed_text_and_clipboard_keep_their_identity_with_raw_metadata() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9930);
    adapter
        .app
        .config(serde_json::json!({"settings":{"keyboard_shortcuts":false}}))
        .unwrap();
    adapter.app.open_create_space();
    assert!(!adapter.input(Input::RawKey(&raw(
        KeyCode::Char('z'),
        window::Modifiers::NONE,
        true,
    ))));
    let mut text = logical_key(KeyCode::Char('a'), window::Modifiers::NONE);
    text.raw = Some(raw(KeyCode::Char('q'), window::Modifiers::NONE, true));
    assert!(adapter.input(Input::Key(&text)));

    // IME commits use processed modifiers even when raw metadata still has Ctrl.
    let mut composed = logical_key(KeyCode::Composed("界".into()), window::Modifiers::NONE);
    composed.raw = Some(raw(KeyCode::Char('q'), window::Modifiers::CTRL, true));
    assert!(adapter.input(Input::Key(&composed)));
    composed.key_is_down = false;
    assert!(adapter.input(Input::Key(&composed)));
    composed.key_is_down = true;
    composed.modifiers = window::Modifiers::CTRL;
    composed.raw.as_mut().unwrap().modifiers = window::Modifiers::NONE;
    assert!(!adapter.input(Input::Key(&composed)));

    // Clipboard identity uses the delivered logical key and raw modifiers.
    for code in ['\u{1}', '\u{3}'] {
        let mut event = logical_key(KeyCode::Char(code), window::Modifiers::NONE);
        event.raw = Some(raw(KeyCode::Char('q'), window::Modifiers::CTRL, true));
        assert!(adapter.input(Input::Key(&event)));
    }
    assert_eq!(copied_text(&mut adapter), "a界");
}

#[test]
fn unclaimed_command_chords_reach_host_bindings_while_search_is_open() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9860);
    adapter.app.open_tab_navigator();
    for mods in [
        window::Modifiers::SUPER,
        window::Modifiers::SUPER | window::Modifiers::SHIFT,
        window::Modifiers::CTRL,
    ] {
        assert!(!adapter.input(Input::Key(&logical_key(KeyCode::Char(' '), mods))));
        assert!(!adapter.input(Input::Key(&logical_key(
            KeyCode::Composed(" ".into()),
            mods
        ))));
    }
    assert!(adapter.input(Input::Key(&logical_key(
        KeyCode::Char(' '),
        window::Modifiers::NONE
    ))));
    assert!(adapter.app.ui().is_modal());
}

#[test]
fn saved_shortcuts_are_claimed_without_sidebar_focus_and_release_does_not_execute() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9910);
    adapter
        .app
        .dispatch(core::Intent::SetSetting {
            key: "keybinds".into(),
            value: serde_json::json!({"commands": ["Ctrl+Alt+p", "F12"]}),
        })
        .unwrap();
    assert!(!adapter.keyboard_focus());
    let old = if cfg!(target_os = "macos") {
        window::Modifiers::SUPER
    } else {
        window::Modifiers::CTRL | window::Modifiers::SHIFT
    };
    assert!(!adapter.input(Input::RawKey(&raw(KeyCode::Char('p'), old, true))));
    for key in [
        KeyCode::Char('p'),
        KeyCode::Physical(window::PhysKeyCode::P),
    ] {
        let mods = window::Modifiers::CTRL | window::Modifiers::ALT;
        assert!(adapter.input(Input::RawKey(&raw(key.clone(), mods, true))));
        assert!(matches!(
            adapter.commands().as_slice(),
            [Command::OpenCommandPalette]
        ));
        assert!(adapter.input(Input::RawKey(&raw(key, mods, false))));
        assert!(adapter.commands().is_empty());
    }
    assert!(adapter.input(Input::Key(&logical_key(
        KeyCode::Function(12),
        window::Modifiers::NONE
    ))));
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::OpenCommandPalette]
    ));
}

#[test]
fn settings_recorder_saves_a_physical_key_without_executing_it() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9911);
    adapter.app.ui_mut().open_settings();

    click(
        &mut adapter,
        ui::ElementId::SettingsCategory("keybinds".into()),
    );
    // Search keeps this action visible even in a short settings viewport.
    click(&mut adapter, ui::ElementId::SettingsSearch);
    adapter.ui_input(ui::UiInput::Text("Command palette".into()));
    click(
        &mut adapter,
        ui::ElementId::Setting("shortcut.commands".into()),
    );
    assert!(adapter.app.ui().recording_keybind());
    assert!(adapter.input(Input::RawKey(&raw(
        KeyCode::Physical(window::PhysKeyCode::F12),
        window::Modifiers::NONE,
        true
    ))));
    assert!(adapter.commands().is_empty());
    assert!(
        core::keybinds::bindings(&adapter.app.model().settings, "commands").contains(&"F12".into())
    );
    // Escape leaves the launcher, which records until then, without reaching the shell.
    let escape = raw(KeyCode::Char('\u{1b}'), window::Modifiers::NONE, true);
    assert!(adapter.input(Input::RawKey(&escape)));
    assert!(!adapter.app.ui().recording_shortcut());
    assert!(adapter.content_page());
}

fn click(adapter: &mut Adapter, id: ui::ElementId) {
    adapter.render(geometry(), Instant::now());
    let rect = adapter
        .app
        .ui()
        .hit_regions()
        .iter()
        .find(|hit| hit.id == id)
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

fn control_shortcut_events() -> Vec<(window::KeyEvent, &'static str)> {
    let mut events = Vec::new();
    for (key, physical, expected) in [
        (
            KeyCode::Char('\u{8}'),
            window::PhysKeyCode::Backspace,
            "Ctrl+Backspace",
        ),
        (
            KeyCode::Physical(window::PhysKeyCode::Backspace),
            window::PhysKeyCode::Backspace,
            "Ctrl+Backspace",
        ),
        (KeyCode::Char('h'), window::PhysKeyCode::H, "Ctrl+h"),
    ] {
        let mut event = logical_key(KeyCode::Char('\u{8}'), window::Modifiers::CTRL);
        let mut raw = raw(key, window::Modifiers::CTRL, true);
        raw.phys_code = Some(physical);
        event.raw = Some(raw);
        events.push((event, expected));
    }
    events.push((
        logical_key(
            KeyCode::Physical(window::PhysKeyCode::Backspace),
            window::Modifiers::CTRL,
        ),
        "Ctrl+Backspace",
    ));
    events.push((
        logical_key(KeyCode::Char('\u{8}'), window::Modifiers::CTRL),
        "Ctrl+h",
    ));
    for (key, expected) in [
        (KeyCode::Char('\t'), "Ctrl+Alt+Tab"),
        (KeyCode::Char('\r'), "Ctrl+Alt+Enter"),
    ] {
        events.push((
            logical_key(key, window::Modifiers::CTRL | window::Modifiers::ALT),
            expected,
        ));
    }
    events
}

#[test]
fn control_backspace_and_h_dispatch_distinct_shortcuts() {
    config::designate_this_as_the_main_thread();
    for (event, expected) in control_shortcut_events() {
        for raw_path in [false, true] {
            if raw_path && event.raw.is_none() {
                continue;
            }
            let mut adapter = Adapter::new(9912);
            adapter.app.dispatch(core::Intent::SetSetting {
                key: "keybinds".into(),
                value: serde_json::json!({"commands": [if expected == "Ctrl+h" { "Ctrl+Backspace" } else { expected }], "settings": ["Ctrl+h"]}),
            }).unwrap();
            let input = if raw_path {
                Input::RawKey(event.raw.as_ref().unwrap())
            } else {
                Input::Key(&event)
            };
            assert!(adapter.input(input));
            if expected != "Ctrl+h" {
                assert!(matches!(
                    adapter.commands().as_slice(),
                    [Command::OpenCommandPalette]
                ));
                assert!(!adapter.content_page());
            } else {
                assert!(adapter.commands().is_empty());
                assert!(adapter.content_page());
            }
        }
    }
}

#[test]
fn settings_recorder_keeps_control_backspace_and_h_distinct() {
    config::designate_this_as_the_main_thread();
    for (event, expected) in control_shortcut_events() {
        for raw_path in [false, true] {
            if raw_path && event.raw.is_none() {
                continue;
            }
            let mut adapter = Adapter::new(9913);
            adapter.app.ui_mut().open_settings();
            click(
                &mut adapter,
                ui::ElementId::SettingsCategory("keybinds".into()),
            );
            click(&mut adapter, ui::ElementId::SettingsSearch);
            adapter.ui_input(ui::UiInput::Text("Command palette".into()));
            click(
                &mut adapter,
                ui::ElementId::Setting("shortcut.commands".into()),
            );
            assert!(adapter.app.ui().recording_keybind());
            let input = if raw_path {
                Input::RawKey(event.raw.as_ref().unwrap())
            } else {
                Input::Key(&event)
            };
            assert!(adapter.input(input));
            assert!(adapter.commands().is_empty());
            assert!(
                core::keybinds::bindings(&adapter.app.model().settings, "commands")
                    .contains(&expected.into()),
                "failed to record {}, raw={}",
                expected,
                raw_path
            );
        }
    }
}

#[test]
fn unbound_processed_control_h_does_not_dispatch_control_backspace() {
    config::designate_this_as_the_main_thread();
    for modal in [false, true] {
        let mut adapter = Adapter::new(9914);
        adapter
            .app
            .dispatch(core::Intent::SetSetting {
                key: "keybinds".into(),
                value: serde_json::json!({"commands": ["Ctrl+Backspace"]}),
            })
            .unwrap();
        if modal {
            adapter.app.open_tab_navigator();
        }
        assert!(!adapter.input(Input::Key(&logical_key(
            KeyCode::Char('\u{8}'),
            window::Modifiers::CTRL
        ))));
        assert!(adapter.commands().is_empty());
    }
}

#[test]
fn quick_terminal_shortcut_opens_over_settings_and_leaves_terminal_keys_alone() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9915);
    adapter.app.ui_mut().open_settings();
    let toggle = raw(KeyCode::Char('`'), window::Modifiers::CTRL, true);
    assert!(adapter.input(Input::RawKey(&toggle)));
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::ToggleTerminalOverlay]
    ));
    assert!(adapter.content_page());
    assert!(!adapter.text_input_active());
    assert_eq!(
        adapter.terminal_input(
            Input::RawKey(&raw(KeyCode::Char('`'), window::Modifiers::CTRL, false)),
            HideOnEscape::Auto
        ),
        TerminalKey::Handled
    );
    assert!(adapter.commands().is_empty());
    for (key, mods) in [
        (KeyCode::Char('a'), window::Modifiers::NONE),
        (KeyCode::Char('c'), window::Modifiers::CTRL),
        (KeyCode::Char('t'), window::Modifiers::SUPER),
    ] {
        assert_eq!(
            adapter.terminal_input(Input::RawKey(&raw(key, mods, true)), HideOnEscape::Auto),
            TerminalKey::Pass
        );
    }
    assert!(adapter.commands().is_empty());
    // By default the host judges whether anything runs before Escape hides the overlay.
    let escape = raw(KeyCode::Char('\u{1b}'), window::Modifiers::NONE, true);
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&escape), HideOnEscape::Auto),
        TerminalKey::Escape
    );
    let release = raw(KeyCode::Char('\u{1b}'), window::Modifiers::NONE, false);
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&release), HideOnEscape::Auto),
        TerminalKey::Pass
    );
    assert!(adapter.commands().is_empty());
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&toggle), HideOnEscape::Auto),
        TerminalKey::Handled
    );
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::ToggleTerminalOverlay]
    ));
}

#[test]
fn escape_hides_the_quick_terminal_as_configured() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9918);
    let escape = |down| raw(KeyCode::Char('\u{1b}'), window::Modifiers::NONE, down);
    let shifted = raw(KeyCode::Char('\u{1b}'), window::Modifiers::SHIFT, true);
    for (mode, press, release) in [
        (HideOnEscape::Never, TerminalKey::Pass, TerminalKey::Pass),
        (
            HideOnEscape::Always,
            TerminalKey::Handled,
            TerminalKey::Handled,
        ),
        (HideOnEscape::Auto, TerminalKey::Escape, TerminalKey::Pass),
    ] {
        assert_eq!(
            adapter.terminal_input(Input::RawKey(&escape(true)), mode),
            press
        );
        let hides = matches!(
            adapter.commands().as_slice(),
            [Command::HideTerminalOverlay]
        );
        assert_eq!(hides, mode == HideOnEscape::Always);
        assert_eq!(
            adapter.terminal_input(Input::RawKey(&escape(false)), mode),
            release
        );
        // Escape keeps its terminal meaning when modified.
        assert_eq!(
            adapter.terminal_input(Input::RawKey(&shifted), mode),
            TerminalKey::Pass
        );
        assert!(adapter.commands().is_empty());
    }
    for (value, mode) in [
        ("never", HideOnEscape::Never),
        ("always", HideOnEscape::Always),
        ("auto", HideOnEscape::Auto),
    ] {
        adapter
            .app
            .dispatch(core::Intent::SetSetting {
                key: "quick_terminal_hide_on_escape".into(),
                value: serde_json::json!(value),
            })
            .unwrap();
        assert_eq!(adapter.quick_terminal_config().hide_on_escape, mode);
    }
}

#[test]
fn quick_terminal_respects_remapping_and_disabled_shortcuts() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9916);
    adapter
        .app
        .dispatch(core::Intent::SetSetting {
            key: "keybinds".into(),
            value: serde_json::json!({"quick_terminal": ["F12"]}),
        })
        .unwrap();
    let original = raw(KeyCode::Char('`'), window::Modifiers::CTRL, true);
    let replacement = raw(KeyCode::Function(12), window::Modifiers::NONE, true);
    assert!(!adapter.input(Input::RawKey(&original)));
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&original), HideOnEscape::Auto),
        TerminalKey::Pass
    );
    assert!(adapter.input(Input::RawKey(&replacement)));
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::ToggleTerminalOverlay]
    ));
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&replacement), HideOnEscape::Auto),
        TerminalKey::Handled
    );
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::ToggleTerminalOverlay]
    ));
    adapter
        .app
        .dispatch(core::Intent::SetSetting {
            key: "keyboard_shortcuts".into(),
            value: serde_json::json!(false),
        })
        .unwrap();
    assert!(!adapter.input(Input::RawKey(&replacement)));
    assert_eq!(
        adapter.terminal_input(Input::RawKey(&replacement), HideOnEscape::Auto),
        TerminalKey::Pass
    );
    adapter.message(Message::Action(core::Action::Ui(
        core::UiAction::QuickTerminal,
    )));
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::ToggleTerminalOverlay]
    ));
}

#[test]
fn raw_metadata_selects_shortcuts_but_processed_press_state_controls_delivery() {
    config::designate_this_as_the_main_thread();
    for down in [false, true] {
        let mut event = logical_key(
            KeyCode::Composed("ignored".into()),
            window::Modifiers::SUPER,
        );
        event.key_is_down = down;
        event.raw = Some(raw(
            KeyCode::Physical(window::PhysKeyCode::F12),
            window::Modifiers::CTRL,
            !down,
        ));

        let mut adapter = Adapter::new(9931);
        adapter
            .app
            .dispatch(core::Intent::SetSetting {
                key: "keybinds".into(),
                value: serde_json::json!({"commands": ["Ctrl+F12"]}),
            })
            .unwrap();
        assert!(adapter.input(Input::Key(&event)));
        assert_eq!(
            matches!(adapter.commands().as_slice(), [Command::OpenCommandPalette]),
            down
        );

        adapter
            .app
            .dispatch(core::Intent::SetSetting {
                key: "keybinds".into(),
                value: serde_json::json!({"quick_terminal": ["Ctrl+F12"]}),
            })
            .unwrap();
        assert_eq!(
            adapter.terminal_input(Input::Key(&event), HideOnEscape::Auto),
            TerminalKey::Handled
        );
        assert_eq!(
            matches!(
                adapter.commands().as_slice(),
                [Command::ToggleTerminalOverlay]
            ),
            down
        );
    }
}

#[test]
fn palette_recording_keeps_native_raw_labels_for_processed_events() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9932);
    let native_key = KeyCode::Char('<');
    let native_modifiers = window::Modifiers::CTRL | window::Modifiers::SHIFT;
    adapter.app.ui_mut().open_commands(vec![ui::CommandEntry {
        label: "Native binding".into(),
        shortcuts: vec![palette::shortcut_label(&native_key, native_modifiers)],
        description: String::new(),
    }]);
    click(&mut adapter, ui::ElementId::RecordShortcut);
    let mut event = logical_key(KeyCode::Composed("ignored".into()), window::Modifiers::NONE);
    event.raw = Some(raw(native_key, native_modifiers, true));
    assert!(adapter.input(Input::Key(&event)));
    assert!(adapter.commands().is_empty());
    adapter.render(geometry(), Instant::now());
    let results = adapter
        .app
        .ui()
        .hit_regions()
        .iter()
        .filter_map(|hit| match &hit.id {
            ui::ElementId::Menu(id) => Some(id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results, ["command/0"]);
    assert!(adapter.app.ui().recording_shortcut());
}

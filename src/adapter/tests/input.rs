use super::*;

fn raw(key: KeyCode, mods: window::Modifiers, down: bool) -> window::RawKeyEvent {
    window::RawKeyEvent {
        key,
        modifiers: mods,
        leds: Default::default(),
        phys_code: None,
        raw_code: 0,
        #[cfg(windows)]
        scan_code: 0,
        repeat_count: 1,
        key_is_down: down,
        handled: window::Handled::new(),
    }
}
fn geometry() -> Geometry {
    Geometry {
        sidebar: Bounds {
            x: 0.,
            y: 0.,
            width: 256.,
            height: 480.,
        },
        content: Bounds {
            x: 256.,
            y: 0.,
            width: 800.,
            height: 480.,
        },
        cell_width: 8.,
        cell_height: 20.,
        dpi: 96.,
        ..Geometry::default()
    }
}
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
        shortcut_key(&KeyCode::Char('\u{7}'), mods),
        Some(ui::Key::Character('g'))
    );
    assert_eq!(shortcut_key(&KeyCode::Char('\t'), mods), Some(ui::Key::Tab));
}

fn logical_key(key: KeyCode, mods: window::Modifiers) -> window::KeyEvent {
    window::KeyEvent {
        key,
        modifiers: mods,
        leds: Default::default(),
        repeat_count: 1,
        key_is_down: true,
        raw: None,
        #[cfg(windows)]
        win32_uni_char: None,
    }
}

fn paste_token(adapter: &mut Adapter) -> u64 {
    adapter
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::Paste(token) => Some(token),
            _ => None,
        })
        .expect("clipboard read request")
}

fn copied_text(adapter: &mut Adapter) -> String {
    adapter
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::Clipboard(text) => Some(text),
            _ => None,
        })
        .expect("clipboard write request")
}

fn editor_command(adapter: &mut Adapter, key: char) {
    assert!(adapter.input(Input::RawKey(&raw(
        KeyCode::Char(key),
        window::Modifiers::CTRL,
        true,
    ))));
}

fn begin_paste(adapter: &mut Adapter) -> u64 {
    editor_command(adapter, 'v');
    paste_token(adapter)
}

fn editor_text(adapter: &mut Adapter) -> String {
    editor_command(adapter, 'a');
    editor_command(adapter, 'c');
    copied_text(adapter)
}

#[test]
fn delayed_paste_precedes_typed_text_and_enter_without_timing_assumptions() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9850);
    adapter.app.open_create_space();
    let token = begin_paste(&mut adapter);
    adapter.ui_input(ui::UiInput::Text(" suffix".into()));
    adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
    adapter.ui_input(ui::UiInput::Text("must not reach another editor".into()));
    assert_eq!(adapter.app.model().spaces.len(), 1);
    adapter.message(serde_json::json!({"paste":"Project","token":token}));
    assert!(
        adapter
            .app
            .model()
            .spaces
            .iter()
            .any(|space| space.name == "Project suffix")
    );
    assert!(adapter.pending_paste.is_none());
    adapter.render(geometry(), Instant::now());
    assert!(!adapter.app.ui().has_overlay());
}

#[test]
fn delayed_pastes_preserve_copy_cut_and_multiple_read_order() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9851);
    adapter.app.open_create_space();
    let first = begin_paste(&mut adapter);
    editor_command(&mut adapter, 'a');
    editor_command(&mut adapter, 'c');
    editor_command(&mut adapter, 'x');
    editor_command(&mut adapter, 'v');
    adapter.ui_input(ui::UiInput::Text(" tail".into()));
    assert!(adapter.commands().is_empty());
    adapter.message(serde_json::json!({"paste":"First","token":first}));
    let commands = adapter.commands();
    assert!(
        matches!(commands.as_slice(), [Command::Clipboard(copy), Command::Clipboard(cut), Command::Paste(_)] if copy == "First" && cut == "First")
    );
    let second = adapter.pending_paste.as_ref().unwrap().token;
    assert_ne!(first, second);
    adapter.message(serde_json::json!({"paste":"stale","token":first}));
    adapter.message(serde_json::json!({"paste":"Second","token":second}));
    assert_eq!(editor_text(&mut adapter), "Second tail");
}

#[test]
fn failed_or_expired_paste_preserves_typing_but_never_submits_the_old_value() {
    config::designate_this_as_the_main_thread();
    for expired in [false, true] {
        let mut adapter = Adapter::new(9852);
        adapter.app.open_create_space();
        adapter.ui_input(ui::UiInput::Text("Old".into()));
        let token = begin_paste(&mut adapter);
        adapter.ui_input(ui::UiInput::Text(" tail".into()));
        adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
        adapter.ui_input(ui::UiInput::key(ui::Key::F10));
        editor_command(&mut adapter, 'x');
        editor_command(&mut adapter, 'v');
        if expired {
            let deadline = adapter.pending_paste.as_ref().unwrap().deadline;
            assert!(adapter.deadline().unwrap() <= deadline);
            adapter.render(geometry(), deadline);
        } else {
            adapter.message(serde_json::json!({"paste_error":"unavailable","token":token}));
            adapter.render(geometry(), Instant::now());
        }
        assert!(adapter.pending_paste.is_none());
        assert!(adapter.commands().is_empty());
        assert_eq!(adapter.app.model().spaces.len(), 1);
        assert!(adapter.app.ui().has_overlay());
        let rendered = adapter
            .app
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Clipboard unavailable"));
        adapter.message(serde_json::json!({"paste":"late","token":token}));
        assert_eq!(editor_text(&mut adapter), "Old tail");
    }
}

#[test]
fn pending_paste_bounds_release_input_and_ignore_late_completion() {
    config::designate_this_as_the_main_thread();
    for bytes in [false, true] {
        let mut adapter = Adapter::new(9853);
        adapter.app.open_create_space();
        let token = begin_paste(&mut adapter);
        if bytes {
            adapter.ui_input(ui::UiInput::Text("x".repeat(MAX_PASTE_INPUT_BYTES + 1)));
        } else {
            for _ in 0..=MAX_PASTE_INPUTS {
                adapter.ui_input(ui::UiInput::key(ui::Key::Character('x')));
            }
        }
        assert!(adapter.pending_paste.is_none());
        adapter.message(serde_json::json!({"paste":"late","token":token}));
        let expected = if bytes {
            ui::TextEditor::MAX_BYTES
        } else {
            MAX_PASTE_INPUTS + 1
        };
        assert_eq!(editor_text(&mut adapter), "x".repeat(expected));
    }
}

#[test]
fn pending_paste_cancels_queued_input_on_escape_and_focus_departure() {
    config::designate_this_as_the_main_thread();
    for focus_loss in [false, true] {
        let mut adapter = Adapter::new(9854);
        adapter.app.open_create_space();
        let token = begin_paste(&mut adapter);
        adapter.ui_input(ui::UiInput::Text("discarded".into()));
        adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
        if focus_loss {
            adapter.input(Input::Focus(false));
            adapter.input(Input::Focus(true));
        } else {
            adapter.ui_input(ui::UiInput::key(ui::Key::Escape));
            adapter.message(serde_json::json!({"action":"create_space"}));
        }
        adapter.ui_input(ui::UiInput::Text("Keep".into()));
        adapter.message(serde_json::json!({"paste":"late","token":token}));
        assert_eq!(editor_text(&mut adapter), "Keep");
        assert_eq!(adapter.app.model().spaces.len(), 1);
    }
}

#[test]
fn pending_paste_precedes_clipboard_context_and_mouse_submit() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9855);
    adapter.app.open_create_space();
    let token = begin_paste(&mut adapter);
    editor_command(&mut adapter, 'a');
    adapter.ui_input(ui::UiInput::key(ui::Key::F10));
    editor_command(&mut adapter, 'c');
    adapter.message(serde_json::json!({"paste":"Project","token":token}));
    assert_eq!(copied_text(&mut adapter), "Project");
    adapter.render(geometry(), Instant::now());
    let submit = adapter
        .app
        .ui()
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ui::ElementId::Submit)
        .unwrap()
        .rect;
    let token = begin_paste(&mut adapter);
    adapter.ui_input(ui::UiInput::PointerDown {
        x: submit.x,
        y: submit.y,
        button: ui::MouseButton::Left,
        modifiers: ui::Modifiers::default(),
    });
    adapter.ui_input(ui::UiInput::PointerUp {
        x: submit.x,
        y: submit.y,
        button: ui::MouseButton::Left,
    });
    adapter.message(serde_json::json!({"paste":"Mouse submit","token":token}));
    assert!(
        adapter
            .app
            .model()
            .spaces
            .iter()
            .any(|space| space.name == "Mouse submit")
    );
}

#[test]
fn enter_arriving_after_the_paste_deadline_does_not_submit_stale_text() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9856);
    adapter.app.open_create_space();
    adapter.ui_input(ui::UiInput::Text("Old".into()));
    let token = begin_paste(&mut adapter);
    adapter.pending_paste.as_mut().unwrap().deadline = Instant::now();
    adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
    assert_eq!(adapter.app.model().spaces.len(), 1);
    assert!(adapter.pending_paste.is_none());
    adapter.message(serde_json::json!({"paste":"late","token":token}));
    assert_eq!(editor_text(&mut adapter), "Old");
}

#[test]
fn paste_replay_stops_at_focus_departure_and_preserves_ime_cancellation() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9857);
    adapter.app.open_create_space();
    adapter.render(geometry(), Instant::now());
    let token = begin_paste(&mut adapter);
    adapter.ui_input(ui::UiInput::ImePreedit {
        text: "かな".into(),
        cursor: None,
    });
    adapter.ui_input(ui::UiInput::ImePreedit {
        text: String::new(),
        cursor: None,
    });
    adapter.message(serde_json::json!({"paste":"Project","token":token}));
    adapter.render(geometry(), Instant::now());
    let rendered = adapter
        .app
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!rendered.contains("かな"));
    let token = begin_paste(&mut adapter);
    adapter.ui_input(ui::UiInput::key(ui::Key::Tab));
    adapter.ui_input(ui::UiInput::Text("discarded".into()));
    adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
    adapter.message(serde_json::json!({"paste":" next","token":token}));
    assert_eq!(adapter.app.ui().focused(), Some(&ui::ElementId::Submit));
    assert_eq!(adapter.app.model().spaces.len(), 1);
    adapter.ui_input(ui::UiInput::key(ui::Key::Enter));
    assert!(
        adapter
            .app
            .model()
            .spaces
            .iter()
            .any(|space| space.name == "Project next")
    );
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
        adapter.message(serde_json::json!({"paste":"Pasted 界","token":token}));
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
fn paste_survives_hover_and_empty_composition_notifications() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9845);
    adapter.app.open_create_space();
    adapter.ui_input(ui::UiInput::Text("Before".into()));
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('a'),
        window::Modifiers::CTRL,
        true,
    )));
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('v'),
        window::Modifiers::CTRL,
        true,
    )));
    let token = paste_token(&mut adapter);
    adapter.ui_input(ui::UiInput::PointerMove {
        x: u16::MAX,
        y: u16::MAX,
    });
    adapter.ui_input(ui::UiInput::ImePreedit {
        text: String::new(),
        cursor: None,
    });
    adapter.message(serde_json::json!({"paste":"After","token":token}));
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('a'),
        window::Modifiers::CTRL,
        true,
    )));
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('c'),
        window::Modifiers::CTRL,
        true,
    )));
    assert_eq!(copied_text(&mut adapter), "After");
}

#[test]
fn paste_is_discarded_after_editing_focus_changes() {
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9846);
    adapter.app.open_create_space();
    adapter.ui_input(ui::UiInput::Text("Keep".into()));
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('v'),
        window::Modifiers::CTRL,
        true,
    )));
    let token = paste_token(&mut adapter);
    adapter.input(Input::Focus(false));
    adapter.input(Input::Focus(true));
    adapter.message(serde_json::json!({"paste":"Discard","token":token}));
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('a'),
        window::Modifiers::CTRL,
        true,
    )));
    adapter.input(Input::RawKey(&raw(
        KeyCode::Char('c'),
        window::Modifiers::CTRL,
        true,
    )));
    assert_eq!(copied_text(&mut adapter), "Keep");
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

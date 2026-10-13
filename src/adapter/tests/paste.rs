use super::super::support::{copied_text, geometry, paste_token, raw};
use super::*;
use window::KeyCode;

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
    adapter.message(Message::Paste {
        text: Some("Project".into()),
        token,
    });
    assert!(
        adapter
            .app
            .model()
            .spaces
            .iter()
            .any(|space| space.name == "Project suffix")
    );
    assert!(adapter.input.deadline().is_none());
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
    adapter.message(Message::Paste {
        text: Some("First".into()),
        token: first,
    });
    let commands = adapter.commands();
    assert!(
        matches!(commands.as_slice(), [Command::Clipboard(copy), Command::Clipboard(cut), Command::Paste(_)] if copy == "First" && cut == "First")
    );
    let second = match commands.last().unwrap() {
        Command::Paste(token) => *token,
        _ => unreachable!(),
    };
    assert_ne!(first, second);
    adapter.message(Message::Paste {
        text: Some("stale".into()),
        token: first,
    });
    adapter.message(Message::Paste {
        text: Some("Second".into()),
        token: second,
    });
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
            let deadline = adapter.input.deadline().unwrap();
            assert!(adapter.deadline().unwrap() <= deadline);
            adapter.render(geometry(), deadline);
        } else {
            adapter.message(Message::Paste { token, text: None });
            adapter.render(geometry(), Instant::now());
        }
        assert!(adapter.input.deadline().is_none());
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
        adapter.message(Message::Paste {
            text: Some("late".into()),
            token,
        });
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
        assert!(adapter.input.deadline().is_none());
        adapter.message(Message::Paste {
            text: Some("late".into()),
            token,
        });
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
            adapter.message(Message::Action(core::Action::Ui(
                core::UiAction::CreateSpace,
            )));
        }
        adapter.ui_input(ui::UiInput::Text("Keep".into()));
        adapter.message(Message::Paste {
            text: Some("late".into()),
            token,
        });
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
    adapter.message(Message::Paste {
        text: Some("Project".into()),
        token,
    });
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
    adapter.message(Message::Paste {
        text: Some("Mouse submit".into()),
        token,
    });
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
    let deadline = adapter.input.deadline().unwrap();
    adapter.ui_input_at(ui::UiInput::key(ui::Key::Enter), deadline);
    assert_eq!(adapter.app.model().spaces.len(), 1);
    assert!(adapter.input.deadline().is_none());
    adapter.message(Message::Paste {
        text: Some("late".into()),
        token,
    });
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
    adapter.message(Message::Paste {
        text: Some("Project".into()),
        token,
    });
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
    adapter.message(Message::Paste {
        text: Some(" next".into()),
        token,
    });
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
    adapter.message(Message::Paste {
        text: Some("After".into()),
        token,
    });
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
    adapter.message(Message::Paste {
        text: Some("Discard".into()),
        token,
    });
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

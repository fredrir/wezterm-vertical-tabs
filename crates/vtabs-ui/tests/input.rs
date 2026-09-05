use vtabs_ui::{EditResult, Key, Modifiers, TextEditor};

#[test]
fn grapheme_editing_never_splits_combining_emoji_or_cjk() {
    let mut edit = TextEditor::new("e\u{301}👨‍👩‍👧‍👦界");
    assert_eq!(edit.grapheme_count(), 3);
    edit.key(&Key::Backspace, Modifiers::default());
    assert_eq!(edit.text(), "e\u{301}👨‍👩‍👧‍👦");
    edit.key(&Key::Left, Modifiers::default());
    edit.key(&Key::Delete, Modifiers::default());
    assert_eq!(edit.text(), "e\u{301}");
    edit.key(&Key::Backspace, Modifiers::default());
    assert_eq!(edit.text(), "");
}

#[test]
fn selection_copy_cut_and_paste_are_explicit_native_requests() {
    let mut edit = TextEditor::new("one 界 two");
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    edit.key(&Key::Character('a'), ctrl);
    assert_eq!(
        edit.key(&Key::Character('c'), ctrl),
        EditResult::Copy("one 界 two".into())
    );
    assert_eq!(
        edit.key(&Key::Character('x'), ctrl),
        EditResult::Copy("one 界 two".into())
    );
    assert_eq!(edit.text(), "");
    assert_eq!(edit.key(&Key::Character('v'), ctrl), EditResult::Paste);
    edit.insert("paste\n\u{1b}[2J");
    assert_eq!(edit.text(), "paste[2J");
}

#[test]
fn word_selection_and_combining_insertion_keep_valid_cursor() {
    let mut edit = TextEditor::new("one two");
    edit.key(
        &Key::Left,
        Modifiers {
            control: true,
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(edit.selected_text(), "two");
    edit.insert("é");
    assert_eq!(edit.text(), "one é");
    let mut edit = TextEditor::new("e");
    edit.insert("\u{301}");
    assert_eq!(edit.cursor(), 1);
    assert_eq!(edit.grapheme_count(), 1);
    edit.key(&Key::Backspace, Modifiers::default());
    assert_eq!(edit.text(), "");
}

#[test]
fn command_arrows_move_and_select_to_line_boundaries() {
    let mut edit = TextEditor::new("one e\u{301} 界 👨‍👩‍👧‍👦");
    let command = Modifiers {
        super_key: true,
        ..Modifiers::default()
    };
    edit.key(&Key::Left, command);
    assert_eq!(edit.cursor(), 0);
    assert!(edit.selection().is_none());
    edit.key(&Key::Right, Modifiers::default());
    edit.key(
        &Key::Right,
        Modifiers {
            shift: true,
            ..command
        },
    );
    assert_eq!(edit.selected_text(), "ne e\u{301} 界 👨‍👩‍👧‍👦");
    edit.key(&Key::Right, command);
    assert_eq!(edit.cursor(), edit.grapheme_count());
    assert!(edit.selection().is_none());
    edit.key(
        &Key::Left,
        Modifiers {
            shift: true,
            ..command
        },
    );
    assert_eq!(edit.selected_text(), edit.text());
}

#[test]
fn command_backspace_deletes_prefix_or_selection_and_preserves_suffix() {
    let command = Modifiers {
        super_key: true,
        ..Modifiers::default()
    };
    let mut edit = TextEditor::new("one 界👨‍👩‍👧‍👦");
    edit.key(&Key::Left, Modifiers::default());
    edit.key(&Key::Backspace, command);
    assert_eq!(edit.text(), "👨‍👩‍👧‍👦");
    assert_eq!(edit.cursor(), 0);
    edit.key(&Key::Backspace, command);
    assert_eq!(edit.text(), "👨‍👩‍👧‍👦");

    let mut edit = TextEditor::new("one two");
    edit.key(
        &Key::Left,
        Modifiers {
            shift: true,
            alt: true,
            ..Modifiers::default()
        },
    );
    edit.key(&Key::Backspace, command);
    assert_eq!(edit.text(), "one ");
    assert_eq!(edit.cursor(), 4);
}

#[test]
fn control_and_option_keep_word_navigation_and_deletion() {
    for modifier in [
        Modifiers {
            control: true,
            ..Modifiers::default()
        },
        Modifiers {
            alt: true,
            ..Modifiers::default()
        },
    ] {
        let mut edit = TextEditor::new("one two three");
        edit.key(&Key::Left, modifier);
        assert_eq!(edit.cursor(), 8);
        edit.key(&Key::Left, modifier);
        assert_eq!(edit.cursor(), 4);
        edit.key(&Key::Right, modifier);
        assert_eq!(edit.cursor(), 8);
        edit.key(&Key::Backspace, modifier);
        assert_eq!(edit.text(), "one three");
        assert_eq!(edit.cursor(), 4);
    }
}

#[test]
fn ime_preedit_is_not_committed_until_native_commit_and_escape_cancels_it() {
    let mut edit = TextEditor::new("A");
    edit.set_preedit("かな", Some(3));
    assert_eq!(edit.text(), "A");
    assert_eq!(edit.display_text(), "Aかな");
    assert_eq!(edit.cursor_columns(), 3);
    assert_eq!(
        edit.key(&Key::Escape, Modifiers::default()),
        EditResult::Changed
    );
    assert_eq!(edit.display_text(), "A");
    edit.set_preedit("漢", Some(1));
    assert_eq!(edit.cursor_columns(), 1);
    edit.insert("漢");
    assert_eq!(edit.text(), "A漢");
    assert!(edit.preedit().is_empty());
}

#[test]
fn input_is_bounded_and_scroll_is_aligned_to_wide_graphemes() {
    let mut edit = TextEditor::new("界界界");
    edit.keep_cursor_visible(4);
    assert_eq!(edit.scroll_columns, 4);
    edit.click_column(0, false);
    assert_eq!(edit.cursor(), 2);
    edit.select_all();
    edit.insert(&"界".repeat(5000));
    assert!(edit.text().len() <= TextEditor::MAX_BYTES);
    assert!(edit.text().is_char_boundary(edit.text().len()));
}

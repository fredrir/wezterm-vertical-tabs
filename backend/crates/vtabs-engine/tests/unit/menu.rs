use super::*;

fn item(id: &str, label: &str) -> MenuItem {
    MenuItem {
        id: id.into(),
        label: label.into(),
        hint: None,
        mode: None,
        value: None,
        disabled: false,
        danger: false,
        confirm: None,
    }
}

#[test]
fn wrap_breaks_on_a_space_then_a_slash_then_hard() {
    assert_eq!(wrap("hello world", 7, 3, "…"), vec!["hello", "world"]);
    assert_eq!(wrap("a/bb/ccc", 4, 3, "…"), vec!["a/", "bb/", "ccc"]);
    assert_eq!(wrap("abcdefgh", 4, 3, "…"), vec!["abcd", "efgh"]);
}

#[test]
fn wrap_folds_the_overflow_into_the_last_line_it_had_room_for() {
    // one row: the rest is glued back on and truncated, so nothing is silently lost
    assert_eq!(wrap("hello world", 7, 1, "…"), vec!["hello …"]);
    assert!(wrap("", 7, 3, "…").is_empty());
}

#[test]
fn the_header_drops_its_lowest_priority_line_first() {
    let lines = vec![
        HeadLine {
            text: "title".into(),
            meta_tone: false,
            drop: drop::TITLE,
        },
        HeadLine {
            text: "meta".into(),
            meta_tone: true,
            drop: drop::META,
        },
        HeadLine {
            text: "".into(),
            meta_tone: true,
            drop: drop::SEPARATOR,
        },
    ];
    let kept = drop_to(&lines, 2);
    assert_eq!(
        kept.iter().map(|l| l.drop).collect::<Vec<_>>(),
        vec![drop::TITLE, drop::SEPARATOR]
    );
    assert_eq!(
        drop_to(&lines, 1)[0].drop,
        drop::TITLE,
        "the title survives"
    );
}

#[test]
fn move_skips_disabled_items_and_stops_at_the_ends() {
    let mut items = vec![item("a", "A"), item("b", "B"), item("c", "C")];
    items[1].disabled = true;
    assert_eq!(move_by(&items, 1, 1), 3, "over the disabled one");
    assert_eq!(move_by(&items, 3, 1), 3, "stops at the end");
    assert_eq!(move_by(&items, 1, -1), 1, "stops at the start");
}

#[test]
fn jump_finds_the_next_match_and_wraps() {
    let items = vec![item("a", "Close tab"), item("b", "Pin"), item("c", "Copy")];
    assert_eq!(jump(&items, 1, 'c'), Some(3), "the next C, not the current");
    assert_eq!(jump(&items, 3, 'c'), Some(1), "wraps");
    assert_eq!(jump(&items, 1, 'z'), None);
}

#[test]
fn the_edit_buffer_moves_and_deletes_by_character() {
    let mut s = MenuState {
        buffer: "abc".into(),
        cursor: 4,
        ..Default::default()
    };
    assert_eq!(edit(&mut s, "backspace", false), Edit::Consumed);
    assert_eq!((s.buffer.as_str(), s.cursor), ("ab", 3));
    edit(&mut s, "x", false);
    assert_eq!((s.buffer.as_str(), s.cursor), ("abx", 4));
    edit(&mut s, "home", false);
    edit(&mut s, "delete", false);
    assert_eq!((s.buffer.as_str(), s.cursor), ("bx", 1));
    edit(&mut s, "u", true);
    assert_eq!((s.buffer.as_str(), s.cursor), ("", 1));
    assert_eq!(edit(&mut s, "enter", false), Edit::Commit);
    assert_eq!(edit(&mut s, "escape", false), Edit::Cancel);
    assert_eq!(edit(&mut s, "c", true), Edit::Cancel);
}

#[test]
fn the_edit_buffer_moves_and_deletes_by_grapheme() {
    let mut s = MenuState {
        buffer: "e\u{301}👩‍💻".into(),
        cursor: 3,
        ..Default::default()
    };
    edit(&mut s, "backspace", false);
    assert_eq!((s.buffer.as_str(), s.cursor), ("e\u{301}", 2));
    edit(&mut s, "backspace", false);
    assert_eq!((s.buffer.as_str(), s.cursor), ("", 1));
    edit(&mut s, "👨‍👩‍👧‍👦", false);
    assert_eq!((s.buffer.as_str(), s.cursor), ("👨‍👩‍👧‍👦", 2));
}

#[test]
fn the_rename_cursor_uses_display_columns_not_grapheme_indices() {
    let state = MenuState {
        buffer: "a👩‍💻b".into(),
        cursor: 3,
        ..Default::default()
    };
    let theme = crate::theme::resolve(
        &crate::theme::UserTheme::default(),
        &crate::theme::Palette::default(),
        false,
    );
    let rows = rename_rows(&state, 16, &theme, "…");
    let field = &rows[2].spans;
    assert_eq!(field[2].text, "a👩‍💻b");
    assert_eq!(field[3].text, "b");
    assert_eq!(field[3].x, 6, "one narrow plus one two-column grapheme");
}

#[test]
fn ctrl_w_eats_the_word_before_the_cursor() {
    let mut s = MenuState {
        buffer: "one two  ".into(),
        cursor: 10,
        ..Default::default()
    };
    edit(&mut s, "w", true);
    assert_eq!((s.buffer.as_str(), s.cursor), ("one ", 5));
}

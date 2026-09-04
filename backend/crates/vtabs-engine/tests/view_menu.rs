use vtabs_engine::enrich::MenuHeader;
use vtabs_engine::menu::{self, Level, MenuCfg, MenuState, Outcome};
use vtabs_engine::theme::{Palette, Theme, UserTheme};
use vtabs_engine::{color::Color, scene::PopRow};
use vtabs_protocol::payload::MenuMsg;

fn theme() -> Theme {
    vtabs_engine::theme::resolve(&UserTheme::default(), &Palette::default(), false)
}

fn cfg() -> MenuCfg {
    MenuCfg {
        padding_left: 1,
        padding_right: 1,
        ..Default::default()
    }
}

fn msg(json: &str) -> MenuMsg {
    serde_json::from_str(json).expect("a menu message")
}

/// The root level as popover.lua's `wire_body` composes it.
fn root() -> MenuMsg {
    msg(
        r#"{"open":true,"level":"root","anchor":{"row":4,"col":2},"target":7,"selected":1,
        "items":[
          {"id":"activate","label":"Switch to tab"},
          {"id":"pin","label":"Pin tab","hint":"^P"},
          {"id":"rename","label":"Rename…"},
          {"id":"space","label":"Move to space","hint":"▸","disabled":true},
          {"id":"close","label":"Close tab","hint":"^W","danger":true}]}"#,
    )
}

fn confirm() -> MenuMsg {
    msg(
        r#"{"open":true,"level":"confirm","anchor":{"row":4,"col":2},"target":7,"selected":2,
        "items":[{"id":"confirm_close","label":"Close","danger":true},
                 {"id":"confirm_cancel","label":"Cancel"}]}"#,
    )
}

fn open(m: &MenuMsg, state: &MenuState, dims: (i64, i64)) -> Box<menu::Placed> {
    let header = test_header(m);
    open_with_header(m, &header, state, dims)
}

fn open_with_header(
    m: &MenuMsg,
    header: &MenuHeader,
    state: &MenuState,
    dims: (i64, i64),
) -> Box<menu::Placed> {
    match menu::plan(m, Some(header), state, &cfg(), &theme(), dims) {
        Outcome::Open(placed) => placed,
        other => panic!("expected an open menu, got {other:?}"),
    }
}

fn test_header(m: &MenuMsg) -> MenuHeader {
    match Level::of(m) {
        Level::Confirm => MenuHeader {
            title: "Close nvim?".into(),
            meta: Some("and 2 others".into()),
        },
        Level::Rename => MenuHeader {
            title: "Rename tab".into(),
            meta: None,
        },
        Level::Spaces => MenuHeader {
            title: "Move to space".into(),
            meta: Some("nvim".into()),
        },
        Level::Root => MenuHeader {
            title: "nvim".into(),
            meta: Some("~/src/vtabs".into()),
        },
    }
}

fn adopted(m: &MenuMsg) -> MenuState {
    let mut state = MenuState::default();
    state.adopt(m);
    state
}

/// The row's text at its own columns, the way `composite` would lay the spans down.
fn text_of(row: &PopRow, w: i64) -> String {
    let mut cells = vec![' '; w.max(0) as usize];
    for span in &row.spans {
        for (i, ch) in span.text.chars().enumerate() {
            let at = span.x - 1 + i as i64;
            if at >= 0 && (at as usize) < cells.len() {
                cells[at as usize] = ch;
            }
        }
    }
    cells.into_iter().collect()
}

fn row_with(placed: &menu::Placed, needle: &str) -> usize {
    let w = placed.rect.w.unwrap();
    placed
        .rect
        .rows
        .iter()
        .position(|r| text_of(r, w).contains(needle))
        .unwrap_or_else(|| panic!("no row carrying {needle:?}"))
}

fn fg_at(row: &PopRow, needle: &str) -> Color {
    row.spans
        .iter()
        .find(|s| s.text.contains(needle))
        .and_then(|s| s.fg)
        .expect("a coloured span")
}

// ── invariant 4 ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cancel_is_the_armed_default_at_the_confirm_level() {
    let state = adopted(&confirm());
    assert_eq!(state.selected, 2, "the message arms Cancel, not Close");
    let placed = open(&confirm(), &state, (28, 24));
    let w = placed.rect.w.unwrap();
    let cancel = &placed.rect.rows[row_with(&placed, "Cancel")];
    let close = &placed.rect.rows[row_with(&placed, "Close")];
    assert_eq!(
        cancel.bg,
        Some(theme().popover_sel_bg),
        "Cancel is selected"
    );
    assert_eq!(close.bg, None, "the destructive answer is not");
    assert!(
        text_of(cancel, w).contains('▎') && !text_of(close, w).contains('▎'),
        "and the marker follows the selection"
    );
}

// ── invariant 5 ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_selected_row_wears_the_accent_fill() {
    let t = theme();
    let placed = open(&root(), &adopted(&root()), (28, 24));
    let row = &placed.rect.rows[row_with(&placed, "Switch to tab")];
    assert_eq!(row.bg, Some(t.popover_sel_bg), "popover_sel_bg fills it");
    assert_eq!(fg_at(row, "Switch"), t.popover_sel_fg, "and sel_fg inks it");

    let hinted = &placed.rect.rows[row_with(&placed, "Pin tab")];
    assert_eq!(hinted.bg, None);
    assert_eq!(fg_at(hinted, "Pin tab"), t.fg);
    assert_eq!(fg_at(hinted, "^P"), t.disabled_fg, "an idle hint stays dim");

    // the selected row's hint takes sel_hint, not the dim it would wear anywhere else
    let mut on_pin = adopted(&root());
    on_pin.selected = 2;
    let moved = open(&root(), &on_pin, (28, 24));
    let row = &moved.rect.rows[row_with(&moved, "Pin tab")];
    assert_eq!(fg_at(row, "^P"), t.popover_sel_hint);
}

#[test]
fn a_disabled_row_is_dim_and_still_carries_its_id() {
    let placed = open(&root(), &adopted(&root()), (28, 24));
    let at = row_with(&placed, "Move to space");
    assert_eq!(fg_at(&placed.rect.rows[at], "Move to"), theme().disabled_fg);
    assert_eq!(
        placed.hits.rows[at],
        (Some("space".into()), true),
        "a click can name it, and learns it refuses"
    );
}

// ── invariant 3 ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_confirm_that_cannot_be_drawn_is_refused_and_draws_nothing() {
    // MIN_RENDER_W is 4: three columns minus the padding leaves the rect nothing to hold.
    let narrow = menu::plan(
        &confirm(),
        None,
        &MenuState::default(),
        &cfg(),
        &theme(),
        (4, 24),
    );
    assert!(
        matches!(
            narrow,
            Outcome::Refused {
                why: "width",
                level: Level::Confirm
            }
        ),
        "too narrow: {narrow:?}"
    );
    let flat = menu::plan(
        &confirm(),
        None,
        &MenuState::default(),
        &cfg(),
        &theme(),
        (28, 2),
    );
    assert!(
        matches!(
            flat,
            Outcome::Refused {
                why: "rows",
                level: Level::Confirm
            }
        ),
        "too short: {flat:?}"
    );
    // and a pane that can hold two borders and a cell draws, however cramped
    assert!(matches!(
        menu::plan(
            &confirm(),
            None,
            &MenuState::default(),
            &cfg(),
            &theme(),
            (28, 3)
        ),
        Outcome::Open(_)
    ));
}

#[test]
fn a_closed_message_draws_nothing() {
    let closed = msg(r#"{"open":false}"#);
    assert!(matches!(
        menu::plan(
            &closed,
            None,
            &MenuState::default(),
            &cfg(),
            &theme(),
            (28, 24)
        ),
        Outcome::Closed
    ));
    // but one this backend already told Lua to close draws nothing at all
    let mut state = adopted(&root());
    state.dismiss();
    assert!(matches!(
        menu::plan(&root(), None, &state, &cfg(), &theme(), (28, 24)),
        Outcome::Closed
    ));
}

// ── placement ───────────────────────────────────────────────────────────────────────────────

#[test]
fn every_row_answers_for_the_whole_rect_so_a_click_beside_the_menu_is_click_away() {
    let placed = open(&root(), &adopted(&root()), (28, 24));
    let w = placed.rect.w.unwrap();
    assert_eq!(placed.hits.w, w);
    assert!(placed.hits.inside(placed.hits.x));
    assert!(placed.hits.inside(placed.hits.x + w - 1));
    assert!(!placed.hits.inside(placed.hits.x + w), "one past the edge");
    assert!(!placed.hits.covers(placed.hits.y - 1), "a scrim row");
    assert_eq!(
        placed.hits.rows[0],
        (None, false),
        "the frame names nothing"
    );
}

// ── the levels ──────────────────────────────────────────────────────────────────────────────

#[test]
fn the_header_wraps_the_title_and_keeps_the_confirm_question_whole() {
    let long = root();
    let header = MenuHeader {
        title: "a very long tab title that will not fit".into(),
        meta: Some("~/src/vtabs".into()),
    };
    let placed = open_with_header(&long, &header, &adopted(&long), (28, 24));
    let w = placed.rect.w.unwrap();
    let head: Vec<String> = placed.rect.rows[1..4]
        .iter()
        .map(|r| text_of(r, w))
        .collect();
    assert!(
        head[0].contains("a very"),
        "wrapped, not truncated: {head:?}"
    );
    assert!(head[1].contains("that"), "onto a second line: {head:?}");

    let placed = open(&confirm(), &adopted(&confirm()), (28, 24));
    let w = placed.rect.w.unwrap();
    assert!(text_of(&placed.rect.rows[1], w).contains("Close nvim?"));
    assert!(
        text_of(&placed.rect.rows[2], w).contains("and 2 others"),
        "the count comes after the question"
    );
}

#[test]
fn the_rename_box_keeps_the_width_the_menu_it_came_from_asked_for() {
    let mut state = adopted(&root());
    let root_w = open(&root(), &state, (28, 24)).rect.w.unwrap();

    let rename = msg(
        r#"{"open":true,"level":"rename","anchor":{"row":4,"col":2},"target":7,"selected":1,
            "items":[{"id":"rename_field","mode":"edit","label":"","value":"nvim"}]}"#,
    );
    state.adopt(&rename);
    assert_eq!(state.buffer, "nvim", "the field seeds the buffer");
    assert_eq!(state.cursor, 5, "with the cursor past the last character");

    let placed = open(&rename, &state, (28, 24));
    assert_eq!(placed.rect.w, Some(root_w), "the box does not jump");
    let w = placed.rect.w.unwrap();
    assert!(text_of(&placed.rect.rows[1], w).contains("Rename tab"));
    assert!(text_of(&placed.rect.rows[3], w).contains("nvim"));
}

#[test]
fn the_selection_is_local_until_the_level_or_its_items_change() {
    let mut state = adopted(&root());
    state.selected = 4;
    // the same message again: a stale round-trip must not drag the selection back to 1
    state.adopt(&root());
    assert_eq!(state.selected, 4);

    let mut renamed = root();
    renamed.items[1].label = "Unpin tab".into();
    state.adopt(&renamed);
    assert_eq!(state.selected, 1, "new items, so the message decides again");

    state.selected = 3;
    state.adopt(&confirm());
    assert_eq!(state.selected, 2, "and so does a new level");
}

fn spaces() -> MenuMsg {
    msg(
        r#"{"open":true,"level":"spaces","anchor":{"row":4,"col":2},"target":7,"selected":1,
        "items":[{"id":"space:home","label":"Home","hint":"3","disabled":true},
                 {"id":"space:claude","label":"Claude","hint":"1"},
                 {"id":"space:pi","label":"pi"},
                 {"id":"space_auto","label":"Auto (follow rules)","disabled":true}]}"#,
    )
}

#[test]
fn the_spaces_level_keeps_the_root_width_and_starts_past_the_current_space() {
    let mut state = adopted(&root());
    let root_w = open(&root(), &state, (28, 24)).rect.w.unwrap();
    state.adopt(&spaces());
    assert_eq!(
        state.selected, 2,
        "selected:1 names the disabled current space; the first live row takes it"
    );
    let placed = open(&spaces(), &state, (28, 24));
    assert_eq!(placed.level, Level::Spaces);
    let w = placed.rect.w.unwrap();
    assert!(
        w >= root_w,
        "the sub-level never draws narrower than the root it came from"
    );
    assert!(text_of(&placed.rect.rows[1], w).contains("Move to space"));
}

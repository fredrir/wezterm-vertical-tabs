//! The rendering half of 06-p5-spec.md's invariants: what the menu draws, where it draws it, and
//! what it does when it cannot.

use vtabs_protocol::v2::MenuMsg;
use vtabs_theme::{Palette, Theme, UserTheme};
use vtabs_view::menu::{self, Level, MenuCfg, MenuState, Outcome};
use vtabs_view::scene::{PopRow, Rgb};

fn theme() -> Theme {
    vtabs_theme::resolve(&UserTheme::default(), &Palette::default(), false)
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
        r#"{"rev":1,"open":true,"level":"root","anchor":{"row":4,"col":2},"target":7,"selected":1,
        "header":{"title":"nvim","meta":"~/src/vtabs"},
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
        r#"{"rev":2,"open":true,"level":"confirm","anchor":{"row":4,"col":2},"target":7,"selected":2,
        "header":{"title":"Close nvim?","meta":"and 2 others"},
        "items":[{"id":"confirm_close","label":"Close","danger":true},
                 {"id":"confirm_cancel","label":"Cancel"}]}"#,
    )
}

fn open(m: &MenuMsg, state: &MenuState, dims: (i64, i64)) -> Box<menu::Placed> {
    match menu::plan(m, state, &cfg(), &theme(), dims) {
        Outcome::Open(placed) => placed,
        other => panic!("expected an open menu, got {other:?}"),
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

fn fg_at(row: &PopRow, needle: &str) -> Rgb {
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
    let narrow = menu::plan(&confirm(), &MenuState::default(), &cfg(), &theme(), (4, 24));
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
    let flat = menu::plan(&confirm(), &MenuState::default(), &cfg(), &theme(), (28, 2));
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
        menu::plan(&confirm(), &MenuState::default(), &cfg(), &theme(), (28, 3)),
        Outcome::Open(_)
    ));
}

#[test]
fn a_closed_message_draws_nothing() {
    let closed = msg(r#"{"rev":3,"open":false}"#);
    assert!(matches!(
        menu::plan(&closed, &MenuState::default(), &cfg(), &theme(), (28, 24)),
        Outcome::Closed
    ));
    // but one this backend already told Lua to close draws nothing at all
    let mut state = adopted(&root());
    state.dismiss();
    assert!(matches!(
        menu::plan(&root(), &state, &cfg(), &theme(), (28, 24)),
        Outcome::Closed
    ));
}

// ── placement ───────────────────────────────────────────────────────────────────────────────

#[test]
fn the_menu_opens_below_its_anchor_and_slides_back_inside_the_sidebar() {
    let placed = open(&root(), &adopted(&root()), (28, 24));
    assert_eq!(placed.rect.y, 5, "the row after the anchor");
    assert_eq!(placed.rect.x, 2, "the column that asked for it");

    // §6.4: an anchor near the right edge slides back so the whole rect stays in the pane
    let mut far = root();
    far.anchor.as_mut().unwrap().col = Some(26);
    let placed = open(&far, &adopted(&far), (28, 24));
    let w = placed.rect.w.unwrap();
    assert_eq!(placed.rect.x + w - 1, 27, "flush against the right padding");
}

#[test]
fn a_menu_with_no_room_below_opens_above_its_anchor() {
    let mut low = root();
    low.anchor.as_mut().unwrap().row = 22;
    let placed = open(&low, &adopted(&low), (28, 24));
    assert_eq!(
        placed.rect.y + placed.rect.h - 1,
        21,
        "it ends at the anchor"
    );
}

#[test]
fn a_pane_too_short_for_the_header_drops_it_and_then_scrolls_the_list() {
    // 5 items plus 2 frame rows is 7; a 7-row pane can hold the list but not one header line
    let placed = open(&root(), &adopted(&root()), (28, 7));
    let w = placed.rect.w.unwrap();
    assert_eq!(placed.rect.h, 7);
    assert!(
        !placed
            .rect
            .rows
            .iter()
            .any(|r| text_of(r, w).contains("nvim")),
        "the header went first"
    );

    // 5 rows holds 3 items, and the selection at the end scrolls them into view
    let mut deep = adopted(&root());
    deep.selected = 5;
    let placed = open(&root(), &deep, (28, 5));
    assert_eq!(placed.rect.h, 5);
    assert_eq!(
        placed.hits.rows[1..4]
            .iter()
            .map(|(id, _)| id.clone().unwrap())
            .collect::<Vec<_>>(),
        vec!["rename", "space", "close"],
        "the last page"
    );
}

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
    let mut long = root();
    long.header.as_mut().unwrap().title = "a very long tab title that will not fit".into();
    let placed = open(&long, &adopted(&long), (28, 24));
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
        r#"{"rev":4,"open":true,"level":"rename","anchor":{"row":4,"col":2},"target":7,"selected":1,
            "header":{"title":"Rename tab"},
            "items":[{"id":"rename_field","mode":"edit","label":"","value":"nvim"}]}"#,
    );
    state.adopt(&rename);
    assert_eq!(state.buffer, "nvim", "the field seeds the buffer");
    assert_eq!(state.cursor, 5, "with the cursor past the last character");

    let placed = open(&rename, &state, (28, 24));
    assert_eq!(placed.rect.w, Some(root_w), "the box does not jump");
    assert_eq!(placed.rect.h, 7, "five rows plus its frame");
    let w = placed.rect.w.unwrap();
    assert!(text_of(&placed.rect.rows[1], w).contains("Rename tab"));
    assert!(text_of(&placed.rect.rows[3], w).contains("nvim"));
    assert!(text_of(&placed.rect.rows[5], w).contains("⏎ save"));
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

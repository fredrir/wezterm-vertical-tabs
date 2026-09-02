use vtabs_engine::interaction::{MenuView, menu_key, menu_mouse};
use vtabs_engine::menu::{self, MenuCfg, MenuState, Outcome, Placed};
use vtabs_engine::theme::{Palette, Theme, UserTheme};
use vtabs_engine::ui::UiState;
use vtabs_protocol::types::{Button, Mods, Mouse, MouseKind};
use vtabs_protocol::v2::MenuMsg;
use vtabs_protocol::{Event, Intent};

const DIMS: (i64, i64) = (28, 24);

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

fn root() -> MenuMsg {
    msg(
        r#"{"rev":1,"open":true,"level":"root","anchor":{"row":4,"col":2},"target":7,"selected":1,
        "items":[
          {"id":"activate","label":"Switch to tab"},
          {"id":"pin","label":"Pin tab"},
          {"id":"rename","label":"Rename…"},
          {"id":"space","label":"Move to space","disabled":true},
          {"id":"close","label":"Close tab","danger":true}]}"#,
    )
}

fn confirm() -> MenuMsg {
    msg(
        r#"{"rev":2,"open":true,"level":"confirm","anchor":{"row":4,"col":2},"target":7,"selected":2,
        "header":{"title":"Close nvim?"},
        "items":[{"id":"confirm_close","label":"Close","danger":true},
                 {"id":"confirm_cancel","label":"Cancel"}]}"#,
    )
}

fn rename() -> MenuMsg {
    msg(
        r#"{"rev":3,"open":true,"level":"rename","anchor":{"row":4,"col":2},"target":7,"selected":1,
        "items":[{"id":"rename_field","mode":"edit","label":"","value":"nvim"}]}"#,
    )
}

/// The menu as the runtime holds it: the message, the state it adopted, and where it landed.
struct Open {
    msg: MenuMsg,
    state: MenuState,
    placed: Box<Placed>,
}

fn opened(m: MenuMsg) -> Open {
    let mut state = MenuState::default();
    state.adopt(&m);
    let placed = match menu::plan(&m, &state, &cfg(), &theme(), DIMS) {
        Outcome::Open(placed) => placed,
        other => panic!("expected an open menu, got {other:?}"),
    };
    state.selected = placed.selected;
    Open {
        msg: m,
        state,
        placed,
    }
}

impl Open {
    fn view(&self) -> MenuView<'_> {
        MenuView {
            level: self.placed.level,
            items: &self.msg.items,
            hits: &self.placed.hits,
            follow_pointer: true,
        }
    }

    /// The pane row the item with this id was drawn on.
    fn row_of(&self, id: &str) -> u16 {
        let at = self
            .placed
            .hits
            .rows
            .iter()
            .position(|(row, _)| row.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("{id} was not drawn"));
        (self.placed.hits.y + at as i64) as u16
    }

    fn col(&self) -> u16 {
        (self.placed.hits.x + 1) as u16
    }
}

fn ev(kind: MouseKind, button: Button, x: u16, y: u16) -> Mouse {
    Mouse {
        kind,
        button,
        x,
        y,
        dy: 0,
        mods: Mods::default(),
    }
}

fn verbs(events: &[Event]) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Intent { intent } => Some(intent.name()),
            _ => None,
        })
        .collect()
}

fn intent(events: &[Event]) -> &Intent {
    events
        .iter()
        .find_map(|e| match e {
            Event::Intent { intent } => Some(intent),
            _ => None,
        })
        .expect("an intent event")
}

// ── invariant 1 ─────────────────────────────────────────────────────────────────────────────

#[test]
fn destructive_acts_only_on_release_same_target() {
    let m = opened(root());
    let (v, ui) = (m.view(), UiState::default());
    let (x, close) = (m.col(), m.row_of("close"));

    let press = menu_mouse(
        &v,
        &m.state,
        &ui,
        &ev(MouseKind::Press, Button::Left, x, close),
        0,
    );
    assert!(verbs(&press.events).is_empty(), "the press never destroys");
    let armed = press.menu.expect("state");
    assert_eq!(armed.armed.as_deref(), Some("close"), "it arms instead");

    // released somewhere else: the arm is spent and nothing runs
    let elsewhere = m.row_of("pin");
    let away = menu_mouse(
        &v,
        &armed,
        &ui,
        &ev(MouseKind::Release, Button::Left, x, elsewhere),
        0,
    );
    assert!(verbs(&away.events).is_empty(), "another row is not it");
    assert!(away.menu.unwrap().armed.is_none(), "and the arm is gone");

    // released beside the menu, on the same row: the columns are the rect's, not the item's
    let beside = (m.placed.hits.x + m.placed.hits.w) as u16;
    let outside = menu_mouse(
        &v,
        &armed,
        &ui,
        &ev(MouseKind::Release, Button::Left, beside, close),
        0,
    );
    assert!(verbs(&outside.events).is_empty(), "off the rect is not it");

    let same = menu_mouse(
        &v,
        &armed,
        &ui,
        &ev(MouseKind::Release, Button::Left, x, close),
        0,
    );
    assert_eq!(verbs(&same.events), vec!["menu_pick"]);
    assert_eq!(
        intent(&same.events),
        &Intent::MenuPick {
            item_id: "close".into()
        }
    );
}

#[test]
fn a_disabled_item_answers_neither_press_nor_release() {
    let m = opened(root());
    let (v, ui) = (m.view(), UiState::default());
    let y = m.row_of("space");
    for kind in [MouseKind::Press, MouseKind::Release] {
        let r = menu_mouse(&v, &m.state, &ui, &ev(kind, Button::Left, m.col(), y), 0);
        assert!(verbs(&r.events).is_empty(), "{kind:?} on a disabled item");
    }
}

// ── invariant 2 ─────────────────────────────────────────────────────────────────────────────

#[test]
fn pointer_drift_never_changes_the_confirm_selection() {
    let m = opened(confirm());
    let (v, ui) = (m.view(), UiState::default());
    // Cancel is armed; the pointer resting over Close must not arm the destructive answer
    let over_close = ev(
        MouseKind::Move,
        Button::None,
        m.col(),
        m.row_of("confirm_close"),
    );
    let r = menu_mouse(&v, &m.state, &ui, &over_close, 0);
    assert_eq!(r.menu.unwrap().selected, 2, "Cancel keeps the selection");
    assert!(!r.repaint, "and nothing needs repainting");
}

#[test]
fn the_pointer_selects_the_row_it_is_over_but_only_inside_the_menu() {
    let m = opened(root());
    let (v, ui) = (m.view(), UiState::default());
    let over_pin = ev(MouseKind::Move, Button::None, m.col(), m.row_of("pin"));
    let r = menu_mouse(&v, &m.state, &ui, &over_pin, 0);
    assert_eq!(r.menu.unwrap().selected, 2);
    assert!(r.repaint);

    // a pointer that wandered onto the scrim must not erase a keyboard selection
    let scrim = ev(MouseKind::Move, Button::None, m.col(), 1);
    assert_eq!(
        menu_mouse(&v, &m.state, &ui, &scrim, 0)
            .menu
            .unwrap()
            .selected,
        1
    );
    // nor may one beside the rect on a row the menu owns
    let beside = (m.placed.hits.x + m.placed.hits.w) as u16;
    let off = ev(MouseKind::Move, Button::None, beside, m.row_of("pin"));
    assert_eq!(
        menu_mouse(&v, &m.state, &ui, &off, 0)
            .menu
            .unwrap()
            .selected,
        1
    );
    // and never onto a disabled row
    let dead = ev(MouseKind::Move, Button::None, m.col(), m.row_of("space"));
    assert_eq!(
        menu_mouse(&v, &m.state, &ui, &dead, 0)
            .menu
            .unwrap()
            .selected,
        1
    );
}

#[test]
fn follow_pointer_off_leaves_the_selection_to_the_keyboard() {
    let m = opened(root());
    let mut v = m.view();
    v.follow_pointer = false;
    let over_pin = ev(MouseKind::Move, Button::None, m.col(), m.row_of("pin"));
    let r = menu_mouse(&v, &m.state, &UiState::default(), &over_pin, 0);
    assert_eq!(r.menu.unwrap().selected, 1);
}

// ── click-away and the wheel ────────────────────────────────────────────────────────────────

#[test]
fn a_click_away_closes_the_menu_and_a_right_click_hands_the_pane_back() {
    let m = opened(root());
    let (v, ui) = (m.view(), UiState::default());
    let scrim = ev(MouseKind::Press, Button::Left, m.col(), 1);
    let away = menu_mouse(&v, &m.state, &ui, &scrim, 0);
    assert_eq!(verbs(&away.events), vec!["menu_closed"]);
    assert!(
        away.menu.unwrap().is_dismissed(),
        "it stops drawing at once"
    );
    assert!(!away.fall_through, "a left click away is spent on the menu");

    // a right click on the scrim closes and then lets the list open one for the row beneath
    let right = ev(MouseKind::Press, Button::Right, m.col(), 1);
    let r = menu_mouse(&v, &m.state, &ui, &right, 0);
    assert_eq!(verbs(&r.events), vec!["menu_closed"]);
    assert!(r.fall_through);

    // a left click on a row the menu owns but beside its columns is click-away too
    let beside = (m.placed.hits.x + m.placed.hits.w) as u16;
    let edge = ev(MouseKind::Press, Button::Left, beside, m.row_of("pin"));
    assert_eq!(
        verbs(&menu_mouse(&v, &m.state, &ui, &edge, 0).events),
        vec!["menu_closed"]
    );
}

#[test]
fn the_wheel_moves_the_selection_and_tells_lua_nothing() {
    let m = opened(root());
    let (v, ui) = (m.view(), UiState::default());
    let down = Mouse {
        kind: MouseKind::Wheel,
        button: Button::None,
        x: m.col(),
        y: m.row_of("pin"),
        dy: 1,
        mods: Mods::default(),
    };
    let r = menu_mouse(&v, &m.state, &ui, &down, 0);
    assert!(r.events.is_empty(), "the selection is Rust's alone");
    assert_eq!(r.menu.unwrap().selected, 2);
    assert!(r.repaint);

    let up = Mouse { dy: -1, ..down };
    let r = menu_mouse(&v, &m.state, &ui, &up, 0);
    assert_eq!(r.menu.unwrap().selected, 1, "already at the top");
    assert!(
        !r.repaint,
        "and a selection that did not move draws nothing"
    );
}

// ── keys ────────────────────────────────────────────────────────────────────────────────────

fn key(m: &Open, state: &MenuState, name: &str, mods: Mods) -> vtabs_engine::Resolution {
    menu_key(&m.view(), state, &UiState::default(), name, mods)
}

#[test]
fn esc_closes_the_root_and_steps_back_out_of_a_sub_level() {
    let root = opened(root());
    assert_eq!(
        verbs(&key(&root, &root.state, "escape", Mods::default()).events),
        vec!["menu_closed"]
    );
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert_eq!(
        verbs(&key(&root, &root.state, "c", ctrl).events),
        vec!["menu_closed"]
    );

    let sub = opened(confirm());
    assert_eq!(
        verbs(&key(&sub, &sub.state, "escape", Mods::default()).events),
        vec!["menu_back"]
    );
    let field = opened(rename());
    assert_eq!(
        verbs(&key(&field, &field.state, "escape", Mods::default()).events),
        vec!["menu_back"]
    );
}

#[test]
fn enter_picks_the_selection_and_the_arrows_skip_what_is_disabled() {
    let m = opened(root());
    let plain = Mods::default();
    let mut state = m.state.clone();

    let down = key(&m, &state, "down", plain);
    state = down.menu.unwrap();
    assert_eq!(state.selected, 2, "Pin tab");
    assert!(down.events.is_empty(), "navigation says nothing to Lua");

    state = key(&m, &state, "j", plain).menu.unwrap();
    assert_eq!(state.selected, 3, "Rename…");
    state = key(&m, &state, "down", plain).menu.unwrap();
    assert_eq!(state.selected, 5, "over the disabled row to Close tab");
    state = key(&m, &state, "down", plain).menu.unwrap();
    assert_eq!(state.selected, 5, "and stops at the end");

    let picked = key(&m, &state, "enter", plain);
    assert_eq!(verbs(&picked.events), vec!["menu_pick"]);
    assert_eq!(
        intent(&picked.events),
        &Intent::MenuPick {
            item_id: "close".into()
        }
    );

    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    let back = key(&m, &state, "down", shift).menu.unwrap();
    assert_eq!(back.selected, 3, "shift reverses the direction");
}

#[test]
fn a_letter_jumps_to_the_next_item_that_starts_with_it() {
    let m = opened(root());
    let plain = Mods::default();
    let jumped = key(&m, &m.state, "p", plain).menu.unwrap();
    assert_eq!(jumped.selected, 2, "Pin tab");
    let none = key(&m, &m.state, "z", plain).menu.unwrap();
    assert_eq!(none.selected, 1, "no match leaves it where it was");
    // an unknown key is swallowed, never handed to the shell
    assert!(key(&m, &m.state, "f5", plain).events.is_empty());
}

#[test]
fn the_rename_buffer_commits_the_text_rust_owns() {
    let m = opened(rename());
    let plain = Mods::default();
    let mut state = m.state.clone();
    assert_eq!(state.buffer, "nvim");

    state = key(&m, &state, "backspace", plain).menu.unwrap();
    state = key(&m, &state, "e", plain).menu.unwrap();
    assert_eq!(state.buffer, "nvie");

    let done = key(&m, &state, "enter", plain);
    assert_eq!(verbs(&done.events), vec!["rename_commit"]);
    assert_eq!(
        intent(&done.events),
        &Intent::RenameCommit {
            text: "nvie".into()
        }
    );
}

/// The "Move to space ▸" level as popover.lua composes it: the current space cannot be picked.
fn spaces() -> MenuMsg {
    msg(
        r#"{"rev":4,"open":true,"level":"spaces","anchor":{"row":4,"col":2},"target":7,"selected":1,
        "header":{"title":"Move to space","meta":"nvim"},
        "items":[{"id":"space:home","label":"Home","hint":"3","disabled":true},
                 {"id":"space:claude","label":"Claude","hint":"1"},
                 {"id":"space:pi","label":"pi"},
                 {"id":"space_auto","label":"Auto (follow rules)","disabled":true}]}"#,
    )
}

#[test]
fn the_spaces_level_steps_back_on_esc_and_never_rests_on_the_current_space() {
    let sub = opened(spaces());
    let plain = Mods::default();
    assert_eq!(
        verbs(&key(&sub, &sub.state, "escape", plain).events),
        vec!["menu_back"],
        "a sub-level, whatever its rows look like"
    );
    assert_eq!(
        sub.state.selected, 2,
        "selected:1 names the disabled current space, so the first live row takes the highlight"
    );
    let picked = key(&sub, &sub.state, "enter", plain);
    assert_eq!(verbs(&picked.events), vec!["menu_pick"]);
    assert_eq!(
        intent(&picked.events),
        &Intent::MenuPick {
            item_id: "space:claude".into()
        }
    );
    let up = key(&sub, &sub.state, "k", plain).menu.unwrap();
    assert_eq!(up.selected, 2, "k cannot climb onto the current space");
    let down = key(&sub, &up, "j", plain).menu.unwrap();
    assert_eq!(down.selected, 3);
    let past = key(&sub, &down, "j", plain).menu.unwrap();
    assert_eq!(past.selected, 3, "nor land on the disabled Auto row");
}

#[path = "../../../tests/support/scene.rs"]
mod test_scene;

use vtabs_engine::config::{ContextMode, HoverMode, Position, WheelMode};
use vtabs_engine::interaction::{Knobs, MirroredDrag, key, mouse};
use vtabs_engine::layout::{Part, Plan, RegionKind, plan};
use vtabs_engine::ui::{ArmKind, UiState};
use vtabs_protocol::types::{Button, Mods, Mouse, MouseKind};
use vtabs_protocol::{Event, Intent};

fn knobs<'a>(cols: i64) -> Knobs<'a> {
    Knobs {
        cols,
        position: Position::Left,
        double_click_ms: 300,
        tear_off: true,
        wheel: WheelMode::Scroll,
        context: ContextMode::Popover,
        hover_mode: HoverMode::Follow,
        slot_rows: 4,
        focus_on: false,
        focus_index: 1,
        ordered: &[],
        drag: None,
        scroll_top: 0,
        space_ids: &[],
        active_space: None,
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

fn wheel(dy: i8) -> Mouse {
    Mouse {
        kind: MouseKind::Wheel,
        button: Button::None,
        x: 5,
        y: 5,
        dy,
        mods: Mods::default(),
    }
}

fn action(events: &[Event]) -> Option<&'static str> {
    events.iter().find_map(|e| match e {
        Event::Intent { intent } => Some(intent.name()),
        _ => None,
    })
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

fn title_row(p: &Plan, rows: i64) -> i64 {
    (1..=rows)
        .find(|&y| {
            let r = p.at(y);
            r.kind == RegionKind::Tab && r.part == Some(Part::Title)
        })
        .expect("a card")
}

fn close_row(p: &Plan, rows: i64, x: i64) -> i64 {
    (1..=rows)
        .find(|&y| p.at(y).span(x) == Some("close"))
        .expect("a close span")
}

#[test]
fn a_press_on_the_card_body_activates_and_arms_the_drag() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let r = mouse(
        &p,
        &knobs(view.cols),
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 6, y as u16),
        1000,
    );
    assert_eq!(action(&r.events), Some("press_card"));
    let d = r.ui.drag.expect("the press armed a drag");
    assert_eq!((d.origin_x, d.origin_y, d.active), (6, y, false));
}

#[test]
fn the_close_button_acts_on_the_release_and_only_on_the_same_target() {
    let mut view = test_scene::sidebar();
    test_scene::hover_close(&mut view);
    let p = plan(&view);
    let x = view.hover.unwrap().x;
    let y = close_row(&p, view.rows, x);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, x as u16, y as u16),
        0,
    );
    assert!(down.events.is_empty(), "the press only arms");
    assert_eq!(down.ui.armed.map(|a| a.kind), Some(ArmKind::Close));

    let up = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Left, x as u16, y as u16),
        50,
    );
    assert_eq!(action(&up.events), Some("request_close"));

    let elsewhere = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Left, 3, y as u16),
        50,
    );
    assert_eq!(action(&elsewhere.events), None, "released off the ✕");
}

#[test]
fn motion_cancels_an_armed_close() {
    let mut view = test_scene::sidebar();
    test_scene::hover_close(&mut view);
    let p = plan(&view);
    let x = view.hover.unwrap().x;
    let y = close_row(&p, view.rows, x);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, x as u16, y as u16),
        0,
    );
    let dragged = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Drag, Button::Left, x as u16, y as u16 + 1),
        10,
    );
    assert!(dragged.ui.armed.is_none(), "the ✕ disarmed");
    let up = mouse(
        &p,
        &k,
        &dragged.ui,
        &ev(MouseKind::Release, Button::Left, x as u16, y as u16),
        20,
    );
    assert_eq!(action(&up.events), None);
}

#[test]
fn a_drag_needs_both_the_threshold_and_the_dwell() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 6, y as u16),
        0,
    );

    let too_soon = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Drag, Button::Left, 6, y as u16 + 3),
        10,
    );
    assert_eq!(
        action(&too_soon.events),
        None,
        "past the rows, inside the dwell"
    );

    let too_near = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Drag, Button::Left, 6, y as u16),
        500,
    );
    assert_eq!(action(&too_near.events), None, "past the dwell, no travel");

    let go = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Drag, Button::Left, 6, y as u16 + 3),
        500,
    );
    assert_eq!(action(&go.events), Some("drag_to"));
    assert!(go.ui.drag.unwrap().active);
}

#[test]
fn a_drag_armed_in_another_process_still_moves_here() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let mut k = knobs(view.cols);
    k.drag = Some(MirroredDrag {
        id: 2,
        active: true,
        origin_x: 6,
        origin_y: y,

        outside: false,
    });
    let r = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Drag, Button::Left, 6, y as u16 + 3),
        500,
    );
    assert_eq!(action(&r.events), Some("drag_to"));
    assert!(matches!(intent(&r.events), Intent::DragTo { slot, .. } if *slot > 0));
}

#[test]
fn travel_to_the_inner_edge_tears_off() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let mut k = knobs(view.cols);
    k.drag = Some(MirroredDrag {
        id: 2,
        active: true,
        origin_x: 6,
        origin_y: y,

        outside: false,
    });
    let edge = view.cols as u16;
    let r = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Drag, Button::Left, edge, y as u16),
        500,
    );
    assert!(
        matches!(intent(&r.events), Intent::DragTo { outside: true, .. }),
        "at the content edge"
    );

    let up = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Release, Button::Left, edge, y as u16),
        600,
    );
    assert_eq!(action(&up.events), Some("drag_end"));
    assert!(
        matches!(
            intent(&up.events),
            Intent::DragEnd {
                outside: true,
                slot: None
            }
        ),
        "a tear-off carries no slot"
    );
}

#[test]
fn the_wheel_scrolls_or_switches_by_config() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let k = knobs(view.cols);
    let r = mouse(&p, &k, &UiState::default(), &wheel(1), 0);
    assert_eq!(action(&r.events), Some("set_scroll"));
    assert!(matches!(
        intent(&r.events),
        Intent::SetScroll { top: 1, .. }
    ));
    assert_eq!(r.ui.scroll, Some(1));
    assert!(r.ui.user_scrolled);
    let again = mouse(&p, &k, &r.ui, &wheel(1), 10);
    assert_eq!(
        intent(&again.events),
        &Intent::SetScroll { top: 2, user: true },
        "it accumulates from the optimistic value"
    );

    let mut switch = knobs(view.cols);
    switch.wheel = WheelMode::Switch;
    let s = mouse(&p, &switch, &UiState::default(), &wheel(-1), 0);
    assert_eq!(action(&s.events), Some("wheel_tab"));
    assert_eq!(intent(&s.events), &Intent::WheelTab { dy: -1 });
}

#[test]
fn a_double_click_off_the_cards_opens_a_tab() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let empty = (1..=view.rows)
        .find(|&y| p.at(y).kind == RegionKind::Space)
        .expect("empty space");
    let k = knobs(view.cols);
    let one = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 5, empty as u16),
        0,
    );
    assert_eq!(action(&one.events), None, "one click is not two");
    let two = mouse(
        &p,
        &k,
        &one.ui,
        &ev(MouseKind::Press, Button::Left, 5, empty as u16),
        100,
    );
    assert_eq!(action(&two.events), Some("new_tab"));
}

#[test]
fn a_right_press_and_release_on_a_card_opens_the_menu() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Right, 6, y as u16),
        0,
    );
    assert_eq!(down.ui.armed.map(|a| a.kind), Some(ArmKind::Menu));
    let up = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Right, 6, y as u16),
        30,
    );
    assert_eq!(action(&up.events), Some("open_menu"));
    assert!(matches!(intent(&up.events), Intent::OpenMenu { row, .. } if *row == y));
}

#[test]
fn a_middle_click_closes_the_card_it_landed_on() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Middle, 6, y as u16),
        0,
    );
    let up = mouse(
        &p,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Middle, 6, y as u16),
        30,
    );
    assert_eq!(action(&up.events), Some("request_close"));
}

#[test]
fn a_strip_button_press_names_the_button() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let (y, span) = (1..=view.rows)
        .find_map(|y| {
            let r = p.at(y);
            (r.kind == RegionKind::Action).then(|| (y, r.spans[0].clone()))
        })
        .expect("an action row");
    let r = mouse(
        &p,
        &knobs(view.cols),
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, span.x1 as u16, y as u16),
        0,
    );
    match r.events.first() {
        Some(Event::Intent {
            intent: Intent::Strip { button_id },
        }) => assert_eq!(*button_id, span.id),
        other => panic!("expected a strip intent, got {other:?}"),
    }
}

#[test]
fn focus_keys_map_to_verbs_and_the_rest_fall_through() {
    let ids = [7i64, 8, 9];
    let mut k = knobs(28);
    k.focus_on = true;
    k.ordered = &ids;
    k.focus_index = 1;

    assert_eq!(
        action(&key(&k, "j", Mods::default(), b"j").events),
        Some("set_focus_index")
    );
    assert_eq!(
        intent(&key(&k, "j", Mods::default(), b"j").events),
        &Intent::SetFocusIndex { index: 2 }
    );
    assert_eq!(
        intent(&key(&k, "end", Mods::default(), b"").events),
        &Intent::SetFocusIndex { index: 3 }
    );
    assert_eq!(
        action(&key(&k, "escape", Mods::default(), b"").events),
        Some("blur_sidebar")
    );

    match key(&k, "2", Mods::default(), b"2").events.first() {
        Some(Event::Intent {
            intent: Intent::ActivateTab { tab_id },
        }) => assert_eq!(*tab_id, 8, "the Nth ordered item"),
        other => panic!("expected activate_tab, got {other:?}"),
    }

    assert!(matches!(
        key(&k, "r", Mods::default(), b"r").events.as_slice(),
        [Event::Intent {
            intent: Intent::RenameTab { tab_id: 7 }
        }]
    ));
    assert!(matches!(
        key(&k, "J", Mods::default(), b"J").events.as_slice(),
        [Event::Intent {
            intent: Intent::MoveTab {
                tab_id: 7,
                slot: 2,
                focus_index: 2
            }
        }]
    ));
    assert!(
        key(&k, "z", Mods::default(), b"z").events.is_empty(),
        "an unbound focus key is consumed without raw forwarding"
    );
}

#[test]
fn keys_flow_untouched_while_focus_is_off() {
    let r = key(&knobs(28), "j", Mods::default(), b"j");
    assert!(matches!(r.events.first(), Some(Event::Key { .. })));
}

fn switcher_row(p: &Plan, rows: i64) -> i64 {
    (1..=rows)
        .find(|&y| p.at(y).kind == RegionKind::Spaces)
        .expect("a switcher row")
}

fn switched_to(events: &[Event]) -> Option<String> {
    events.iter().find_map(|e| match e {
        Event::Intent {
            intent: Intent::SwitchSpace { space_id },
        } => Some(space_id.clone()),
        _ => None,
    })
}

#[test]
fn a_press_on_a_space_icon_switches_to_it_and_a_second_tap_opens_no_tab() {
    let mut view = test_scene::sidebar();
    test_scene::spaces(&mut view);
    let p = plan(&view);
    let y = switcher_row(&p, view.rows) as u16;
    let k = knobs(view.cols);
    let first = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 17, y),
        0,
    );
    assert_eq!(switched_to(&first.events).as_deref(), Some("pi"));
    assert_eq!(first.events.len(), 1);
    let second = mouse(
        &p,
        &k,
        &first.ui,
        &ev(MouseKind::Press, Button::Left, 17, y),
        100,
    );
    assert_eq!(switched_to(&second.events).as_deref(), Some("pi"));
    assert_ne!(
        action(&second.events),
        Some("new_tab"),
        "a double tap on the switcher is two switches, not a tab"
    );
    let own = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 14, y),
        0,
    );
    assert_eq!(
        switched_to(&own.events).as_deref(),
        Some("claude"),
        "the active slot names itself; Lua no-ops it"
    );
    let beside = mouse(
        &p,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 3, y),
        0,
    );
    assert!(
        beside.events.is_empty(),
        "the row is inert beside the icons"
    );
}

#[test]
fn the_wheel_over_the_switcher_steps_between_spaces_and_stops_at_the_ends() {
    let mut view = test_scene::sidebar();
    test_scene::spaces(&mut view);
    let p = plan(&view);
    let y = switcher_row(&p, view.rows) as u16;
    let ids = ["home", "claude", "pi"];
    let mut k = knobs(view.cols);
    k.space_ids = &ids;
    k.active_space = Some(1);
    let turn = |dy: i8| Mouse {
        kind: MouseKind::Wheel,
        button: Button::None,
        x: 14,
        y,
        dy,
        mods: Mods::default(),
    };
    let down = mouse(&p, &k, &UiState::default(), &turn(1), 0);
    assert_eq!(switched_to(&down.events).as_deref(), Some("pi"));
    assert!(
        down.ui.scroll.is_none(),
        "nothing is applied before the model comes back"
    );
    let up = mouse(&p, &k, &UiState::default(), &turn(-1), 0);
    assert_eq!(switched_to(&up.events).as_deref(), Some("home"));
    k.active_space = Some(2);
    let end = mouse(&p, &k, &UiState::default(), &turn(1), 0);
    assert!(end.events.is_empty(), "silent past the last space");
    let list = mouse(&p, &k, &UiState::default(), &wheel(1), 0);
    assert_eq!(
        action(&list.events),
        Some("set_scroll"),
        "away from the switcher the wheel is the list's"
    );
}

#[test]
fn the_space_cycling_keys_resolve_with_wraparound_in_focus_mode() {
    let mut k = knobs(28);
    k.focus_on = true;
    let ids = ["home", "work", "play"];
    k.space_ids = &ids;
    k.active_space = Some(0);
    assert_eq!(
        switched_to(&key(&k, "]", Mods::default(), b"]").events).as_deref(),
        Some("work")
    );
    assert_eq!(
        switched_to(&key(&k, "[", Mods::default(), b"[").events).as_deref(),
        Some("play")
    );
}

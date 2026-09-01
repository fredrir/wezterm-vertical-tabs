//! The gesture table of 05-p4b-spec.md, against the committed scenes' real layouts.

use vtabs_core::ui::{ArmKind, UiState};
use vtabs_input::resolve::{Knobs, MirroredDrag, key, mouse};
use vtabs_protocol::types::{Button, Mods, Mouse, MouseKind};
use vtabs_protocol::{DoArgs, DoId, Event};
use vtabs_view::enrich::PopoverHits;
use vtabs_view::layout::{Part, Plan, RegionKind, plan};
use vtabs_view::scene::RenderInput;

fn scene(name: &str) -> RenderInput {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugin/tests/golden/scenes")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn knobs<'a>(cols: i64) -> Knobs<'a> {
    Knobs {
        cols,
        position: "left",
        double_click_ms: 300,
        tear_off: true,
        wheel: "scroll",
        context: "popover",
        hover_mode: "follow",
        slot_rows: 4,
        focus_on: false,
        focus_index: 1,
        ordered: &[],
        drag: None,
        scroll_top: 0,
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
        Event::Do { a, .. } => Some(*a),
        _ => None,
    })
}

fn args(events: &[Event]) -> DoArgs {
    events
        .iter()
        .find_map(|e| match e {
            Event::Do { args, .. } => Some((**args).clone()),
            _ => None,
        })
        .expect("a do event")
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
    let view = scene("tabs");
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let r = mouse(
        &p,
        None,
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
    let view = scene("hover-close");
    let p = plan(&view);
    let x = view.hover.unwrap().x;
    let y = close_row(&p, view.rows, x);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, x as u16, y as u16),
        0,
    );
    assert!(down.events.is_empty(), "the press only arms");
    assert_eq!(down.ui.armed.map(|a| a.kind), Some(ArmKind::Close));

    let up = mouse(
        &p,
        None,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Left, x as u16, y as u16),
        50,
    );
    assert_eq!(action(&up.events), Some("request_close"));

    let elsewhere = mouse(
        &p,
        None,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Left, 3, y as u16),
        50,
    );
    assert_eq!(action(&elsewhere.events), None, "released off the ✕");
}

#[test]
fn motion_cancels_an_armed_close() {
    let view = scene("hover-close");
    let p = plan(&view);
    let x = view.hover.unwrap().x;
    let y = close_row(&p, view.rows, x);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, x as u16, y as u16),
        0,
    );
    let dragged = mouse(
        &p,
        None,
        &k,
        &down.ui,
        &ev(MouseKind::Drag, Button::Left, x as u16, y as u16 + 1),
        10,
    );
    assert!(dragged.ui.armed.is_none(), "the ✕ disarmed");
    let up = mouse(
        &p,
        None,
        &k,
        &dragged.ui,
        &ev(MouseKind::Release, Button::Left, x as u16, y as u16),
        20,
    );
    assert_eq!(action(&up.events), None);
}

#[test]
fn a_drag_needs_both_the_threshold_and_the_dwell() {
    let view = scene("tabs");
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 6, y as u16),
        0,
    );

    let too_soon = mouse(
        &p,
        None,
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
        None,
        &k,
        &down.ui,
        &ev(MouseKind::Drag, Button::Left, 6, y as u16),
        500,
    );
    assert_eq!(action(&too_near.events), None, "past the dwell, no travel");

    let go = mouse(
        &p,
        None,
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
    let view = scene("tabs");
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
    // no local press: the origin can only come from the model (§1.4)
    let r = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Drag, Button::Left, 6, y as u16 + 3),
        500,
    );
    assert_eq!(action(&r.events), Some("drag_to"));
    assert!(args(&r.events).slot.is_some());
}

#[test]
fn travel_to_the_inner_edge_tears_off() {
    let view = scene("tabs");
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
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Drag, Button::Left, edge, y as u16),
        500,
    );
    assert_eq!(args(&r.events).outside, Some(true), "at the content edge");

    let up = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Release, Button::Left, edge, y as u16),
        600,
    );
    assert_eq!(action(&up.events), Some("drag_end"));
    assert_eq!(args(&up.events).outside, Some(true));
    assert_eq!(args(&up.events).slot, None, "a tear-off carries no slot");
}

#[test]
fn the_wheel_scrolls_or_switches_by_config() {
    let view = scene("overflow");
    let p = plan(&view);
    let k = knobs(view.cols);
    let r = mouse(&p, None, &k, &UiState::default(), &wheel(1), 0);
    assert_eq!(action(&r.events), Some("set_scroll"));
    assert_eq!(args(&r.events).top, Some(1));
    assert_eq!(r.ui.scroll, Some(1));
    assert!(r.ui.user_scrolled);
    let again = mouse(&p, None, &k, &r.ui, &wheel(1), 10);
    assert_eq!(
        args(&again.events).top,
        Some(2),
        "it accumulates from the optimistic value"
    );

    let mut switch = knobs(view.cols);
    switch.wheel = "switch";
    let s = mouse(&p, None, &switch, &UiState::default(), &wheel(-1), 0);
    assert_eq!(action(&s.events), Some("wheel_tab"));
    assert_eq!(args(&s.events).dy, Some(-1));
}

#[test]
fn a_double_click_off_the_cards_opens_a_tab() {
    let view = scene("tabs");
    let p = plan(&view);
    let empty = (1..=view.rows)
        .find(|&y| p.at(y).kind == RegionKind::Space)
        .expect("empty space");
    let k = knobs(view.cols);
    let one = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 5, empty as u16),
        0,
    );
    assert_eq!(action(&one.events), None, "one click is not two");
    let two = mouse(
        &p,
        None,
        &k,
        &one.ui,
        &ev(MouseKind::Press, Button::Left, 5, empty as u16),
        100,
    );
    assert_eq!(action(&two.events), Some("new_tab"));
}

#[test]
fn a_right_press_and_release_on_a_card_opens_the_menu() {
    let view = scene("tabs");
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Right, 6, y as u16),
        0,
    );
    assert_eq!(down.ui.armed.map(|a| a.kind), Some(ArmKind::Menu));
    let up = mouse(
        &p,
        None,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Right, 6, y as u16),
        30,
    );
    assert_eq!(action(&up.events), Some("open_menu"));
    assert_eq!(args(&up.events).row, Some(y));
}

#[test]
fn a_middle_click_closes_the_card_it_landed_on() {
    let view = scene("tabs");
    let p = plan(&view);
    let y = title_row(&p, view.rows);
    let k = knobs(view.cols);
    let down = mouse(
        &p,
        None,
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Middle, 6, y as u16),
        0,
    );
    let up = mouse(
        &p,
        None,
        &k,
        &down.ui,
        &ev(MouseKind::Release, Button::Middle, 6, y as u16),
        30,
    );
    assert_eq!(action(&up.events), Some("request_close"));
}

#[test]
fn a_strip_button_press_names_the_button() {
    let view = scene("tabs");
    let p = plan(&view);
    let (y, span) = (1..=view.rows)
        .find_map(|y| {
            let r = p.at(y);
            (r.kind == RegionKind::Action).then(|| (y, r.spans[0].clone()))
        })
        .expect("an action row");
    let r = mouse(
        &p,
        None,
        &knobs(view.cols),
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, span.x1 as u16, y as u16),
        0,
    );
    match r.events.first() {
        Some(Event::Do {
            a,
            id: Some(DoId::Name(name)),
            ..
        }) => {
            assert_eq!(*a, "strip");
            assert_eq!(*name, span.id);
        }
        other => panic!("expected a strip do, got {other:?}"),
    }
}

#[test]
fn the_popover_reports_its_input_raw_and_interprets_nothing() {
    let view = scene("popover-open");
    let p = plan(&view);
    let rect = view.popover.as_ref().expect("the scene has a menu");
    let pop = PopoverHits {
        x: rect.x,
        y: rect.y,
        w: rect.w.unwrap_or(20),
        h: rect.h,
        rows: vec![(Some("close".into()), false), (None, true)],
    };
    let k = knobs(view.cols);
    let inside = mouse(
        &p,
        Some(&pop),
        &k,
        &UiState::default(),
        &ev(
            MouseKind::Press,
            Button::Left,
            (rect.x + 1) as u16,
            rect.y as u16,
        ),
        0,
    );
    let a = args(&inside.events);
    assert_eq!(action(&inside.events), Some("popover_mouse"));
    assert_eq!(
        (a.k, a.kind, a.inside),
        (Some("down"), Some("popover"), Some(true))
    );
    assert_eq!(a.id.as_deref(), Some("close"));

    let scrim = mouse(
        &p,
        Some(&pop),
        &k,
        &UiState::default(),
        &ev(MouseKind::Press, Button::Left, 1, 1),
        0,
    );
    assert_eq!(args(&scrim.events).kind, Some("scrim"));

    let w = mouse(&p, Some(&pop), &k, &UiState::default(), &wheel(1), 0);
    assert_eq!(
        action(&w.events),
        Some("popover_wheel"),
        "the wheel moves the menu, not the list"
    );
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
        args(&key(&k, "j", Mods::default(), b"j").events).index,
        Some(2)
    );
    assert_eq!(
        args(&key(&k, "end", Mods::default(), b"").events).index,
        Some(3)
    );
    assert_eq!(
        action(&key(&k, "escape", Mods::default(), b"").events),
        Some("blur_sidebar")
    );

    match key(&k, "2", Mods::default(), b"2").events.first() {
        Some(Event::Do {
            a,
            id: Some(DoId::Tab(id)),
            ..
        }) => assert_eq!((*a, *id), ("activate_tab_by_id", 8), "the Nth ordered item"),
        other => panic!("expected activate_tab_by_id, got {other:?}"),
    }

    // no `do` verb exists for rename, so Lua's own focus branch must still see the key
    let rename = key(&k, "r", Mods::default(), b"r");
    assert!(matches!(rename.events.first(), Some(Event::Key { .. })));
}

#[test]
fn keys_flow_untouched_while_focus_is_off() {
    let r = key(&knobs(28), "j", Mods::default(), b"j");
    assert!(matches!(r.events.first(), Some(Event::Key { .. })));
}

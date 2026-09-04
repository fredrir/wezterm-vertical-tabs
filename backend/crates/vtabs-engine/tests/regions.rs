//! Integration coverage for the regions produced by sidebar layout.

#[path = "support/scene.rs"]
mod test_scene;

use vtabs_engine::config::{Position, TabHeight};
use vtabs_engine::layout::{Part, RegionKind, on_inner_edge, plan};
use vtabs_engine::scene::FooterEntry;

fn first_title_row(p: &vtabs_engine::layout::Plan, rows: i64) -> i64 {
    (1..=rows)
        .find(|&y| {
            let r = p.at(y);
            r.kind == RegionKind::Tab && r.part == Some(Part::Title)
        })
        .expect("a title row")
}

#[test]
fn a_card_owns_every_row_of_its_slot() {
    let mut view = test_scene::sidebar();
    view.cfg.tab_height = TabHeight::Tall;
    let p = plan(&view);
    // run_layout addendum 6: tall makes a five-row card, pad/pad/title/pad/pad, all one slot
    let title = (1..=view.rows)
        .find(|&y| {
            let r = p.at(y);
            r.kind == RegionKind::Tab && r.part == Some(Part::Title) && p.at(y - 1).slot == r.slot
        })
        .expect("a title row with a pad above it");
    let slot = p.at(title).slot;
    assert!(
        slot.is_some() && p.at(title).id.is_some(),
        "the row names its tab and slot"
    );
    assert_eq!(p.at(title - 1).part, Some(Part::Pad));
    assert_eq!(p.at(title - 2).part, Some(Part::Pad));
    assert_eq!(p.at(title + 1).part, Some(Part::Pad));
    assert_eq!(p.at(title + 2).part, Some(Part::Pad));
    for row in title - 2..=title + 2 {
        assert_eq!(p.at(row).slot, slot, "all five rows are one card");
        assert_eq!(p.at(row).id, p.at(title).id);
    }
}

#[test]
fn the_card_grid_gives_up_a_column_to_the_frame() {
    let mut framed = test_scene::sidebar();
    framed.cfg.frame = true;
    let plain = test_scene::sidebar();
    let (fp, pp) = (plan(&framed), plan(&plain));
    let f = fp.at(first_title_row(&fp, framed.rows));
    let p = pp.at(first_title_row(&pp, plain.rows));
    assert_eq!(f.x2, Some(26), "framed card stops a column early");
    assert_eq!(p.x2, Some(27), "and keeps it when frame is off");
    assert_eq!(f.x1, Some(2));
}

#[test]
fn the_whole_rail_is_the_card_and_it_has_no_close_span() {
    let mut view = test_scene::sidebar();
    test_scene::rail(&mut view, 9);
    let p = plan(&view);
    let title = first_title_row(&p, view.rows);
    let at = p.at(title);
    assert_eq!(at.x1, Some(1), "the whole rail is the card");
    assert_eq!(at.x2, Some(view.cols));
    let icon_x = (view.cols as f64 / 2.0).ceil() as i64;
    assert_eq!(
        at.span(icon_x),
        None,
        "a rail card has no close span, by construction"
    );
    assert!(at.in_card(1) && at.in_card(view.cols));
}

#[test]
fn hover_over_the_close_span_names_it() {
    let mut view = test_scene::sidebar();
    test_scene::hover_close(&mut view);
    let p = plan(&view);
    let hover = view.hover.expect("the scene hovers a close button");
    let at = p.at(hover.y);
    assert_eq!(at.kind, RegionKind::Tab, "the pointer is on a card");
    assert!(at.in_card(hover.x), "and on its surface");
    // the ✕ lives on the card's title row, which is the row the close span is armed from
    let title = (1..=view.rows)
        .find(|&y| {
            let r = p.at(y);
            r.kind == RegionKind::Tab && r.id == at.id && r.part == Some(Part::Title)
        })
        .expect("the hovered card has a title row");
    assert_eq!(p.at(title).span(hover.x), Some("close"));
}

#[test]
fn the_strip_row_carries_one_span_per_button() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let strip = (1..=view.rows)
        .find(|&y| p.at(y).kind == RegionKind::Action)
        .expect("an action row");
    let at = p.at(strip);
    assert_eq!(at.spans.len(), view.strip_buttons.len());
    for button in &view.strip_buttons {
        let span = at.spans.iter().find(|s| s.id == button.id).expect("a span");
        assert_eq!(at.span(span.x1), Some(button.id.as_str()));
        assert_eq!(at.span(span.x2), Some(button.id.as_str()));
    }
}

#[test]
fn the_ghost_and_the_footer_name_themselves() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    assert!(
        (1..=view.rows).any(|y| p.at(y).kind == RegionKind::NewTab),
        "the ghost card answers as new_tab"
    );
    let mut with_footer = test_scene::sidebar();
    with_footer.footer.push(FooterEntry {
        text: "branch".into(),
        icon: None,
        id: Some("branch".into()),
        fg: None,
        bg: None,
        icon_fg: None,
    });
    let fp = plan(&with_footer);
    let row = (1..=with_footer.rows)
        .find(|&y| fp.at(y).kind == RegionKind::Footer)
        .expect("a footer row");
    assert_eq!(fp.at(row).index, Some(1), "addressed by model index");
}

#[test]
fn drop_slot_walks_to_the_next_card_and_past_the_last() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let title = first_title_row(&p, view.rows);
    let slot = p.at(title).slot.expect("a slot");
    assert_eq!(p.drop_slot(title), slot, "on the card, its own slot");
    let last_slot = (1..=view.rows)
        .filter_map(|y| p.at(y).slot)
        .max()
        .expect("a slot");
    assert_eq!(
        p.drop_slot(view.rows),
        last_slot + 1,
        "below every card, the slot after the last"
    );
}

#[test]
fn a_row_off_the_end_is_inert_space() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    let off = p.at(view.rows + 50);
    assert_eq!(off.kind, RegionKind::Space);
    assert!(!off.in_card(1), "space owns no card surface");
    assert_eq!(off.span(1), None);
}

#[test]
fn the_inner_edge_follows_the_sidebar_position() {
    assert!(on_inner_edge(28, 28, Position::Left));
    assert!(!on_inner_edge(27, 28, Position::Left));
    assert!(on_inner_edge(1, 28, Position::Right));
    assert!(!on_inner_edge(2, 28, Position::Right));
}

fn switcher_row(p: &vtabs_engine::layout::Plan, rows: i64) -> i64 {
    (1..=rows)
        .find(|&y| p.at(y).kind == RegionKind::Spaces)
        .expect("a switcher row")
}

#[test]
fn the_switcher_is_the_outermost_row_and_carries_one_span_per_space() {
    let mut view = test_scene::sidebar();
    test_scene::spaces(&mut view);
    let p = plan(&view);
    let row = switcher_row(&p, view.rows);
    assert_eq!(row, view.rows, "below the footer, above nothing");
    assert_eq!(
        p.at(row - 1).kind,
        RegionKind::Space,
        "with a blank row above it"
    );
    let at = p.at(row);
    assert_eq!(at.spans.len(), view.spaces.len());
    // centred in the 26-column card on the strip's 3-cell pitch
    for (space, x) in view.spaces.iter().zip([11, 14, 17]) {
        for col in x - 1..=x + 1 {
            assert_eq!(at.span(col), Some(space.id.as_str()), "column {col}");
        }
    }
    assert_eq!(at.span(9), None, "the row is inert beside the icons");
    assert_eq!(at.span(19), None);
}

#[test]
fn a_narrow_rail_shows_the_active_space_and_its_click_cycles() {
    let mut view = test_scene::sidebar();
    test_scene::spaces(&mut view);
    test_scene::rail(&mut view, 9);
    view.cols = 5;
    let p = plan(&view);
    let at = p.at(switcher_row(&p, view.rows));
    assert_eq!(at.spans.len(), 1, "five columns hold one icon");
    assert_eq!((at.x1, at.x2), (Some(2), Some(4)));
    // `claude` is active; the lone slot answers for the space after it
    assert_eq!(at.span(3), Some("pi"));
}

#[test]
fn without_spaces_no_row_is_a_switcher() {
    let view = test_scene::sidebar();
    let p = plan(&view);
    assert!((1..=view.rows).all(|y| p.at(y).kind != RegionKind::Spaces));
}

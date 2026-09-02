use super::*;

fn spaces(n: usize, active: usize) -> Vec<SpaceEntry> {
    (0..n)
        .map(|i| SpaceEntry {
            id: format!("s{i}"),
            name: format!("s{i}"),
            icon: i.to_string(),
            is_active: i == active,
            has_unseen: false,
        })
        .collect()
}

fn columns(slots: &[SpaceSlot]) -> Vec<i64> {
    slots.iter().map(|s| s.x).collect()
}

#[test]
fn the_switcher_takes_two_rows_when_the_pane_can_spare_them_and_none_without_spaces() {
    assert_eq!(spaces_rows(20, 3), 2);
    assert_eq!(
        spaces_rows(5, 3),
        2,
        "gap, icons and a 3-row list still fit"
    );
    assert_eq!(spaces_rows(4, 3), 1, "the gap goes first");
    assert_eq!(spaces_rows(3, 3), 1);
    assert_eq!(spaces_rows(2, 3), 0, "then the row itself");
    assert_eq!(spaces_rows(20, 0), 0);
}

#[test]
fn slots_sit_centred_on_the_strip_pitch_and_their_spans_touch() {
    let slots = space_slots(&spaces(3, 1), &rail_grid(9), 3, 9);
    assert_eq!(columns(&slots), vec![2, 5, 8]);
    assert_eq!((slots[0].x1, slots[0].x2), (1, 3));
    assert_eq!((slots[2].x1, slots[2].x2), (7, 9));
    assert!(slots[1].active && !slots[0].active);
    assert!(
        slots.iter().all(|s| !s.cut),
        "nothing hidden, nothing faded"
    );
    let two = space_slots(&spaces(2, 0), &rail_grid(9), 3, 9);
    assert_eq!(
        columns(&two),
        vec![3, 6],
        "two icons centre between the same edges"
    );
}

#[test]
fn past_the_width_a_window_around_the_active_space_shows_with_faded_cut_ends() {
    let late = space_slots(&spaces(5, 3), &rail_grid(9), 3, 9);
    let ids: Vec<&str> = late.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["s2", "s3", "s4"]);
    assert_eq!(
        late.iter().map(|s| s.cut).collect::<Vec<_>>(),
        vec![true, false, false],
        "the list continues to the left only"
    );
    let early = space_slots(&spaces(5, 0), &rail_grid(9), 3, 9);
    let ids: Vec<&str> = early.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["s0", "s1", "s2"]);
    assert!(early[2].cut && !early[0].cut);
}

#[test]
fn a_lone_slot_shows_the_active_space_and_targets_the_next_one() {
    let slots = space_slots(&spaces(3, 1), &rail_grid(5), 3, 5);
    assert_eq!(slots.len(), 1, "a five-column rail holds one icon");
    assert_eq!(slots[0].x, 3);
    assert_eq!((slots[0].icon.as_str(), slots[0].active), ("1", true));
    assert_eq!(slots[0].id, "s2", "a click steps on");
    let last = space_slots(&spaces(3, 2), &rail_grid(5), 3, 5);
    assert_eq!(
        last[0].id, "s0",
        "and wraps, or the last space would be a dead end"
    );
    let only = space_slots(&spaces(1, 0), &rail_grid(5), 3, 5);
    assert_eq!(
        only[0].id, "s0",
        "with one space there is nowhere else to go"
    );
}

#[test]
fn the_neighbour_stops_at_the_ends_and_needs_an_active_space() {
    let ids = ["a", "b", "c"];
    assert_eq!(space_neighbour(&ids, Some(1), 1).as_deref(), Some("c"));
    assert_eq!(space_neighbour(&ids, Some(1), -1).as_deref(), Some("a"));
    assert_eq!(space_neighbour(&ids, Some(2), 1), None);
    assert_eq!(space_neighbour(&ids, Some(0), -1), None);
    assert_eq!(space_neighbour(&ids, None, 1), None);
}

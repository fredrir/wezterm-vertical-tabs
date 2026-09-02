use vtabs_engine::{
    frame::Cell,
    fx::{frame_at, phase_named},
};

#[test]
fn a_fade_changes_colour_only() {
    let red = [200, 40, 40];
    let bg = [30, 30, 46];
    let row = vec![Cell::new(Some('h'), red, bg), Cell::new(None, red, bg)];
    let rows = vec![Some(row.clone()), None];
    let p = phase_named("expand_in").unwrap();
    let mid = frame_at(&rows, &[0], &[0], &p, bg, 110);
    let cells = mid[0].as_ref().unwrap();
    assert_eq!(cells[0].ch, Some('h'));
    assert_eq!(cells[1].ch, None);
    assert_ne!(cells[0].fg, red, "mid-fade is between anchor and final");
    let end = frame_at(&rows, &[0], &[0], &p, bg, 220);
    assert_eq!(
        end[0].as_ref().unwrap()[0].fg,
        red,
        "the run lands on the final colour"
    );
    let start = frame_at(&rows, &[0], &[0], &p, bg, 0);
    assert_eq!(
        start[0].as_ref().unwrap()[0].fg,
        bg,
        "and starts at the anchor"
    );
    let out = phase_named("expand_out").unwrap();
    assert_eq!(
        frame_at(&rows, &[0], &[0], &out, bg, 0)[0]
            .as_ref()
            .unwrap()[0]
            .fg,
        red
    );
}

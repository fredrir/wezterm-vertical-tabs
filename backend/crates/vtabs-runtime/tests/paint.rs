use vtabs_engine::frame::Cell;
use vtabs_runtime::paint::changed_rows_bytes;

const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";

fn row(text: &str) -> Option<Vec<Cell>> {
    Some(
        text.chars()
            .map(|c| Cell::new(Some(c), [200, 200, 200], [30, 30, 46]))
            .collect(),
    )
}

#[test]
fn only_the_rows_that_differ_are_written() {
    let bg = [30, 30, 46];
    let shown = vec![row("one"), row("two"), None];
    let next = vec![row("one"), row("deux"), row("new")];
    let fades = vec![None, None, None];
    let out = changed_rows_bytes(&next, &fades, bg, &shown, &fades).expect("two rows moved");
    assert!(
        !out.contains("\x1b[1;1H"),
        "the unchanged row is skipped: {out:?}"
    );
    assert!(out.contains("\x1b[2;1H") && out.contains("deux"));
    assert!(
        out.contains("\x1b[3;1H") && out.contains("new"),
        "a row newly painted counts"
    );
    assert!(out.starts_with(SYNC_BEGIN) && out.ends_with(SYNC_END));
}

#[test]
fn an_identical_frame_writes_nothing_and_a_fade_change_writes_its_row() {
    let bg = [30, 30, 46];
    let rows = vec![row("one"), row("two")];
    let fades = vec![None, None];
    assert!(changed_rows_bytes(&rows, &fades, bg, &rows, &fades).is_none());
    let faded = vec![None, Some(0.5)];
    let out = changed_rows_bytes(&rows, &faded, bg, &rows, &fades).expect("one row faded");
    assert!(!out.contains("\x1b[1;1H") && out.contains("\x1b[2;1H"));
}

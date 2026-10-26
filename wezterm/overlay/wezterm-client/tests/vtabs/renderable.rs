use super::{same_line_contents, update_line, LineEntry};
use std::time::{Duration, Instant};
use termwiz::cell::{CellAttributes, Underline};
use termwiz::color::AnsiColor;
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::SequenceNo;
use wezterm_term::Line;

fn line(text: &str, seqno: SequenceNo) -> Line {
    Line::from_text(text, &CellAttributes::default(), seqno, None)
}

fn cached(entry: Option<LineEntry>) -> Line {
    match entry {
        Some(LineEntry::Line(line)) => line,
        other => panic!("expected a current line, got {:?}", other),
    }
}

#[test]
fn repeated_cursor_rows_preserve_selection_seqno() {
    let mut seqno = 40;
    let mut prompt = line("user@archie 界👩‍💻 $ ", seqno);
    prompt.scan_and_create_hyperlinks(&[]);
    for remote_seqno in [0, 1, 40, 100, 1000] {
        let mut incoming = line("user@archie 界👩‍💻 $ ", remote_seqno);
        incoming.compress_for_scrollback();
        // Hyperlink scan state and storage representation are not content changes.
        prompt = cached(update_line(
            Some(LineEntry::Line(prompt)),
            incoming,
            None,
            &mut seqno,
        ));
        assert_eq!(seqno, 40, "copy mode must not see a new version");
        assert!(!prompt.changed_since(40), "mouse selection must stay clean");
    }
}

#[test]
fn identical_fetches_and_stale_lines_keep_their_local_stamp() {
    let now = Instant::now();
    for (prior, fetch_start) in [
        (LineEntry::Stale(line("prompt", 40)), None),
        (LineEntry::LineAndFetching(line("prompt", 40), now), None),
        (
            LineEntry::LineAndFetching(line("prompt", 40), now),
            Some(now),
        ),
    ] {
        let mut seqno = 50;
        let result = cached(update_line(
            Some(prior),
            line("prompt", 100),
            fetch_start,
            &mut seqno,
        ));
        assert_eq!(seqno, 50);
        assert_eq!(result.current_seqno(), 40);
    }
}

#[test]
fn delayed_real_output_is_dirty_after_the_last_relay_update() {
    let now = Instant::now();
    let mut seqno = 100;
    let result = cached(update_line(
        Some(LineEntry::LineAndFetching(line("old output", 40), now)),
        line("new output", 41),
        Some(now),
        &mut seqno,
    ));
    assert!(seqno > 100);
    assert!(result.changed_since(100));
    assert_eq!(result.current_seqno(), seqno);

    // The server can send a dirty cursor row twice in the same update.
    let before_duplicate = seqno;
    let result = cached(update_line(
        Some(LineEntry::Line(result)),
        line("new output", 41),
        None,
        &mut seqno,
    ));
    assert_eq!(seqno, before_duplicate);
    assert!(!result.changed_since(before_duplicate));
}

#[test]
fn first_arrivals_are_dirty_even_with_a_zero_upstream_stamp() {
    let now = Instant::now();
    for (prior, fetch_start, remote_seqno) in [
        (None, None, 0),
        (Some(LineEntry::Fetching(now)), Some(now), 0),
        (None, None, 100),
    ] {
        let mut seqno = 40;
        let result = cached(update_line(
            prior,
            line("new row", remote_seqno),
            fetch_start,
            &mut seqno,
        ));
        assert!(seqno > 40);
        assert!(seqno >= remote_seqno);
        assert_eq!(result.current_seqno(), seqno);
        assert!(result.changed_since(40));
        assert!(!result.changed_since(seqno));
    }
}

#[test]
fn overtaken_fetches_do_not_advance_the_version_or_replace_content() {
    let now = Instant::now();
    let newer = now + Duration::from_millis(1);
    for prior in [
        None,
        Some(LineEntry::Line(line("pushed output", 40))),
        Some(LineEntry::Stale(line("pushed output", 40))),
        Some(LineEntry::Fetching(newer)),
        Some(LineEntry::LineAndFetching(line("pushed output", 40), newer)),
    ] {
        let expected_kind = prior.as_ref().map(LineEntry::kind);
        let mut seqno = 40;
        let result = update_line(prior, line("outdated output", 1000), Some(now), &mut seqno);
        assert_eq!(seqno, 40);
        assert_eq!(result.as_ref().map(LineEntry::kind), expected_kind);
        match result {
            Some(
                LineEntry::Line(line)
                | LineEntry::Stale(line)
                | LineEntry::LineAndFetching(line, _),
            ) => {
                assert_eq!(line.as_str(), "pushed output");
                assert_eq!(line.current_seqno(), 40);
            }
            _ => {}
        }
    }
}

#[test]
fn rendering_and_interaction_changes_still_reach_the_gui() {
    let old = line("prompt", 40);
    let mut underline = CellAttributes::default();
    underline.set_underline(Underline::Single);
    let mut color = CellAttributes::default();
    color.set_foreground(AnsiColor::Red);
    let mut link = CellAttributes::default();
    link.set_hyperlink(Some(std::sync::Arc::new(Hyperlink::new(
        "https://example.com",
    ))));
    let mut double_width = old.clone();
    double_width.set_double_width(40);
    let mut double_top = old.clone();
    double_top.set_double_height_top(40);
    let mut double_bottom = old.clone();
    double_bottom.set_double_height_bottom(40);
    let mut bidi = old.clone();
    bidi.set_bidi_enabled(true, 40);

    for incoming in [
        line("Prompt", 40),
        line("prompt ", 40),
        Line::from_text("prompt", &underline, 40, None),
        Line::from_text("prompt", &color, 40, None),
        Line::from_text("prompt", &link, 40, None),
        Line::from_text_with_wrapped_last_col("prompt", &CellAttributes::default(), 40),
        double_width,
        double_top,
        double_bottom,
        bidi,
    ] {
        assert!(!same_line_contents(&old, &incoming));
        let mut seqno = 40;
        let result = cached(update_line(
            Some(LineEntry::Line(old.clone())),
            incoming,
            None,
            &mut seqno,
        ));
        assert!(seqno > 40);
        assert!(result.changed_since(40));
    }
}

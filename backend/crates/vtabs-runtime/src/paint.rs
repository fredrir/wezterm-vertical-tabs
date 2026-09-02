//! Encodes a laid-out frame the way render.lua's `emit`/`paint` do, byte for byte: one CUP per
//! painted row, SGR only where a run changes, and the cursor hidden for the whole frame, all
//! inside one DEC 2026 synchronized update so a resize never shows half a frame.
//!
//! A repaint over a frame the pane already shows writes only the rows that differ: every byte a
//! sidebar emits is parsed by the terminal, shipped through any mux in between and re-rendered by
//! the GUI, so a hover that moves one row costs two rows, not the whole pane.

use vtabs_view::frame::{Cell, Rgb, faded};
use vtabs_view::render::Frame;

const ESC: char = '\x1b';
const HIDE_CURSOR: &str = "\x1b[?25l";
const SYNC_BEGIN: &str = "\x1b[?2026h";
const SYNC_END: &str = "\x1b[?2026l";
const RESET: &str = "\x1b[0m";

fn cup(out: &mut String, row: usize) {
    out.push(ESC);
    out.push_str(&format!("[{row};1H"));
}

fn sgr_fg(out: &mut String, c: Rgb) {
    out.push(ESC);
    out.push_str(&format!("[38;2;{};{};{}m", c[0], c[1], c[2]));
}

fn sgr_bg(out: &mut String, c: Rgb) {
    out.push(ESC);
    out.push_str(&format!("[48;2;{};{};{}m", c[0], c[1], c[2]));
}

fn same(a: &Cell, b: &Cell) -> bool {
    a.scrim == b.scrim && a.fg == b.fg && a.bg == b.bg && a.bold == b.bold
}

/// One row's escapes. An all-blank run omits the fg code, exactly as `emit()` does, so the row
/// inherits the fg it last set rather than paying for one it cannot show.
fn row_bytes(cells: &[Cell], page_bg: Rgb, fade: Option<f64>) -> String {
    let mut out = String::new();
    let mut run: Option<Cell> = None;
    let mut body = String::new();

    let flush = |run: &Option<Cell>, body: &mut String, out: &mut String| {
        if let Some(r) = run.filter(|_| !body.is_empty()) {
            sgr_bg(out, faded(r.bg, page_bg, r.scrim));
            if !body.chars().all(|c| c == ' ') {
                sgr_fg(out, faded(r.fg, page_bg, fade.unwrap_or(r.scrim)));
            }
            if r.bold {
                out.push_str("\x1b[1m");
            }
            out.push_str(body);
            if r.bold {
                out.push_str("\x1b[22m");
            }
        }
        body.clear();
    };

    for cell in cells {
        let Some(ch) = cell.ch else { continue };
        if !matches!(run, Some(r) if same(&r, cell)) {
            flush(&run, &mut body, &mut out);
            run = Some(*cell);
        }
        body.push(ch);
    }
    flush(&run, &mut body, &mut out);
    out.push_str(RESET);
    out
}

/// One synchronized update around the rows written.
fn framed(body: &str) -> String {
    format!("{SYNC_BEGIN}{HIDE_CURSOR}{body}{RESET}{SYNC_END}")
}

/// The whole frame as one write: an unpainted row is skipped, keeping whatever the pane had there.
pub fn rows_bytes(rows: &[Option<Vec<Cell>>], fades: &[Option<f64>], page_bg: Rgb) -> String {
    let mut body = String::new();
    for (i, cells) in rows.iter().enumerate() {
        let Some(cells) = cells else { continue };
        cup(&mut body, i + 1);
        body.push_str(&row_bytes(cells, page_bg, fades.get(i).copied().flatten()));
    }
    framed(&body)
}

/// Only the rows that differ from the frame on screen, or `None` when nothing does. Both frames
/// must share `page_bg`: it is mixed into every scrimmed cell, so a different page is a different
/// row even where the cells agree. A row no longer painted is left as it was, as `rows_bytes` leaves it.
pub fn changed_rows_bytes(
    rows: &[Option<Vec<Cell>>],
    fades: &[Option<f64>],
    page_bg: Rgb,
    shown_rows: &[Option<Vec<Cell>>],
    shown_fades: &[Option<f64>],
) -> Option<String> {
    let mut body = String::new();
    for (i, cells) in rows.iter().enumerate() {
        let Some(cells) = cells else { continue };
        let fade = fades.get(i).copied().flatten();
        let same_cells = shown_rows
            .get(i)
            .is_some_and(|shown| shown.as_deref() == Some(cells.as_slice()));
        if same_cells && shown_fades.get(i).copied().flatten() == fade {
            continue;
        }
        cup(&mut body, i + 1);
        body.push_str(&row_bytes(cells, page_bg, fade));
    }
    if body.is_empty() {
        return None;
    }
    Some(framed(&body))
}

pub fn frame_bytes(frame: &Frame, page_bg: Rgb) -> String {
    rows_bytes(&frame.cells, &frame.fades, page_bg)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

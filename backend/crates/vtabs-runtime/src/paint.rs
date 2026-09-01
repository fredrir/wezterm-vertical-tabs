//! Encodes a laid-out frame the way render.lua's `emit`/`paint` do, byte for byte: one CUP per
//! painted row, SGR only where a run changes, and the cursor hidden for the whole frame.

use vtabs_view::frame::{Cell, Rgb, faded};
use vtabs_view::render::Frame;

const ESC: char = '\x1b';
const HIDE_CURSOR: &str = "\x1b[?25l";
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

/// The whole frame as one write: an unpainted row is skipped, keeping whatever the pane had there.
pub fn rows_bytes(rows: &[Option<Vec<Cell>>], fades: &[Option<f64>], page_bg: Rgb) -> String {
    let mut out = String::from(HIDE_CURSOR);
    for (i, cells) in rows.iter().enumerate() {
        let Some(cells) = cells else { continue };
        cup(&mut out, i + 1);
        out.push_str(&row_bytes(cells, page_bg, fades.get(i).copied().flatten()));
    }
    out.push_str(RESET);
    out
}

pub fn frame_bytes(frame: &Frame, page_bg: Rgb) -> String {
    rows_bytes(&frame.cells, &frame.fades, page_bg)
}

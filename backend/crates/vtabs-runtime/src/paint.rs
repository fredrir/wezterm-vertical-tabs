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
pub fn frame_bytes(frame: &Frame, page_bg: Rgb) -> String {
    let mut out = String::from(HIDE_CURSOR);
    for (i, cells) in frame.cells.iter().enumerate() {
        let Some(cells) = cells else { continue };
        cup(&mut out, i + 1);
        out.push_str(&row_bytes(cells, page_bg, frame.fades[i]));
    }
    out.push_str(RESET);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(ch: char, fg: Rgb, bg: Rgb) -> Cell {
        Cell::new(Some(ch), fg, bg)
    }

    #[test]
    fn a_run_pays_for_its_colours_once() {
        let red = [255, 0, 0];
        let black = [0, 0, 0];
        let row = vec![cell('h', red, black), cell('i', red, black)];
        assert_eq!(
            row_bytes(&row, black, None),
            "\x1b[48;2;0;0;0m\x1b[38;2;255;0;0mhi\x1b[0m"
        );
    }

    #[test]
    fn an_all_blank_run_omits_the_foreground() {
        let black = [0, 0, 0];
        let row = vec![cell(' ', [1, 2, 3], black)];
        assert_eq!(row_bytes(&row, black, None), "\x1b[48;2;0;0;0m \x1b[0m");
    }

    #[test]
    fn bold_is_opened_and_closed_around_its_run() {
        let black = [0, 0, 0];
        let mut c = cell('x', [9, 9, 9], black);
        c.bold = true;
        assert_eq!(
            row_bytes(&[c], black, None),
            "\x1b[48;2;0;0;0m\x1b[38;2;9;9;9m\x1b[1mx\x1b[22m\x1b[0m"
        );
    }

    #[test]
    fn a_continuation_cell_never_breaks_a_run() {
        let black = [0, 0, 0];
        let wide = [
            cell('漢', [1, 1, 1], black),
            Cell::new(None, [1, 1, 1], black),
        ];
        assert_eq!(
            row_bytes(&wide, black, None),
            "\x1b[48;2;0;0;0m\x1b[38;2;1;1;1m漢\x1b[0m"
        );
    }
}

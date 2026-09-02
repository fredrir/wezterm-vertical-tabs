//! Cell primitives shared by the sidebar and settings renderers.

use crate::{color::Color, text};

/// One column. `ch == None` is Lua's `""` continuation cell: it owns no text and never breaks a run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: Option<char>,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub scrim: f64,
}

impl Cell {
    pub fn new(ch: Option<char>, fg: Color, bg: Color) -> Self {
        Cell {
            ch,
            fg,
            bg,
            bold: false,
            scrim: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub fg: Color,
    pub bg: Option<Color>,
    pub bold: bool,
}

impl Style {
    pub fn fg(fg: Color) -> Self {
        Style {
            fg,
            bg: None,
            bold: false,
        }
    }
}

/// One cell per column; a wide glyph owns two cells, so a row is always exactly `cols` wide.
pub fn new_line(cols: i64, bg: Color, fg: Color) -> Vec<Cell> {
    vec![Cell::new(Some(' '), fg, bg); cols.max(0) as usize]
}

pub fn fill(cells: &mut [Cell], x1: i64, x2: i64, bg: Color) {
    let last = x2.min(cells.len() as i64);
    let mut x = x1.max(1);
    while x <= last {
        cells[(x - 1) as usize].bg = bg;
        x += 1;
    }
}

/// Writes `text` from column `x`, never past `limit`.
pub fn put(cells: &mut [Cell], x: i64, s: &str, st: &Style, limit: i64) {
    let n = cells.len() as i64;
    let limit = limit.min(n);
    let mut col = x;
    for ch in s.chars() {
        let w = text::width(ch.encode_utf8(&mut [0u8; 4])) as i64;
        if w == 0 {
            continue;
        }
        if col < 1 || col + w - 1 > limit {
            break;
        }
        let head = (col - 1) as usize;
        let bg = st.bg.unwrap_or(cells[head].bg);
        cells[head] = Cell {
            ch: Some(ch),
            fg: st.fg,
            bg,
            bold: st.bold,
            scrim: 0.0,
        };
        for k in 1..w {
            let idx = (col + k - 1) as usize;
            let bg = st.bg.unwrap_or(cells[idx].bg);
            cells[idx] = Cell::new(None, st.fg, bg);
        }
        col += w;
    }
}

pub fn faded(rgb: Color, page_bg: Color, fade: f64) -> Color {
    if fade <= 0.0 {
        return rgb;
    }
    crate::theme::mix(rgb, page_bg, fade)
}

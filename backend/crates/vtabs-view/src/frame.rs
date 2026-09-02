//! Port of render.lua's cell primitives (`new_line`/`fill`/`put`) and its `emit()` run model,
//! plus the two dump serializations `plugin/tests/support/helpers.lua` writes the goldens from.

use crate::text;

pub type Rgb = [u8; 3];

/// One column. `ch == None` is Lua's `""` continuation cell: it owns no text and never breaks a run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: Option<char>,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub scrim: f64,
}

impl Cell {
    pub fn new(ch: Option<char>, fg: Rgb, bg: Rgb) -> Self {
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
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub bold: bool,
}

impl Style {
    pub fn fg(fg: Rgb) -> Self {
        Style {
            fg,
            bg: None,
            bold: false,
        }
    }
}

/// One cell per column; a wide glyph owns two cells, so a row is always exactly `cols` wide.
pub fn new_line(cols: i64, bg: Rgb, fg: Rgb) -> Vec<Cell> {
    vec![Cell::new(Some(' '), fg, bg); cols.max(0) as usize]
}

pub fn fill(cells: &mut [Cell], x1: i64, x2: i64, bg: Rgb) {
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

pub fn faded(rgb: Rgb, page_bg: Rgb, fade: f64) -> Rgb {
    if fade <= 0.0 {
        return rgb;
    }
    vtabs_theme::mix(rgb, page_bg, fade)
}

fn same(a: &Cell, b: &Cell) -> bool {
    a.scrim == b.scrim && a.fg == b.fg && a.bg == b.bg && a.bold == b.bold
}

fn hex(fg: Rgb, bg: Rgb, bold: bool) -> String {
    format!(
        "#{:02x}{:02x}{:02x}/#{:02x}{:02x}{:02x}{}",
        fg[0],
        fg[1],
        fg[2],
        bg[0],
        bg[1],
        bg[2],
        if bold { "*" } else { "" }
    )
}

/// One row's text and its `#fg/#bg[*]:width` span list, at the SGR granularity `emit()` produces:
/// an all-blank run omits the fg code, so its span inherits the fg the row last set.
pub fn emit(cells: &[Cell], page_bg: Rgb, fade: Option<f64>) -> (String, String) {
    let mut text = String::new();
    let mut spans: Vec<(String, i64)> = Vec::new();
    let (mut sgr_fg, mut sgr_bg) = ([0u8; 3], [0u8; 3]);
    let mut run: Option<Cell> = None;
    let mut body = String::new();

    let mut flush = |run: &Option<Cell>, body: &mut String, spans: &mut Vec<(String, i64)>| {
        if let Some(r) = run.filter(|_| !body.is_empty()) {
            sgr_bg = faded(r.bg, page_bg, r.scrim);
            if !body.chars().all(|c| c == ' ') {
                sgr_fg = faded(r.fg, page_bg, fade.unwrap_or(r.scrim));
            }
            let key = hex(sgr_fg, sgr_bg, r.bold);
            let w = text::width(body) as i64;
            if w > 0 {
                match spans.last_mut() {
                    Some(last) if last.0 == key => last.1 += w,
                    _ => spans.push((key, w)),
                }
            }
        }
        body.clear();
    };

    for cell in cells {
        let Some(ch) = cell.ch else { continue };
        if !matches!(run, Some(r) if same(&r, cell)) {
            flush(&run, &mut body, &mut spans);
            run = Some(*cell);
        }
        body.push(ch);
        text.push(ch);
    }
    flush(&run, &mut body, &mut spans);

    let joined = spans
        .iter()
        .map(|(key, w)| format!("{key}:{w}"))
        .collect::<Vec<_>>()
        .join(" ");
    (text, joined)
}

/// `helpers.dump_lines`: a 2-row column ruler, then one line per frame row.
pub fn dump_text(rows: &[String], cols: i64) -> String {
    let mut out = String::new();
    for x in 1..=cols {
        if x % 10 == 0 {
            out.push_str(&(x / 10).to_string());
        } else {
            out.push(' ');
        }
    }
    out.push('\n');
    for x in 1..=cols {
        out.push_str(&(x % 10).to_string());
    }
    out.push('\n');
    for line in rows {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `helpers.dump_styled`: `%2d: ` then the row's spans, space-separated.
pub fn dump_styled(rows: &[String]) -> String {
    let mut out = String::new();
    for (i, spans) in rows.iter().enumerate() {
        out.push_str(&format!("{:2}: {spans}\n", i + 1));
    }
    out
}

/// Both golden serializations of a laid-out frame, whichever widget laid it out.
pub fn dumps(
    rows: &[Option<Vec<Cell>>],
    fades: &[Option<f64>],
    page_bg: Rgb,
    cols: i64,
) -> (String, String) {
    let mut texts = Vec::with_capacity(rows.len());
    let mut styled = Vec::with_capacity(rows.len());
    for (row, cells) in rows.iter().enumerate() {
        let fade = fades.get(row).copied().flatten();
        let (text, spans) = emit(cells.as_deref().unwrap_or(&[]), page_bg, fade);
        texts.push(text);
        styled.push(spans);
    }
    (dump_text(&texts, cols), dump_styled(&styled))
}

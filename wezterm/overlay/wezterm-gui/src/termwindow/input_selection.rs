use super::TermWindow;
use crate::selection::{SelectionRange, SelectionX};
use mux::pane::Pane;
use std::convert::TryFrom;
use unicode_segmentation::UnicodeSegmentation;
use wezterm_term::{unicode_column_width, Line, StableRowIndex};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Point {
    y: StableRowIndex,
    x: usize,
}

struct Glyph<'a> {
    at: Point,
    text: &'a str,
    width: usize,
}

struct Layout<'a> {
    glyphs: Vec<Glyph<'a>>,
    boundaries: Vec<(usize, Point)>,
}

fn advance(at: &mut Point, width: usize, cols: usize) {
    at.x += width;
    if at.x == cols {
        at.x = 0;
        at.y += 1;
    }
}

fn layout(buffer: &str, start_x: usize, cols: usize) -> Option<Layout<'_>> {
    let mut at = Point { y: 0, x: start_x };
    let mut glyphs = Vec::new();
    let mut boundaries = vec![(0, at)];
    let mut offset = 0;
    for text in buffer.graphemes(true) {
        match text {
            "\n" => {
                at.y += 1;
                at.x = 0;
            }
            "\t" => {
                for _ in 0..(8 - at.x % 8) {
                    glyphs.push(Glyph {
                        at,
                        text: " ",
                        width: 1,
                    });
                    advance(&mut at, 1, cols);
                }
            }
            _ => {
                if text.chars().any(char::is_control) {
                    return None;
                }
                let width = unicode_column_width(text, None);
                if width == 0 || width > cols {
                    return None;
                }
                if at.x + width > cols {
                    at.x = 0;
                    at.y += 1;
                }
                glyphs.push(Glyph { at, text, width });
                advance(&mut at, width, cols);
            }
        }
        offset += text.chars().count();
        boundaries.push((offset, at));
    }
    Some(Layout { glyphs, boundaries })
}

fn resolve(
    buffer: &str,
    cursor: usize,
    screen_cursor: Point,
    start: Point,
    end: Point,
    cols: usize,
    mut matches: impl FnMut(Point, usize, &str) -> bool,
) -> Option<(usize, usize)> {
    if cols == 0 || buffer.is_empty() || buffer.len() > 128 * 1024 {
        return None;
    }
    let mut result = None;
    for start_x in 0..cols {
        let layout = layout(buffer, start_x, cols)?;
        let &(_, point) = layout.boundaries.iter().find(|(i, _)| *i == cursor)?;
        if point.x != screen_cursor.x {
            continue;
        }
        let top = screen_cursor.y - point.y;
        let relative = |p: Point| Point {
            y: p.y - top,
            x: p.x,
        };
        let start = relative(start);
        let mut end = relative(end);
        if let Some(glyph) = layout.glyphs.iter().find(|glyph| {
            glyph.at.y == end.y && glyph.at.x < end.x && end.x < glyph.at.x + glyph.width
        }) {
            end = glyph.at;
            advance(&mut end, glyph.width, cols);
        }
        let first = layout.boundaries.first()?.1;
        let last = layout.boundaries.last()?.1;
        if start < first || end > last || start >= end {
            continue;
        }
        let Some(&(start, _)) = layout.boundaries.iter().find(|(_, p)| *p == start) else {
            continue;
        };
        let Some(&(end, _)) = layout.boundaries.iter().find(|(_, p)| *p == end) else {
            continue;
        };
        if !layout.glyphs.iter().all(|glyph| {
            matches(
                Point {
                    y: top + glyph.at.y,
                    x: glyph.at.x,
                },
                glyph.width,
                glyph.text,
            )
        }) {
            continue;
        }
        if result.is_some_and(|previous| previous != (start, end)) {
            return None;
        }
        result = Some((start, end));
    }
    result
}

fn selection_points(range: SelectionRange, cols: usize) -> Option<(Point, Point)> {
    let (first, last) = if (range.start.y, range.start.x) <= (range.end.y, range.end.x) {
        (range.start, range.end)
    } else {
        (range.end, range.start)
    };
    let column = |x| match x {
        SelectionX::BeforeZero => Some(0),
        SelectionX::Cell(x) if x < cols => Some(x),
        _ => None,
    };
    let start = Point {
        y: first.y,
        x: column(first.x)?,
    };
    let mut end = Point {
        y: last.y,
        x: column(last.x)?,
    };
    if matches!(last.x, SelectionX::Cell(_)) {
        advance(&mut end, 1, cols);
    }
    Some((start, end))
}

pub(crate) struct InputSelection {
    start: Point,
    end: Point,
    cursor: Point,
    cols: usize,
    top: StableRowIndex,
    lines: Vec<Line>,
}

impl mlua::UserData for InputSelection {
    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method(
            "get_range",
            |lua, this, (buffer, cursor): (String, usize)| {
                let range = resolve(
                    &buffer,
                    cursor,
                    this.cursor,
                    this.start,
                    this.end,
                    this.cols,
                    |at, width, text| {
                        at.y.checked_sub(this.top)
                            .and_then(|row| usize::try_from(row).ok())
                            .and_then(|row| this.lines.get(row))
                            .is_some_and(|line| line.columns_as_str(at.x..at.x + width) == text)
                    },
                );
                match range {
                    Some((start, end)) => {
                        let result = lua.create_table()?;
                        result.set("start", start)?;
                        result.set("end", end)?;
                        Ok(Some(result))
                    }
                    None => Ok(None),
                }
            },
        );
    }
}

impl TermWindow {
    pub(crate) fn input_selection(&mut self, pane: &dyn Pane) -> Option<InputSelection> {
        if pane.is_alt_screen_active() {
            return None;
        }
        let selection = *self.selection(pane.pane_id());
        if selection.rectangular {
            return None;
        }
        let dims = pane.get_dimensions();
        let cols = dims.cols;
        let (start, end) = selection_points(selection.range?, cols)?;
        let cursor = pane.get_cursor_position();
        let (top, lines) = pane
            .get_lines(dims.physical_top..dims.physical_top + dims.viewport_rows as StableRowIndex);
        Some(InputSelection {
            start,
            end,
            cursor: Point {
                y: cursor.y,
                x: cursor.x,
            },
            cols,
            top,
            lines,
        })
    }
}

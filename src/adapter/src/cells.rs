//! Ratatui cells become Termwiz lines without terminal output or full-row reconversion.
use crate::termwindow::ui_host::{Geometry, Surface};
use ratatui::{
    buffer::{Buffer, Cell as UiCell, CellWidth},
    style::{Color, Modifier},
};
use std::rc::Rc;
use termwiz::{
    cell::{Blink, CellAttributes, Intensity, Underline},
    color::{ColorAttribute, RgbColor},
    surface::Line,
};
use vtabs_app::ui::FrameUpdate;

/// `FrameUpdate::changed_cells` is the UI's row-major Ratatui diff. Unchanged rows retain
/// their Rc identity and cached quads. A retained previous frame forces copy-on-write
/// only for rows that change; the GUI publishes the updated surface after this returns.
pub fn update(surface: &mut Surface, buffer: &Buffer, frame: &FrameUpdate, geometry: Geometry) {
    let columns = usize::from(buffer.area.width);
    let rows = usize::from(buffer.area.height);
    let sequence = frame.revision as usize;
    let rebuild = frame.resized || surface.columns != columns || surface.rows.len() != rows;
    if rebuild {
        surface.rows.clear();
        surface.rows.reserve(rows);
        for y in buffer.area.y..buffer.area.bottom() {
            let mut line = Line::with_width(columns, sequence);
            let mut x = buffer.area.x;
            while x < buffer.area.right() {
                let cell = &buffer[(x, y)];
                let width = cell_width(cell, buffer.area.right() - x);
                line.set_cell_grapheme(
                    usize::from(x - buffer.area.x),
                    cell.symbol(),
                    width,
                    attributes(cell),
                    sequence,
                );
                x += width as u16;
            }
            surface.rows.push(Rc::new(line));
        }
    } else {
        let mut changes = frame.changed_cells.as_slice();
        while let Some(&(_, y)) = changes.first() {
            let count = changes.iter().take_while(|(_, row)| *row == y).count();
            let (row_changes, remaining) = changes.split_at(count);
            changes = remaining;
            if y < buffer.area.y || y >= buffer.area.bottom() {
                continue;
            }
            let row = &mut surface.rows[usize::from(y - buffer.area.y)];
            let line = Rc::make_mut(row);
            let mut covered_until = buffer.area.x;
            for &(x, _) in row_changes {
                if x < covered_until || x >= buffer.area.right() {
                    continue;
                }
                let relative_x = usize::from(x - buffer.area.x);
                let cell = &buffer[(x, y)];
                let width = cell_width(cell, buffer.area.right() - x);
                let old_width = assign_cell(line, relative_x, cell, width, sequence);
                covered_until = x + width as u16;
                let mut repair_until = relative_x.saturating_add(old_width).min(columns);
                // A narrow replacement must restore every formerly covered cell, including
                // attributes Ratatui may omit because they are invisible on a blank. Repair
                // can encounter a shifted wide glyph; extend through its former coverage.
                while usize::from(covered_until - buffer.area.x) < repair_until {
                    let next_x = usize::from(covered_until - buffer.area.x);
                    let next = &buffer[(covered_until, y)];
                    let next_width = cell_width(next, buffer.area.right() - covered_until);
                    let previous_width = assign_cell(line, next_x, next, next_width, sequence);
                    repair_until =
                        repair_until.max(next_x.saturating_add(previous_width).min(columns));
                    covered_until += next_width as u16;
                }
                // Ratatui may explicitly clear a VS16 emoji's trailing cell. lines
                // already clear that coverage; writing it again would erase the wide lead.
            }
        }
    }
    surface.columns = columns;
    surface.revision = frame.revision;
    surface.offset = (frame.transform.translate_x * geometry.sidebar.width, 0.0);
    surface.opacity = frame.transform.opacity;
}

fn assign_cell(
    line: &mut Line,
    index: usize,
    cell: &UiCell,
    width: usize,
    sequence: usize,
) -> usize {
    // Indexed reads avoid Line::get_cell's linear visible-cell search. Termwiz vector
    // storage is materialized once, and is retained between changes.
    let previous = &line.cells_mut()[index];
    let previous_width = previous.width().max(1);
    let same_glyph = previous.str() == cell.symbol() && previous_width == width;
    let attributes = attributes(cell);
    if same_glyph {
        // TachyonFX color transitions reuse grapheme storage. Sequence changes
        // still invalidate upstream's line shape key, which includes foreground/background.
        // Styles on the covered cells must agree with their lead's attributes.
        let cells = line.cells_mut_for_attr_changes_only();
        for cell in &mut cells[index..index + width] {
            *cell.attrs_mut() = attributes.clone();
        }
        line.update_last_change_seqno(sequence);
    } else {
        line.set_cell_grapheme(index, cell.symbol(), width, attributes, sequence);
    }
    previous_width
}

fn cell_width(cell: &UiCell, remaining: u16) -> usize {
    // Use Ratatui's policy, including forced widths and halfwidth kana marks. Recomputing
    // width through Termwiz (or bare unicode-width) can disagree with the composed grid.
    usize::from(cell.cell_width().max(1).min(remaining))
}

fn attributes(cell: &UiCell) -> CellAttributes {
    let mut attributes = CellAttributes::default();
    attributes
        .set_foreground(color(cell.fg))
        .set_background(color(cell.bg));
    attributes.set_intensity(if cell.modifier.contains(Modifier::BOLD) {
        Intensity::Bold
    } else if cell.modifier.contains(Modifier::DIM) {
        Intensity::Half
    } else {
        Intensity::Normal
    });
    attributes.set_italic(cell.modifier.contains(Modifier::ITALIC));
    attributes.set_underline(if cell.modifier.contains(Modifier::UNDERLINED) {
        Underline::Single
    } else {
        Underline::None
    });
    attributes.set_reverse(cell.modifier.contains(Modifier::REVERSED));
    attributes.set_strikethrough(cell.modifier.contains(Modifier::CROSSED_OUT));
    attributes.set_invisible(cell.modifier.contains(Modifier::HIDDEN));
    attributes.set_blink(if cell.modifier.contains(Modifier::RAPID_BLINK) {
        Blink::Rapid
    } else if cell.modifier.contains(Modifier::SLOW_BLINK) {
        Blink::Slow
    } else {
        Blink::None
    });
    attributes
}

fn color(color: Color) -> ColorAttribute {
    match color {
        Color::Reset => ColorAttribute::Default,
        Color::Rgb(r, g, b) => {
            ColorAttribute::TrueColorWithDefaultFallback(RgbColor::new_8bpc(r, g, b).into())
        }
        Color::Indexed(index) => ColorAttribute::PaletteIndex(index),
        named => ColorAttribute::PaletteIndex(match named {
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::Gray => 7,
            Color::DarkGray => 8,
            Color::LightRed => 9,
            Color::LightGreen => 10,
            Color::LightYellow => 11,
            Color::LightBlue => 12,
            Color::LightMagenta => 13,
            Color::LightCyan => 14,
            Color::White => 15,
            _ => unreachable!("RGB, indexed, and default colors handled above"),
        }),
    }
}

#[cfg(test)]
#[path = "../tests/cells.rs"]
mod tests;

//! Ratatui cells become native lines without terminal output or full-row reconversion.
use crate::termwindow::native_ui::{Geometry, Surface};
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
/// their Rc identity and native cached quads. A retained previous frame forces copy-on-write
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
                // Ratatui may explicitly clear a VS16 emoji's trailing cell. Native lines
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
    // Indexed reads avoid Line::get_cell's linear visible-cell search. Native vector
    // storage is materialized once, and is retained between changes.
    let previous = &line.cells_mut()[index];
    let previous_width = previous.width().max(1);
    let same_glyph = previous.str() == cell.symbol() && previous_width == width;
    let attributes = attributes(cell);
    if same_glyph {
        // TachyonFX color transitions reuse native grapheme storage. Sequence changes
        // still invalidate upstream's line shape key, which includes foreground/background.
        // Styles on the covered cells must agree with their lead's native attributes.
        let cells = line.cells_mut_for_attr_changes_only();
        for native in &mut cells[index..index + width] {
            *native.attrs_mut() = attributes.clone();
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
mod tests {
    use super::*;
    use ratatui::{
        buffer::CellDiffOption,
        layout::Rect,
        style::Style,
        widgets::{Block, Clear, Paragraph, Widget},
    };
    use std::num::NonZeroU16;
    use vtabs_app::ui::SurfaceTransform;

    fn surface() -> Surface {
        Surface {
            rows: Vec::new(),
            columns: 0,
            revision: 0,
            offset: (0., 0.),
            opacity: 1.,
        }
    }
    fn frame(previous: Option<&Buffer>, next: &Buffer, revision: u64) -> FrameUpdate {
        let resized = previous.is_none_or(|previous| previous.area != next.area);
        let changed_cells = if resized {
            (next.area.y..next.area.bottom())
                .flat_map(|y| (next.area.x..next.area.right()).map(move |x| (x, y)))
                .collect::<Vec<_>>()
        } else {
            previous
                .unwrap()
                .diff_iter(next)
                .map(|(x, y, _)| (x, y))
                .collect::<Vec<_>>()
        };
        let mut dirty_rows = Vec::new();
        for &(_, y) in &changed_cells {
            if dirty_rows.last() != Some(&y) {
                dirty_rows.push(y);
            }
        }
        FrameUpdate {
            revision,
            resized,
            changed_cells,
            dirty_rows,
            cursor: None,
            ime_rect: None,
            transform: SurfaceTransform {
                translate_x: 0.,
                opacity: 1.,
            },
        }
    }
    fn assert_surface(surface: &Surface, buffer: &Buffer) {
        assert_eq!(surface.rows.len(), usize::from(buffer.area.height));
        assert_eq!(surface.columns, usize::from(buffer.area.width));
        for (row, line) in surface.rows.iter().enumerate() {
            assert_eq!(line.len(), surface.columns);
            let mut next_column = 0;
            for native in line.visible_cells() {
                assert_eq!(
                    native.cell_index(),
                    next_column,
                    "missing native lead in row {row}"
                );
                let x = buffer.area.x + native.cell_index() as u16;
                let wanted = &buffer[(x, buffer.area.y + row as u16)];
                assert_eq!(native.str(), wanted.symbol(), "grapheme at {x},{row}");
                assert_eq!(
                    native.width(),
                    cell_width(wanted, buffer.area.right() - x),
                    "width at {x},{row}"
                );
                assert_eq!(
                    native.attrs().foreground(),
                    color(wanted.fg),
                    "foreground at {x},{row}"
                );
                assert_eq!(
                    native.attrs().background(),
                    color(wanted.bg),
                    "background at {x},{row}"
                );
                next_column += native.width();
            }
            assert_eq!(next_column, surface.columns);
        }
    }
    fn painted(width: u16, height: u16, text: &str, style: Style) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        Block::default()
            .style(style)
            .render(buffer.area, &mut buffer);
        Paragraph::new(text)
            .style(style)
            .render(buffer.area, &mut buffer);
        buffer
    }

    #[test]
    fn preserves_graphemes_width_and_background_for_cjk_emoji_combining_rtl() {
        let buffer = painted(
            40,
            4,
            "界面 e\u{301} 👨‍👩‍👧‍👦 ☎\u{fe0f}\nملاحظات العربية\nｶﾞ ﾊﾟ",
            Style::default()
                .fg(Color::Rgb(13, 127, 240))
                .bg(Color::Indexed(234)),
        );
        let mut surface = surface();
        update(
            &mut surface,
            &buffer,
            &frame(None, &buffer, 1),
            Geometry::default(),
        );
        assert_surface(&surface, &buffer);
        for line in &surface.rows {
            let _ = line.cluster(None);
        }
    }

    #[test]
    fn changed_rows_copy_on_write_and_unchanged_rows_retain_native_cache_identity() {
        let mut buffer = painted(20, 3, "first\nsecond\nthird", Style::default());
        let mut surface = surface();
        update(
            &mut surface,
            &buffer,
            &frame(None, &buffer, 1),
            Geometry::default(),
        );
        let published = surface.rows.clone();
        let before = buffer.clone();
        buffer[(2, 1)].set_symbol("界");
        buffer[(3, 1)].reset();
        update(
            &mut surface,
            &buffer,
            &frame(Some(&before), &buffer, 2),
            Geometry::default(),
        );
        assert!(Rc::ptr_eq(&published[0], &surface.rows[0]));
        assert!(!Rc::ptr_eq(&published[1], &surface.rows[1]));
        assert!(Rc::ptr_eq(&published[2], &surface.rows[2]));
        assert_eq!(published[1].get_cell(2).unwrap().str(), "c");
        assert_surface(&surface, &buffer);
        let unique = Rc::as_ptr(&surface.rows[1]);
        let before = buffer.clone();
        buffer[(0, 1)].set_fg(Color::Red);
        update(
            &mut surface,
            &buffer,
            &frame(Some(&before), &buffer, 3),
            Geometry::default(),
        );
        assert_eq!(unique, Rc::as_ptr(&surface.rows[1]));
        assert_surface(&surface, &buffer);
    }

    #[test]
    fn vs16_diff_trailing_clear_does_not_erase_the_new_emoji() {
        let before = painted(8, 1, "ABCD", Style::default().bg(Color::Blue));
        let after = painted(8, 1, "☎\u{fe0f}CD", Style::default().bg(Color::Green));
        let mut surface = surface();
        update(
            &mut surface,
            &before,
            &frame(None, &before, 1),
            Geometry::default(),
        );
        let changed = frame(Some(&before), &after, 2);
        assert!(
            changed.changed_cells.contains(&(1, 0)),
            "fixture must exercise VS16 trailing clear"
        );
        update(&mut surface, &after, &changed, Geometry::default());
        assert_surface(&surface, &after);
        assert_eq!(surface.rows[0].get_cell(0).unwrap().str(), "☎\u{fe0f}");
    }

    #[test]
    fn wide_to_narrow_and_shifted_wide_restore_all_covered_native_cells() {
        let mut surface = surface();
        let mut previous = None;
        for (revision, text, bg) in [
            (1, "界界界", Color::Red),
            (2, "a界b", Color::Reset),
            (3, "abcdef", Color::Blue),
            (4, "🚀e\u{301}", Color::Green),
            (5, "ab", Color::Reset),
        ] {
            let next = painted(12, 2, text, Style::default().fg(Color::White).bg(bg));
            update(
                &mut surface,
                &next,
                &frame(previous.as_ref(), &next, revision),
                Geometry::default(),
            );
            assert_surface(&surface, &next);
            previous = Some(next);
        }
    }

    #[test]
    fn overlay_replacement_resize_and_empty_surface_never_leave_stale_cells() {
        let mut previous = painted(
            32,
            10,
            "Sidebar 界\nTab 🚀",
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Rgb(20, 30, 40)),
        );
        let mut surface = surface();
        update(
            &mut surface,
            &previous,
            &frame(None, &previous, 1),
            Geometry::default(),
        );
        let mut overlay = previous.clone();
        let area = Rect::new(1, 1, 25, 7);
        Clear.render(area, &mut overlay);
        Paragraph::new("Menu\nRename\nClose")
            .style(Style::default().fg(Color::White).bg(Color::Red))
            .render(area, &mut overlay);
        update(
            &mut surface,
            &overlay,
            &frame(Some(&previous), &overlay, 2),
            Geometry::default(),
        );
        assert_surface(&surface, &overlay);
        update(
            &mut surface,
            &previous,
            &frame(Some(&overlay), &previous, 3),
            Geometry::default(),
        );
        assert_surface(&surface, &previous);
        for (width, height) in [(1, 1), (70, 120), (0, 0), (5, 3)] {
            let next = painted(
                width,
                height,
                "界e\u{301}🚀",
                Style::default().bg(Color::Green),
            );
            update(
                &mut surface,
                &next,
                &frame(Some(&previous), &next, 4),
                Geometry::default(),
            );
            assert_surface(&surface, &next);
            previous = next;
        }
    }

    #[test]
    fn explicit_forced_width_and_nonzero_buffer_origin_match_composed_cells() {
        let mut buffer = Buffer::empty(Rect::new(3, 7, 4, 2));
        buffer[(3, 7)]
            .set_symbol("界")
            .set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(1).unwrap()));
        buffer[(4, 7)].set_symbol("x");
        buffer[(6, 7)].set_symbol("界");
        let mut surface = surface();
        update(
            &mut surface,
            &buffer,
            &frame(None, &buffer, 1),
            Geometry::default(),
        );
        assert_surface(&surface, &buffer);
        assert_eq!(surface.rows[0].get_cell(0).unwrap().width(), 1);
        assert_eq!(surface.rows[0].get_cell(3).unwrap().width(), 1);
    }

    #[test]
    fn converts_all_native_text_modifiers_and_resets_removed_attributes() {
        let modifier = Modifier::BOLD
            | Modifier::ITALIC
            | Modifier::UNDERLINED
            | Modifier::REVERSED
            | Modifier::CROSSED_OUT
            | Modifier::HIDDEN
            | Modifier::RAPID_BLINK;
        let mut buffer = painted(8, 1, "x", Style::default().add_modifier(modifier));
        let mut surface = surface();
        update(
            &mut surface,
            &buffer,
            &frame(None, &buffer, 1),
            Geometry::default(),
        );
        let line = surface.rows[0].get_cell(0).unwrap();
        let attrs = line.attrs();
        assert_eq!(attrs.intensity(), Intensity::Bold);
        assert!(attrs.italic());
        assert_eq!(attrs.underline(), Underline::Single);
        assert!(attrs.reverse());
        assert!(attrs.strikethrough());
        assert!(attrs.invisible());
        assert_eq!(attrs.blink(), Blink::Rapid);
        let before = buffer.clone();
        buffer[(0, 0)].set_style(Style::reset().add_modifier(Modifier::DIM | Modifier::SLOW_BLINK));
        update(
            &mut surface,
            &buffer,
            &frame(Some(&before), &buffer, 2),
            Geometry::default(),
        );
        let line = surface.rows[0].get_cell(0).unwrap();
        let attrs = line.attrs();
        assert_eq!(attrs.intensity(), Intensity::Half);
        assert!(!attrs.italic());
        assert_eq!(attrs.underline(), Underline::None);
        assert!(!attrs.reverse());
        assert!(!attrs.strikethrough());
        assert!(!attrs.invisible());
        assert_eq!(attrs.blink(), Blink::Slow);
    }

    #[test]
    fn incremental_unicode_frames_match_complete_composition() {
        let tokens = ["a", "界", "e\u{301}", "☎\u{fe0f}", "👨‍👩‍👧‍👦", "م", " ", "ｶﾞ"];
        let mut state = 19u64;
        let mut surface = surface();
        let mut previous = None;
        for revision in 1..=500u64 {
            let mut text = String::new();
            for row in 0..4 {
                for _ in 0..20 {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    text.push_str(tokens[((state >> 32) as usize) % tokens.len()]);
                }
                if row < 3 {
                    text.push('\n');
                }
            }
            let next = painted(
                31,
                4,
                &text,
                Style::default()
                    .fg(Color::Indexed((revision / 7) as u8))
                    .bg(Color::Rgb(20, 30, 40)),
            );
            update(
                &mut surface,
                &next,
                &frame(previous.as_ref(), &next, revision),
                Geometry::default(),
            );
            assert_surface(&surface, &next);
            previous = Some(next);
        }
    }

    #[test]
    fn colors_reuse_grapheme_storage_and_invalidate_color_dependent_native_shape_key() {
        let before = painted(12, 2, "👨‍👩‍👧‍👦 Shape", Style::default().fg(Color::White));
        let mut surface = surface();
        update(
            &mut surface,
            &before,
            &frame(None, &before, 1),
            Geometry::default(),
        );
        let shape = surface.rows[0].compute_shape_hash();
        let glyph_storage = surface.rows[0].get_cell(0).unwrap().str().as_ptr();
        let mut after = before.clone();
        after[(0, 0)]
            .set_fg(Color::Rgb(1, 2, 3))
            .set_bg(Color::Indexed(19));
        update(
            &mut surface,
            &after,
            &frame(Some(&before), &after, 2),
            Geometry::default(),
        );
        assert_ne!(shape, surface.rows[0].compute_shape_hash());
        assert_eq!(
            glyph_storage,
            surface.rows[0].get_cell(0).unwrap().str().as_ptr()
        );
        assert_eq!(surface.rows[0].current_seqno(), 2);
        assert_surface(&surface, &after);
        let rows = surface.rows.clone();
        let mut next = frame(Some(&after), &after, 3);
        next.transform = SurfaceTransform {
            translate_x: 0.5,
            opacity: 0.25,
        };
        let mut geometry = Geometry::default();
        geometry.sidebar.width = 240.;
        update(&mut surface, &after, &next, geometry);
        assert!(rows
            .iter()
            .zip(&surface.rows)
            .all(|(a, b)| Rc::ptr_eq(a, b)));
        assert_eq!(surface.offset, (120., 0.));
        assert_eq!(surface.opacity, 0.25);
        for (index, named) in [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
        ]
        .iter()
        .copied()
        .enumerate()
        {
            assert_eq!(color(named), ColorAttribute::PaletteIndex(index as u8));
        }
    }
}

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
        for cell in line.visible_cells() {
            assert_eq!(
                cell.cell_index(),
                next_column,
                "missing cell lead in row {row}"
            );
            let x = buffer.area.x + cell.cell_index() as u16;
            let wanted = &buffer[(x, buffer.area.y + row as u16)];
            assert_eq!(cell.str(), wanted.symbol(), "grapheme at {x},{row}");
            assert_eq!(
                cell.width(),
                cell_width(wanted, buffer.area.right() - x),
                "width at {x},{row}"
            );
            assert_eq!(
                cell.attrs().foreground(),
                color(wanted.fg),
                "foreground at {x},{row}"
            );
            assert_eq!(
                cell.attrs().background(),
                color(wanted.bg),
                "background at {x},{row}"
            );
            next_column += cell.width();
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
fn changed_rows_copy_on_write_and_unchanged_rows_retain_cache_identity() {
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
fn wide_to_narrow_and_shifted_wide_restore_all_covered_cells() {
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
fn converts_all_text_modifiers_and_resets_removed_attributes() {
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
fn colors_reuse_grapheme_storage_and_invalidate_color_dependent_shape_key() {
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
    assert!(
        rows.iter()
            .zip(&surface.rows)
            .all(|(a, b)| Rc::ptr_eq(a, b))
    );
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

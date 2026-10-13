use crate::components::{EDGE_INSET, SURFACE_RADIUS, anchored, panel};
use crate::element::ElementId;
use crate::icons;
use crate::input::display_text;
use crate::overlays::Menu;
use crate::overlays::launcher::LauncherKind;
use crate::runtime::canvas::{Canvas, RoundedSurface};
use ratatui::layout::{Position, Rect};
use ratatui::style::Color;
use tachyonfx::CenteredShrink;
use unicode_width::UnicodeWidthStr;

pub(crate) fn menu(cx: &mut Canvas, area: Rect, anchor: Option<Position>, menu: &mut Menu) -> Rect {
    let compact = matches!(
        menu.search.as_ref().map(|search| search.kind),
        Some(LauncherKind::Commands)
    );
    let height = menu
        .items
        .len()
        .max(1)
        .saturating_add(2)
        .min(usize::from(u16::MAX)) as u16;
    let width = width(menu);
    let rect = match anchor {
        Some(anchor) => anchored(area, Rect::new(anchor.x, anchor.y, 1, 1), width, height),
        None => area.inner_centered(width, height),
    };
    let panel = panel(cx, rect, area);
    let (rect, inner) = (panel.rect, panel.inner);
    let theme = cx.theme;
    if panel.framed {
        cx.write(
            Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1),
            display_text(&menu.title),
            theme.accent(),
        );
    }
    let rows = usize::from(inner.height);
    menu.follow_selection(rows);
    for (offset, item) in menu.items.iter().skip(menu.scroll).take(rows).enumerate() {
        let row = Rect::new(inner.x, inner.y + offset as u16, inner.width, 1);
        let selected = menu.scroll + offset == menu.selected;
        cx.rounded(
            row,
            if selected {
                theme.selected
            } else {
                theme.background
            },
        );
        let style = if !item.enabled {
            theme.muted()
        } else if selected {
            theme.accent().bg(theme.selected)
        } else {
            theme.base()
        };
        let hint_width = item.hint.width().min(usize::from(row.width)) as u16;
        let label = display_text(&item.label);
        let label = if compact {
            label
        } else {
            format!("{} {}", if selected { "›" } else { " " }, label)
        };

        cx.write(
            Rect::new(row.x, row.y, row.width.saturating_sub(hint_width), 1),
            label,
            style,
        );
        if hint_width > 0 {
            cx.write(
                Rect::new(row.right() - hint_width, row.y, hint_width, 1),
                item.hint.clone(),
                style.fg(theme.muted),
            );
        }
        cx.hit(ElementId::Menu(item.id.clone()), row, item.label.clone());
    }
    rect
}

fn width(menu: &Menu) -> u16 {
    let compact = matches!(
        menu.search.as_ref().map(|search| search.kind),
        Some(LauncherKind::Commands)
    );

    let leading = if compact { 0 } else { 2 };

    let rows = menu
        .items
        .iter()
        .map(|item| display_text(&item.label).width() + item.hint.width() + leading + 3);

    rows.chain(std::iter::once(display_text(&menu.title).width() + 2))
        .max()
        .unwrap_or(0)
        .clamp(20, 56) as u16
}

/// Opens beneath its control, right-aligned to it, or above when the page ends first.
pub(crate) fn dropdown(cx: &mut Canvas, area: Rect, menu: &mut Menu) -> Rect {
    let theme = cx.theme;
    let anchor = menu.dropdown.unwrap_or_default();
    let widest = menu
        .items
        .iter()
        .map(|item| display_text(&item.label).width() as u16)
        .max()
        .unwrap_or(0);
    let width = (widest + 7).max(anchor.width).min(area.width);
    let height = (menu.items.len() as u16).clamp(1, area.height.max(1));
    let right_aligned = Rect {
        x: anchor.right().saturating_sub(width),
        ..anchor
    };
    let rect = anchored(area, right_aligned, width, height);
    cx.clear(rect);
    cx.mark(RoundedSurface {
        stacked: true,
        ..RoundedSurface::new(rect, Color::Reset, 0.0, 0.0)
    });
    // The panel reaches a third of a row past its options instead of a whole padding row.
    let grow = (f32::from(height) + 0.6) / f32::from(height);
    let fill = theme.card;
    for (color, radius, inset) in [
        (theme.lift(fill, 14), SURFACE_RADIUS, EDGE_INSET),
        (fill, SURFACE_RADIUS - 1.0, EDGE_INSET + 1.0),
    ] {
        cx.mark(RoundedSurface {
            scale_y: grow,
            ..RoundedSurface::new(rect, color, radius, inset)
        });
    }
    menu.follow_selection(usize::from(height));
    for (offset, item) in menu
        .items
        .iter()
        .enumerate()
        .skip(menu.scroll)
        .take(usize::from(height))
    {
        let row = Rect::new(
            rect.x + 1,
            rect.y + (offset - menu.scroll) as u16,
            rect.width.saturating_sub(2),
            1,
        );
        let id = ElementId::Menu(item.id.clone());
        let row_fill = if offset == menu.selected {
            theme.selected
        } else if cx.hovered(&id) {
            theme.lift(fill, 6)
        } else {
            fill
        };
        cx.fill(RoundedSurface::new(row, row_fill, 6.0, EDGE_INSET));
        let checked = menu.checked == Some(offset);
        let style = theme.base().bg(row_fill);
        cx.write(
            Rect::new(row.x + 1, row.y, 2, 1),
            if checked { icons::CHECK } else { "" },
            style.fg(theme.accent),
        );
        cx.write(
            Rect::new(row.x + 4, row.y, row.width.saturating_sub(5), 1),
            display_text(&item.label),
            style.fg(if checked {
                theme.accent
            } else {
                theme.foreground
            }),
        );
        cx.hit(id, row, "");
    }
    rect
}

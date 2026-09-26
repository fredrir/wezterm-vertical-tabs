use crate::components::{anchored, centered, list, panel};
use crate::element::ElementId;
use crate::input::display_text;
use crate::overlays::Menu;
use crate::runtime::canvas::Canvas;
use ratatui::layout::{Position, Rect};
use unicode_width::UnicodeWidthStr;

pub(crate) fn menu(cx: &mut Canvas, area: Rect, anchor: Option<Position>, menu: &mut Menu) -> Rect {
    let height = menu
        .items
        .len()
        .max(1)
        .saturating_add(2)
        .min(usize::from(u16::MAX)) as u16;
    let width = width(menu);
    let rect = match anchor {
        Some(anchor) => anchored(area, anchor, width, height),
        None => centered(area, width, height),
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
    menu.scroll = list::scroll_to(menu.scroll, menu.selected, menu.items.len(), rows);
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
        cx.write(
            Rect::new(row.x, row.y, row.width.saturating_sub(hint_width), 1),
            format!(
                "{} {}",
                if selected { "›" } else { " " },
                display_text(&item.label)
            ),
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
    let rows = menu
        .items
        .iter()
        .map(|item| display_text(&item.label).width() + item.hint.width() + 5);
    rows.chain(std::iter::once(display_text(&menu.title).width() + 2))
        .max()
        .unwrap_or(0)
        .clamp(20, 56) as u16
}

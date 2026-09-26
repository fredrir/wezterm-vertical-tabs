//! A centered search field over a filtered list of sidebar rows.
use crate::components::row::{Content, Row};
use crate::components::text_input::TextInput;
use crate::components::{ICON_CELLS, ROW_INSET, SURFACE_RADIUS, centered, list};
use crate::element::ElementId;
use crate::icons;
use crate::input::display_text;
use crate::overlays::Menu;
use crate::runtime::canvas::Canvas;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

/// Returns the palette's surface.
pub(crate) fn palette(cx: &mut Canvas, area: Rect, menu: &mut Menu) -> Rect {
    let theme = cx.theme;
    let Some(search) = &mut menu.search else {
        return Rect::default();
    };
    let tall = area.height >= 12 && area.width >= 12;
    let line = if tall { 2 } else { 1 };
    let pad = u16::from(tall);
    let gap = u16::from(tall);
    let wanted = search.all_items.len().max(1).min(usize::from(u16::MAX / 2)) as u16;
    let rect = centered(area, 64, pad * 2 + line + gap + wanted * line);
    cx.clear(rect);
    cx.rounded(rect, theme.background);
    let inner = Rect::new(
        rect.x + pad,
        rect.y + pad,
        rect.width.saturating_sub(pad * 2),
        rect.height.saturating_sub(pad * 2),
    );
    let field = Rect::new(inner.x, inner.y, inner.width, line.min(inner.height));
    cx.surface(field, theme.card, SURFACE_RADIUS, ROW_INSET);
    let lead = ICON_CELLS.min(field.width.saturating_sub(1));
    cx.write(
        Rect::new(field.x + 1, field.y, lead, 1),
        icons::SEARCH,
        theme.muted().bg(theme.card),
    );
    let edit = Rect::new(
        field.x + 1 + lead,
        field.y,
        field.width.saturating_sub(lead + 2),
        1,
    );
    // The host centers first-row text of a two-row surface; marks follow it.
    let shift = if field.height == 2 { 0.5 } else { 0.0 };
    TextInput::new(ElementId::Editor, &mut search.editor, theme.card)
        .active(true)
        .shift(shift)
        .placeholder(Some(&menu.title))
        .render(edit, cx);
    cx.hit(ElementId::Editor, field, "");
    let list = Rect::new(
        inner.x,
        field.bottom() + gap,
        inner.width,
        inner.bottom().saturating_sub(field.bottom() + gap),
    );
    let rows = usize::from(list.height / line).max(1);
    menu.scroll = list::scroll_to(menu.scroll, menu.selected, menu.items.len(), rows);
    if menu.items.is_empty() && list.height > 0 {
        cx.write(
            Rect::new(list.x + 1, list.y, list.width.saturating_sub(2), 1),
            if search.all_items.is_empty() {
                search.empty
            } else {
                "No matches"
            },
            theme.muted(),
        );
    }
    for (offset, item) in menu.items.iter().skip(menu.scroll).take(rows).enumerate() {
        let top = list.y + offset as u16 * line;
        let rect = Rect::new(
            list.x,
            top,
            list.width,
            line.min(list.bottom().saturating_sub(top)),
        );
        if rect.is_empty() {
            break;
        }
        let label = display_text(&item.label);
        let layout = Row {
            icon_color: item.icon_color,
            index: item.index,
            selected: menu.scroll + offset == menu.selected,
            muted: !item.enabled,
            ..Row::new(
                ElementId::Menu(item.id.clone()),
                rect,
                item.icon,
                Content::Label(&label),
            )
        }
        .render(cx);
        let hint = display_text(&item.hint);
        let width = (hint.width() as u16).min(layout.content.width / 2);
        cx.write(
            Rect::new(layout.content.right() - width, rect.y, width, 1),
            hint,
            theme.muted().bg(layout.fill),
        );
    }
    rect
}

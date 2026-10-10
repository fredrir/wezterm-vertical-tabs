use crate::components::button::{Button, Tone};
use crate::components::row::{Content, Row};
use crate::components::scrollbar::Scrollbar;
use crate::components::text_input::TextInput;
use crate::components::{ICON_CELLS, ROW_INSET, SURFACE_RADIUS, centered, list};
use crate::element::ElementId;
use crate::icons;
use crate::input::{TextEditor, display_text};
use crate::overlays::Menu;
use crate::runtime::canvas::Canvas;
use crate::views::launcher::LauncherKind;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

const MAX_VISIBLE_ITEMS: usize = 10;

pub(crate) fn palette(cx: &mut Canvas, area: Rect, menu: &mut Menu) -> Rect {
    let theme = cx.theme;
    let leading = menu.has_leading_column();
    let Some(search) = &mut menu.search else {
        return Rect::default();
    };
    let tall = area.height >= 12 && area.width >= 12;
    let line = if tall { 2 } else { 1 };
    let pad = u16::from(tall);
    let gap = u16::from(tall);
    let wanted = search.all_items.len().clamp(1, MAX_VISIBLE_ITEMS) as u16;
    let heading = if search.launch_menu.is_some() { 2 } else { 0 };
    let height =
        (pad * 2 + heading + line + gap + wanted * line).min(area.height.saturating_sub(2).max(1));
    let rect = centered(area, 64, height);
    cx.clear(rect);
    cx.rounded(rect, theme.background);
    let inner = Rect::new(
        rect.x + pad,
        rect.y + pad,
        rect.width.saturating_sub(pad * 2),
        rect.height.saturating_sub(pad * 2),
    );
    if let Some(launcher) = &search.launch_menu {
        cx.write(
            Rect::new(inner.x, inner.y, inner.width, inner.height.min(1)),
            display_text(&menu.title),
            theme.accent(),
        );
        if inner.height > 1 {
            let help = if launcher.filtering {
                &launcher.config.fuzzy_help_text
            } else {
                &launcher.config.help_text
            };
            cx.write(
                Rect::new(inner.x, inner.y + 1, inner.width, 1),
                display_text(help),
                theme.muted(),
            );
        }
    }
    let heading = heading.min(inner.height);
    let field = Rect::new(
        inner.x,
        inner.y + heading,
        inner.width,
        line.min(inner.height.saturating_sub(heading)),
    );
    cx.surface(field, theme.card, SURFACE_RADIUS, ROW_INSET);
    let lead = ICON_CELLS.min(field.width.saturating_sub(1));
    cx.write(
        Rect::new(field.x + 1, field.y, lead, 1),
        icons::SEARCH,
        theme.muted().bg(theme.card),
    );
    let button_width = if matches!(search.kind, LauncherKind::Commands | LauncherKind::Keybind) {
        3.min(field.width.saturating_sub(lead + 2))
    } else {
        0
    };
    let edit = Rect::new(
        field.x + 1 + lead,
        field.y,
        field.width.saturating_sub(lead + 2 + button_width),
        1,
    );
    let shift = if field.height == 2 { 0.5 } else { 0.0 };
    let mut recorded;
    let edits_query = search.edits_query();
    let editor = if search.kind == LauncherKind::Keybind {
        recorded = TextEditor::new(crate::keybinds::display_chord(search.editor.text()));
        &mut recorded
    } else if let Some(launcher) = &search.launch_menu
        && !launcher.filtering
    {
        recorded = TextEditor::new(&launcher.selection);
        &mut recorded
    } else {
        &mut search.editor
    };
    TextInput::new(ElementId::Editor, editor, theme.card)
        .active(edits_query)
        .shift(shift)
        .placeholder(if search.recording {
            "Recording keys. Press Escape to exit"
        } else {
            &menu.title
        })
        .render(edit, cx);
    cx.hit(ElementId::Editor, field, &menu.title);
    if button_width > 0 {
        Button::action(ElementId::RecordShortcut, icons::KEYBOARD, Tone::Neutral)
            .selected(search.recording)
            .label_right_padding(1)
            .tooltip(if search.recording {
                "Stop recording keys"
            } else {
                "Record keys"
            })
            .render(
                Rect::new(
                    field.right() - button_width - 1,
                    field.y,
                    button_width + 1,
                    field.height,
                ),
                cx,
            );
    }
    let mut list = Rect::new(
        inner.x,
        field.bottom() + gap,
        inner.width,
        inner.bottom().saturating_sub(field.bottom() + gap),
    );
    let rows = usize::from(list.height / line).max(1);
    menu.scroll = list::scroll_to(menu.scroll, menu.selected, menu.items.len(), rows);
    if let Some(launcher) = &mut search.launch_menu {
        launcher.labels = (launcher.config.labels)(
            &launcher.config.alphabet,
            rows.min(menu.items.len().saturating_sub(menu.scroll)),
        );
    }
    search.scrollbar = Scrollbar::new(
        Rect::new(
            list.right().saturating_sub(1),
            list.y,
            u16::from(list.width > 2),
            list.height / line * line,
        ),
        rows,
        menu.items.len(),
        menu.scroll,
    );
    if let Some(scrollbar) = search.scrollbar {
        list.width = list.width.saturating_sub(2);
        scrollbar.render(cx);
    }
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
            leading,
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
        let hint = display_text(
            search
                .launch_menu
                .as_ref()
                .filter(|launcher| !launcher.filtering)
                .and_then(|launcher| launcher.labels.get(offset))
                .unwrap_or(&item.hint),
        );
        let width = (hint.width() as u16).min(layout.content.width / 2);
        cx.write(
            Rect::new(layout.content.right() - width, rect.y, width, 1),
            hint,
            theme.muted().bg(layout.fill),
        );
    }
    rect
}

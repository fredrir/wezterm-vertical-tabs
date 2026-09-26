use crate::actions::Action;
use crate::components::button::{Button, Tone};
use crate::components::text_input::TextInput;
use crate::components::{ICON_CELLS, centered, panel};
use crate::element::ElementId;
use crate::icons;
use crate::input::display_text;
use crate::overlays::{Form, Menu};
use crate::runtime::canvas::Canvas;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

/// Padding, the question, its explanation, a gap, two-row buttons, padding.
const DIALOG_ROWS: u16 = 7;

/// Confirmations fall back to a plain menu when the window is too small for buttons.
pub(crate) fn fits(area: Rect) -> bool {
    area.height >= DIALOG_ROWS && area.width >= 24
}

/// A question, what it costs, and two buttons: the accepting one preselected and last.
pub(crate) fn confirm(cx: &mut Canvas, area: Rect, menu: &Menu) -> Rect {
    let theme = cx.theme;
    let message = menu.message.as_deref().unwrap_or_default();
    let explained = !message.is_empty();
    let rect = centered(area, 46, DIALOG_ROWS - u16::from(!explained));
    cx.clear(rect);
    cx.rounded(rect, theme.background);
    let inner = Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, rect.height - 2);
    cx.write(
        Rect::new(inner.x, inner.y, inner.width, 1),
        format!(
            "{:<width$}{}",
            icons::ALERT,
            display_text(&menu.title),
            width = usize::from(ICON_CELLS)
        ),
        theme.base(),
    );
    if explained {
        cx.write(
            Rect::new(
                inner.x + ICON_CELLS,
                inner.y + 1,
                inner.width - ICON_CELLS,
                1,
            ),
            display_text(message),
            theme.muted(),
        );
    }
    let mut right = inner.right();
    for (at, item) in menu.items.iter().enumerate().rev() {
        let label = display_text(&item.label);
        let width = (label.width() as u16 + 4).min(right - inner.x);
        let button = Rect::new(right - width, inner.bottom() - 2, width, 2);
        right = button.x.saturating_sub(1).max(inner.x);
        let tone = if matches!(item.action, Action::Close) {
            Tone::Neutral
        } else {
            Tone::Danger
        };
        Button::action(ElementId::Menu(item.id.clone()), &label, tone)
            .selected(at == menu.selected)
            .render(button, cx);
    }
    rect
}

pub(crate) fn form(cx: &mut Canvas, area: Rect, form: &mut Form, editing: bool) -> Rect {
    let theme = cx.theme;
    let panel = panel(cx, centered(area, 64, 7), area);
    let inner = panel.inner;
    if inner.height == 0 {
        return panel.rect;
    }
    cx.write(
        Rect::new(inner.x, inner.y, inner.width, 1),
        display_text(&form.title),
        theme.accent(),
    );
    let input_y = inner.y + u16::from(inner.height > 1);
    let edit = Rect::new(inner.x, input_y, inner.width, 1);
    cx.rounded(edit, theme.selected);
    TextInput::new(ElementId::Editor, &mut form.editor, theme.selected)
        .active(editing)
        .render(edit, cx);
    cx.hit(ElementId::Editor, edit, "Text entry");
    if inner.height > 2 {
        cx.write(
            Rect::new(inner.x, input_y + 1, inner.width, 1),
            form.error
                .clone()
                .unwrap_or_else(|| "Enter saves   Esc cancels".into()),
            if form.error.is_some() {
                theme.base().fg(theme.danger)
            } else {
                theme.muted()
            },
        );
    }
    if inner.height > 3 {
        let row = inner.bottom() - 1;
        let button_width = (inner.width / 2).clamp(1, 8);
        let save = Rect::new(inner.x, row, inner.width.min(button_width), 1);
        let gap = u16::from(inner.width > save.width + 1);
        let cancel = Rect::new(
            save.right() + gap,
            row,
            inner.width.saturating_sub(save.width + gap).min(8),
            1,
        );
        for (id, button, label) in [
            (ElementId::Submit, save, "Save"),
            (ElementId::Cancel, cancel, "Cancel"),
        ] {
            Button::text(id, label).tooltip(label).render(button, cx);
        }
    }
    panel.rect
}

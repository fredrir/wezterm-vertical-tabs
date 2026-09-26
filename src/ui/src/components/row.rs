//! The list row behind tabs, folders, the search field and launcher entries.
use super::{DROP_TINT, ICON_CELLS, ROW_INSET, SURFACE_RADIUS, TRAILING_CELLS};
use crate::{ElementId, icons, runtime::canvas::Canvas, shortcuts::platform_tooltip};
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::Line,
};
use unicode_width::UnicodeWidthStr;

/// A control that appears at the row's end while it is hovered or focused.
pub(crate) struct Trailing {
    pub id: ElementId,
    pub icon: &'static str,
    pub tooltip: &'static str,
}

pub(crate) enum Content<'a> {
    Label(&'a str),
    /// Painted by the caller into the row's content area, before the trailing control.
    Custom(&'a mut dyn FnMut(&mut Canvas, &RowLayout)),
}

pub(crate) struct Row<'a> {
    pub id: ElementId,
    pub rect: Rect,
    pub indent: u16,
    pub icon: &'a str,
    pub icon_color: Option<Color>,
    pub index: Option<usize>,
    pub content: Content<'a>,
    pub tooltip: Option<String>,
    pub selected: bool,
    pub muted: bool,
    pub compact: bool,
    pub trailing: Option<Trailing>,
    pub field: bool,
}

impl<'a> Row<'a> {
    pub fn new(id: ElementId, rect: Rect, icon: &'a str, content: Content<'a>) -> Self {
        Self {
            id,
            rect,
            indent: 0,
            icon,
            icon_color: None,
            index: None,
            content,
            tooltip: None,
            selected: false,
            muted: false,
            compact: false,
            trailing: None,
            field: false,
        }
    }
}

pub(crate) struct RowLayout {
    pub fill: Color,
    pub style: Style,
    pub content: Rect,
}

/// Odd-width glyphs shift one cell so they center on the host's pixel grid.
pub(crate) fn icon_rect(mut rect: Rect, label: &str) -> Rect {
    let width = label.width();
    if usize::from(rect.width) > width && usize::from(rect.width) % 2 != width % 2 {
        rect.x += 1;
        rect.width -= 1;
    }
    rect
}

impl Row<'_> {
    pub fn render(self, cx: &mut Canvas) -> RowLayout {
        let theme = cx.theme;
        let pointer = cx.pointer;
        let focused = cx.focused(&self.id);
        let pressed = cx.pressed(&self.id);
        let ghost = pointer.dragging
            && pointer
                .drag
                .as_ref()
                .is_some_and(|drag| !matches!(drag, ElementId::Pane(..)) && drag.row() == self.id);
        let aimed = cx.aimed(&self.id);
        let hovered = cx.row_hovered(&self.id) && !pointer.dragging;
        let fill = match (self.selected && !ghost, hovered || pressed, self.field) {
            _ if aimed => theme.tint(theme.card, DROP_TINT),
            (true, ..) => theme.selected,
            (_, true, true) => theme.lift(theme.card, 5),
            (_, true, false) | (_, false, true) => theme.card,
            _ => theme.background,
        };
        let mut style = theme.base().bg(fill);
        if ghost {
            style = style.fg(theme.lift(theme.background, 30));
        } else if self.selected || aimed || hovered {
            style = style.fg(theme.accent);
        } else if self.muted && !focused {
            style = style.fg(theme.muted);
        }
        cx.surface(
            self.rect,
            fill,
            SURFACE_RADIUS,
            ROW_INSET + cx.press_inset(&self.id),
        );
        let on_pane = matches!(
            pointer.hovered,
            Some(ElementId::Pane(..) | ElementId::ClosePane(..))
        );
        let trailing = self.trailing.filter(|trailing| {
            !self.compact
                && self.rect.width > TRAILING_CELLS + ICON_CELLS
                && ((hovered && !on_pane) || cx.focused(&trailing.id))
        });
        let index = icons::index(self.index).filter(|_| hovered);
        let icon = index.unwrap_or(self.icon);
        let icon_style = match self.icon_color {
            Some(color) if index.is_none() && !ghost => style.fg(color),
            _ => style,
        };
        let tooltip = platform_tooltip(self.tooltip.unwrap_or_default());
        if self.compact {
            let label = format!("{icon} ");
            let visual = icon_rect(self.rect, &label);
            cx.write(
                Rect::new(visual.x, self.rect.y, visual.width, 1),
                Line::from(label).alignment(Alignment::Center),
                icon_style,
            );
            cx.hit(self.id, self.rect, tooltip);
            return RowLayout {
                fill,
                style,
                content: Rect::default(),
            };
        }
        let x = self.rect.x + 1 + self.indent;
        let right = self.rect.right().saturating_sub(1);
        cx.write(
            Rect::new(x, self.rect.y, ICON_CELLS.min(right.saturating_sub(x)), 1),
            icon.to_owned(),
            icon_style,
        );
        let x = (x + ICON_CELLS).min(right);
        let layout = RowLayout {
            fill,
            style,
            content: Rect::new(x, self.rect.y, right - x, self.rect.height),
        };
        cx.hit(self.id, self.rect, tooltip);
        match self.content {
            Content::Label(label) => cx.write(
                Rect::new(layout.content.x, layout.content.y, layout.content.width, 1),
                label.to_owned(),
                style,
            ),
            Content::Custom(paint) => paint(cx, &layout),
        }
        if let Some(trailing) = trailing {
            trailing_control(cx, trailing, self.rect, style);
        }
        layout
    }
}

/// Draws over the row's last cells without clearing them, keeping its surface intact.
pub(crate) fn trailing_control(cx: &mut Canvas, control: Trailing, row: Rect, style: Style) {
    let rect = Rect::new(
        row.right() - TRAILING_CELLS,
        row.y,
        TRAILING_CELLS,
        row.height,
    );
    let symbols = [control.icon, " ", " "];
    for (x, symbol) in (rect.x..rect.right()).zip(symbols) {
        let cell = &mut cx.buf[(x, rect.y)];
        cell.set_symbol(symbol);
        if let Some(fg) = style.fg {
            cell.set_fg(fg);
        }
    }
    cx.hit(control.id, rect, platform_tooltip(control.tooltip.into()));
}

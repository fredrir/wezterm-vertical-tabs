use super::{DROP_TINT, ROW_INSET, SURFACE_RADIUS, row::icon_rect};
use crate::{
    ElementId,
    runtime::canvas::{Canvas, RoundedSurface},
    shortcuts::platform_tooltip,
};
use ratatui::{
    layout::{Alignment, Rect},
    text::Line,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Neutral,
    Danger,
}

enum Kind {
    /// A square glyph on the bare background.
    Icon,
    /// A left-aligned label on a card.
    Text { highlight: bool },
    /// A centered label that tints with its tone when active.
    Action(Tone),
}

pub(crate) struct Button<'a> {
    id: ElementId,
    label: &'a str,
    kind: Kind,
    tooltip: String,
    selected: bool,
}

impl<'a> Button<'a> {
    pub fn icon(id: ElementId, label: &'a str) -> Self {
        Self::new(id, label, Kind::Icon)
    }
    pub fn text(id: ElementId, label: &'a str) -> Self {
        Self::new(id, label, Kind::Text { highlight: false })
    }
    pub fn action(id: ElementId, label: &'a str, tone: Tone) -> Self {
        Self::new(id, label, Kind::Action(tone))
    }
    fn new(id: ElementId, label: &'a str, kind: Kind) -> Self {
        Self {
            id,
            label,
            kind,
            tooltip: String::new(),
            selected: false,
        }
    }
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = tooltip.into();
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    /// Hover and focus fill the whole button instead of only tinting its label.
    pub fn highlight(mut self) -> Self {
        if let Kind::Text { highlight } = &mut self.kind {
            *highlight = true;
        }
        self
    }

    pub fn render(self, rect: Rect, cx: &mut Canvas) {
        match self.kind {
            Kind::Icon => self.render_icon(rect, cx),
            Kind::Text { highlight } => self.render_text(rect, cx, highlight),
            Kind::Action(tone) => self.render_action(rect, cx, tone),
        }
    }

    fn render_icon(self, rect: Rect, cx: &mut Canvas) {
        if rect.is_empty() {
            return;
        }
        let theme = cx.theme;
        let hovered = cx.hovered(&self.id);
        let active = self.selected || hovered || cx.focused(&self.id) || cx.pressed(&self.id);
        let fill = if cx.aimed(&self.id) {
            theme.tint(theme.card, DROP_TINT)
        } else if hovered {
            theme.hover
        } else {
            theme.background
        };
        let visual = icon_rect(rect, self.label);
        let inset = if rect.width >= 3 && rect.height >= 2 {
            5.0
        } else {
            2.0
        };
        cx.fill(RoundedSurface {
            square: true,
            ..RoundedSurface::new(visual, fill, 8.0, inset + cx.press_inset(&self.id))
        });
        let fg = if active { theme.accent } else { theme.muted };
        cx.write(
            Rect::new(visual.x, rect.y, visual.width, 1),
            Line::from(self.label.to_owned()).alignment(Alignment::Center),
            theme.base().fg(fg).bg(fill),
        );
        cx.hit(self.id, rect, platform_tooltip(self.tooltip));
    }

    fn render_text(self, rect: Rect, cx: &mut Canvas, highlight: bool) {
        let theme = cx.theme;
        let lit = highlight && (cx.focused(&self.id) || cx.hovered(&self.id));
        let fill = if self.selected || lit {
            theme.selected
        } else {
            theme.card
        };
        cx.rounded(rect, fill);
        let style = cx.item_style(&self.id, self.selected).bg(fill);
        cx.write(rect, format!(" {}", self.label), style);
        cx.hit(self.id, rect, self.tooltip);
    }

    fn render_action(self, rect: Rect, cx: &mut Canvas, tone: Tone) {
        let theme = cx.theme;
        let active = self.selected || cx.hovered(&self.id);
        let fill = match (tone, active) {
            (Tone::Danger, true) => theme.warn(theme.card, 45),
            (Tone::Danger, false) => theme.warn(theme.card, 22),
            (Tone::Neutral, true) => theme.lift(theme.card, 12),
            (Tone::Neutral, false) => theme.card,
        };
        cx.surface(rect, fill, SURFACE_RADIUS, ROW_INSET);
        let fg = match (tone, active) {
            (Tone::Danger, true) => theme.foreground,
            (_, true) => theme.accent,
            (_, false) => theme.muted,
        };
        cx.write(
            Rect::new(rect.x, rect.y, rect.width, 1),
            Line::from(self.label.to_owned()).alignment(Alignment::Center),
            theme.base().bg(fill).fg(fg),
        );
        cx.hit(self.id, rect, self.tooltip);
    }
}

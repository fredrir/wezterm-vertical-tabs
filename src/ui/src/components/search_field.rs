use crate::components::button::{Button, Tone};
use crate::components::text_input::TextInput;
use crate::components::{EDGE_INSET, ICON_CELLS, SURFACE_RADIUS};
use crate::element::ElementId;
use crate::icons;
use crate::input::TextEditor;
use crate::runtime::canvas::Canvas;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;

const RECORD_CELLS: u16 = 3;

pub(crate) enum Record {
    Toggle { id: ElementId, recording: bool },
    Indicator,
}

pub(crate) struct SearchField<'a> {
    id: ElementId,
    editor: &'a mut TextEditor,
    icon: &'static str,
    inset: f32,
    active: bool,
    placeholder: &'a str,
    tooltip: &'a str,
    record: Option<Record>,
}

impl<'a> SearchField<'a> {
    pub fn new(id: ElementId, editor: &'a mut TextEditor) -> Self {
        Self {
            id,
            editor,
            icon: icons::SEARCH,
            inset: EDGE_INSET,
            active: false,
            placeholder: "",
            tooltip: "",
            record: None,
        }
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }
    pub fn inset(mut self, inset: f32) -> Self {
        self.inset = inset;
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }
    pub fn tooltip(mut self, tooltip: &'a str) -> Self {
        self.tooltip = tooltip;
        self
    }
    pub fn record(mut self, record: Option<Record>) -> Self {
        self.record = record;
        self
    }

    pub fn render(self, rect: Rect, cx: &mut Canvas) {
        let theme = cx.theme;
        cx.surface(rect, theme.card, SURFACE_RADIUS, self.inset);
        let lead = ICON_CELLS.min(rect.width.saturating_sub(1));
        cx.write(
            Rect::new(rect.x + 1, rect.y, lead, 1),
            self.icon,
            theme.muted().bg(theme.card),
        );
        let button = self
            .record
            .as_ref()
            .map_or(0, |_| RECORD_CELLS.min(rect.width.saturating_sub(lead + 2)));
        let edit = Rect::new(
            rect.x + 1 + lead,
            rect.y,
            rect.width.saturating_sub(lead + 2 + button),
            1,
        );
        cx.hit(self.id.clone(), rect, self.tooltip);
        TextInput::new(self.id, self.editor, theme.card)
            .active(self.active)
            .shift(if rect.height == 2 { 0.5 } else { 0.0 })
            .placeholder(self.placeholder)
            .render(edit, cx);
        match self.record.filter(|_| button > 0) {
            Some(Record::Toggle { id, recording }) => {
                Button::action(id, icons::KEYBOARD, Tone::Neutral)
                    .selected(recording)
                    .label_right_padding(1)
                    .render(
                        Rect::new(rect.right() - button - 1, rect.y, button + 1, rect.height),
                        cx,
                    );
            }
            Some(Record::Indicator) => cx.write(
                Rect::new(rect.right() - button, rect.y, button, 1),
                Line::from("●").alignment(Alignment::Center),
                theme.accent().bg(theme.card),
            ),
            None => {}
        }
    }
}

//! One painter for forms, search fields and inline renames.
use crate::{
    ElementId, TextEditor,
    runtime::canvas::{Canvas, Field, RoundedSurface},
};
use ratatui::{
    layout::{Position, Rect},
    style::{Color, Style},
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selection {
    /// Accent-filled, like a native text field.
    Accent,
    /// A rounded selected-row fill that keeps the accent text readable.
    Subtle,
}

pub(crate) struct TextInput<'a> {
    id: ElementId,
    editor: &'a mut TextEditor,
    fill: Color,
    active: bool,
    shift: f32,
    placeholder: Option<&'a str>,
    selection: Selection,
}

impl<'a> TextInput<'a> {
    pub fn new(id: ElementId, editor: &'a mut TextEditor, fill: Color) -> Self {
        Self {
            id,
            editor,
            fill,
            active: false,
            shift: 0.0,
            placeholder: None,
            selection: Selection::Accent,
        }
    }
    /// Draws the selection, preedit and caret.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// Rows the host moves this text down when it centers it in a taller surface.
    pub fn shift(mut self, shift: f32) -> Self {
        self.shift = shift;
        self
    }
    pub fn placeholder(mut self, placeholder: Option<&'a str>) -> Self {
        self.placeholder = placeholder;
        self
    }
    pub fn selection(mut self, selection: Selection) -> Self {
        self.selection = selection;
        self
    }

    pub fn render(self, rect: Rect, cx: &mut Canvas) {
        let theme = cx.theme;
        cx.paint.fields.push(Field {
            id: self.id,
            rect,
            shift: self.shift,
        });
        let editor = self.editor;
        editor.keep_cursor_visible(usize::from(rect.width));
        let display = editor.display_text();
        match self.placeholder.filter(|_| display.is_empty()) {
            Some(placeholder) => {
                cx.write(rect, placeholder.to_owned(), theme.muted().bg(self.fill))
            }
            None => cx.write(
                rect,
                visible_text(&display, editor.scroll_columns),
                theme.base().bg(self.fill),
            ),
        }
        if !self.active {
            return;
        }
        let columns = |range: Range<usize>| {
            let start = range.start.saturating_sub(editor.scroll_columns);
            let end = range.end.saturating_sub(editor.scroll_columns);
            let clamp = |column: usize| rect.x + column.min(usize::from(rect.width)) as u16;
            clamp(start)..clamp(end)
        };
        if let Some(selection) = editor.selection_columns().map(columns)
            && !selection.is_empty()
        {
            let marked = Rect::new(selection.start, rect.y, selection.len() as u16, 1);
            match self.selection {
                Selection::Accent => {
                    cx.mark(RoundedSurface {
                        shift_y: self.shift,
                        ..RoundedSurface::new(marked, theme.accent, 2.0, 0.0)
                    });
                    for x in selection {
                        cx.set_style(
                            x,
                            rect.y,
                            Style::default().fg(theme.background).bg(theme.accent),
                        );
                    }
                }
                Selection::Subtle => {
                    let cells: Vec<_> = selection
                        .clone()
                        .map(|x| cx.buf[(x, rect.y)].clone())
                        .collect();
                    cx.rounded(marked, theme.selected);
                    for (x, cell) in selection.zip(cells) {
                        cx.buf[(x, rect.y)] = cell;
                        cx.set_style(
                            x,
                            rect.y,
                            Style::default().fg(theme.accent).bg(theme.selected),
                        );
                    }
                }
            }
        }
        for x in columns(editor.preedit_columns()) {
            cx.underline(x, rect.y);
        }
        if cx.caret && rect.width > 0 {
            let x = rect.x
                + editor
                    .cursor_columns()
                    .saturating_sub(editor.scroll_columns)
                    .min(usize::from(rect.width - 1)) as u16;
            cx.paint.cursor = Some(Position::new(x, rect.y));
        }
    }
}

/// The graphemes from `scroll` columns on; a clipped wide glyph is left out whole.
fn visible_text(text: &str, scroll: usize) -> String {
    let mut column = 0;
    text.graphemes(true)
        .filter(|grapheme| {
            let start = column;
            column += grapheme.width();
            start >= scroll
        })
        .collect()
}

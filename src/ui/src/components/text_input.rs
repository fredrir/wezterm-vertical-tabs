//! One painter for forms, search fields and inline renames.
use crate::element::ElementId;
use crate::input::TextEditor;
use crate::runtime::canvas::{Canvas, Field, RoundedSurface};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) struct TextInput<'a> {
    id: ElementId,
    editor: &'a mut TextEditor,
    fill: Color,
    active: bool,
    shift: f32,
    placeholder: Option<&'a str>,
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
    /// Shown while the field is empty.
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = Some(placeholder);
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

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub fn command(self) -> bool {
        self.control || self.super_key
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Character(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F2,
    F10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiInput {
    Key {
        key: Key,
        modifiers: Modifiers,
    },
    Text(String),
    Paste(String),
    /// Preedit cursor is a UTF-8 byte offset, as supplied by native IME APIs.
    ImePreedit {
        text: String,
        cursor: Option<usize>,
    },
    ImeCommit(String),
    PointerDown {
        x: u16,
        y: u16,
        button: MouseButton,
        modifiers: Modifiers,
    },
    PointerUp {
        x: u16,
        y: u16,
        button: MouseButton,
    },
    PointerMove {
        x: u16,
        y: u16,
    },
    Scroll {
        x: u16,
        y: u16,
        rows: i32,
    },
    Focus(bool),
    Visibility(bool),
}

impl UiInput {
    pub fn key(key: Key) -> Self {
        Self::Key {
            key,
            modifiers: Modifiers::default(),
        }
    }
}

/// A bounded single-line editor. Cursor and selection endpoints are grapheme indices;
/// byte offsets are computed only when applying edits, so UTF-8 is never sliced inside a
/// grapheme. The native adapter owns clipboard I/O and receives explicit requests.
#[derive(Clone, Debug, Default)]
pub struct TextEditor {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    preedit: String,
    preedit_cursor: usize,
    pub scroll_columns: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditResult {
    Unhandled,
    Changed,
    Copy(String),
    Paste,
    Submit,
    Cancel,
}

impl TextEditor {
    pub const MAX_BYTES: usize = 4096;

    pub fn new(text: impl Into<String>) -> Self {
        let mut editor = Self::default();
        editor.insert(&text.into());
        editor
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn grapheme_count(&self) -> usize {
        self.text.graphemes(true).count()
    }
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        self.anchor
            .filter(|a| *a != self.cursor)
            .map(|a| a.min(self.cursor)..a.max(self.cursor))
    }
    fn byte_at(&self, index: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(index)
            .map_or(self.text.len(), |(b, _)| b)
    }
    pub fn selected_text(&self) -> &str {
        self.selection().map_or("", |range| {
            &self.text[self.byte_at(range.start)..self.byte_at(range.end)]
        })
    }
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.grapheme_count();
    }
    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        self.text
            .replace_range(self.byte_at(range.start)..self.byte_at(range.end), "");
        self.cursor = range.start;
        self.anchor = None;
        true
    }
    pub fn insert(&mut self, value: &str) {
        self.delete_selection();
        let room = Self::MAX_BYTES.saturating_sub(self.text.len());
        let mut insert = String::with_capacity(value.len().min(room));
        for grapheme in value.graphemes(true) {
            if grapheme.chars().any(char::is_control) {
                continue;
            }
            if insert.len() + grapheme.len() > room {
                break;
            }
            insert.push_str(grapheme);
        }
        let offset = self.byte_at(self.cursor);
        self.text.insert_str(offset, &insert);
        // Inserting combining text can merge with an adjacent grapheme. Count from the
        // actual resulting prefix instead of adding the inserted grapheme count.
        self.cursor = self.text[..offset + insert.len()].graphemes(true).count();
        self.anchor = None;
        self.preedit.clear();
    }
    fn move_to(&mut self, target: usize, select: bool) {
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = target.min(self.grapheme_count());
        self.preedit.clear();
    }
    fn word_boundary(&self, backwards: bool) -> usize {
        let graphemes: Vec<_> = self.text.graphemes(true).collect();
        let whitespace = |g: &str| g.chars().all(char::is_whitespace);
        let mut i = self.cursor;
        if backwards {
            while i > 0 && whitespace(graphemes[i - 1]) {
                i -= 1;
            }
            while i > 0 && !whitespace(graphemes[i - 1]) {
                i -= 1;
            }
        } else {
            while i < graphemes.len() && !whitespace(graphemes[i]) {
                i += 1;
            }
            while i < graphemes.len() && whitespace(graphemes[i]) {
                i += 1;
            }
        }
        i
    }
    pub fn preedit(&self) -> &str {
        &self.preedit
    }
    pub fn set_preedit(&mut self, text: &str, cursor: Option<usize>) {
        self.preedit = TextEditor::new(text).text;
        let mut offset = cursor.unwrap_or(self.preedit.len()).min(self.preedit.len());
        while !self.preedit.is_char_boundary(offset) {
            offset -= 1;
        }
        self.preedit_cursor = self.preedit[..offset].graphemes(true).count();
    }
    pub fn display_text(&self) -> String {
        let offset = self.byte_at(self.cursor);
        let mut result = self.text.clone();
        result.insert_str(offset, &self.preedit);
        result
    }
    pub fn cursor_columns(&self) -> usize {
        self.text[..self.byte_at(self.cursor)].width()
            + self
                .preedit
                .graphemes(true)
                .take(self.preedit_cursor)
                .map(str::width)
                .sum::<usize>()
    }
    pub fn selection_columns(&self) -> Option<std::ops::Range<usize>> {
        self.selection().map(|range| {
            self.text[..self.byte_at(range.start)].width()
                ..self.text[..self.byte_at(range.end)].width()
        })
    }
    pub fn preedit_columns(&self) -> std::ops::Range<usize> {
        let start = self.text[..self.byte_at(self.cursor)].width();
        start..start + self.preedit.width()
    }
    pub fn keep_cursor_visible(&mut self, width: usize) {
        let column = self.cursor_columns();
        if column < self.scroll_columns {
            self.scroll_columns = column;
        }
        if column >= self.scroll_columns + width.max(1) {
            self.scroll_columns = column.saturating_sub(width.saturating_sub(1));
        }
        // Align horizontal scrolling with a grapheme boundary; never expose half of a
        // double-width glyph or shift the native caret by its clipped continuation cell.
        let mut boundary = 0;
        for grapheme in self.display_text().graphemes(true) {
            if boundary >= self.scroll_columns {
                break;
            }
            boundary += grapheme.width();
        }
        self.scroll_columns = boundary;
    }
    pub fn click_column(&mut self, column: usize, select: bool) {
        let target = column + self.scroll_columns;
        let mut width = 0;
        let index = self
            .text
            .graphemes(true)
            .position(|g| {
                width += g.width();
                width > target
            })
            .unwrap_or(self.grapheme_count());
        self.move_to(index, select);
    }
    pub fn key(&mut self, key: &Key, mods: Modifiers) -> EditResult {
        use EditResult::*;
        if mods.command() {
            match key {
                Key::Character('a' | 'A') => {
                    self.select_all();
                    return Changed;
                }
                Key::Character('c' | 'C') => return Copy(self.selected_text().to_owned()),
                Key::Character('x' | 'X') => {
                    let value = self.selected_text().to_owned();
                    self.delete_selection();
                    return Copy(value);
                }
                Key::Character('v' | 'V') => return Paste,
                _ => {}
            }
        }
        let word = mods.control || mods.alt;
        match key {
            Key::Enter => Submit,
            Key::Escape => {
                if self.preedit.is_empty() {
                    Cancel
                } else {
                    self.preedit.clear();
                    Changed
                }
            }
            Key::Character(c) if !mods.command() && !mods.alt => {
                self.insert(&c.to_string());
                Changed
            }
            Key::Left => {
                let target = if mods.super_key {
                    0
                } else if word {
                    self.word_boundary(true)
                } else if !mods.shift {
                    self.selection()
                        .map_or(self.cursor.saturating_sub(1), |s| s.start)
                } else {
                    self.cursor.saturating_sub(1)
                };
                self.move_to(target, mods.shift);
                Changed
            }
            Key::Right => {
                let target = if mods.super_key {
                    self.grapheme_count()
                } else if word {
                    self.word_boundary(false)
                } else if !mods.shift {
                    self.selection().map_or(self.cursor + 1, |s| s.end)
                } else {
                    self.cursor + 1
                };
                self.move_to(target, mods.shift);
                Changed
            }
            Key::Home => {
                self.move_to(0, mods.shift);
                Changed
            }
            Key::End => {
                self.move_to(self.grapheme_count(), mods.shift);
                Changed
            }
            Key::Backspace => {
                if !self.delete_selection() && self.cursor > 0 {
                    let target = if mods.super_key {
                        0
                    } else if word {
                        self.word_boundary(true)
                    } else {
                        self.cursor - 1
                    };
                    self.anchor = Some(target);
                    self.delete_selection();
                }
                self.preedit.clear();
                Changed
            }
            Key::Delete => {
                if !self.delete_selection() && self.cursor < self.grapheme_count() {
                    let target = if word {
                        self.word_boundary(false)
                    } else {
                        self.cursor + 1
                    };
                    self.anchor = Some(target);
                    self.delete_selection();
                }
                self.preedit.clear();
                Changed
            }
            _ => Unhandled,
        }
    }
}

/// UI strings never contain terminal control characters; there is no ANSI transport.
pub fn display_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

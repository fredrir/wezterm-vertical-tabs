mod editor;

pub use editor::{EditResult, TextEditor};

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
    /// Preedit cursor is a UTF-8 byte offset, as supplied by IME APIs.
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

/// UI strings never contain terminal control characters; there is no ANSI transport.
pub fn display_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

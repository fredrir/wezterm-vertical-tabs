use super::*;
use std::borrow::Cow;
use window::{KeyCode, MouseEventKind, MousePress};

/// Shortcuts come from raw metadata when present; text and IME keep the processed key.
struct DecodedKey<'a> {
    down: bool,
    modifiers: ui::Modifiers,
    shortcut: Option<ui::Key>,
    clipboard: Option<ui::Key>,
    recording: Cow<'a, KeyCode>,
    native_modifiers: window::Modifiers,
    text: Option<TextKey<'a>>,
}

struct TextKey<'a> {
    code: &'a KeyCode,
    command: bool,
}

impl<'a> DecodedKey<'a> {
    fn raw(key: &'a window::RawKeyEvent) -> Self {
        let mods = modifiers(key.modifiers);
        Self {
            down: key.key_is_down,
            modifiers: mods,
            shortcut: shortcut_key(&key.key, mods),
            clipboard: editor_clipboard_key(&key.key, mods),
            recording: Cow::Borrowed(&key.key),
            native_modifiers: key.modifiers,
            text: None,
        }
    }

    fn processed(key: &'a window::KeyEvent) -> Self {
        let native_modifiers = key.raw.as_ref().map_or(key.modifiers, |raw| raw.modifiers);
        let mods = modifiers(native_modifiers);
        let recording = key.raw.as_ref().map_or_else(
            || logical_shortcut_code(&key.key, mods),
            |raw| Cow::Borrowed(&raw.key),
        );
        Self {
            down: key.key_is_down,
            modifiers: mods,
            shortcut: shortcut_key(&recording, mods),
            clipboard: editor_clipboard_key(&key.key, mods),
            recording,
            native_modifiers,
            text: Some(TextKey {
                code: &key.key,
                command: key
                    .modifiers
                    .intersects(window::Modifiers::SUPER | window::Modifiers::CTRL),
            }),
        }
    }
}

impl Adapter {
    fn record_shortcut(&mut self, input: &DecodedKey<'_>) {
        let key = match input.recording.as_ref() {
            KeyCode::Physical(physical) => physical.to_key_code(),
            key => key.clone(),
        };
        if key.is_modifier() || matches!(key, KeyCode::Composed(_) | KeyCode::VoidSymbol) {
            return;
        }
        let shortcut = if key == KeyCode::Char('\u{1b}') {
            String::new()
        } else if self.app.ui().recording_keybind() {
            let Some(code) = &input.shortcut else {
                return;
            };
            ui::shortcut_chord(code, input.modifiers)
        } else {
            palette::shortcut_label(&key, input.native_modifiers)
        };
        self.ui_input(ui::UiInput::RecordShortcut(shortcut));
    }

    pub(super) fn terminal_key(&mut self, input: Input<'_>, escape: HideOnEscape) -> TerminalKey {
        let key = match input {
            Input::RawKey(key) => DecodedKey::raw(key),
            Input::Key(key) => DecodedKey::processed(key),
            _ => return TerminalKey::Pass,
        };
        let Some(code) = key.shortcut else {
            return TerminalKey::Pass;
        };
        let settings = &self.app.model().settings;
        let escape = (code == ui::Key::Escape && key.modifiers == ui::Modifiers::default())
            .then_some(escape);
        let command = match escape {
            Some(HideOnEscape::Always) => Command::HideTerminalOverlay,
            Some(HideOnEscape::Auto) if key.down => return TerminalKey::Escape,
            Some(HideOnEscape::Auto) => return TerminalKey::Pass,
            _ => {
                match core::keybinds::action(settings, &ui::shortcut_chord(&code, key.modifiers)) {
                    Some("quick_terminal") => Command::ToggleTerminalOverlay,
                    Some("close") => Command::HideTerminalOverlay,
                    _ => return TerminalKey::Pass,
                }
            }
        };
        if key.down {
            self.commands.push(command);
        }
        TerminalKey::Handled
    }
    fn handle_key(&mut self, key: DecodedKey<'_>) -> bool {
        if self.app.ui().recording_shortcut() {
            if key.down {
                self.record_shortcut(&key);
            }
            return true;
        }
        let code = self
            .app
            .ui()
            .text_input_active()
            .then_some(key.clipboard.as_ref())
            .flatten()
            .or_else(|| {
                key.shortcut.as_ref().filter(|code| {
                    ui::matches_shortcut(&self.app.model().settings, code, key.modifiers)
                })
            })
            .cloned();
        if let Some(code) = code {
            if key.down {
                self.ui_input(ui::UiInput::Key {
                    key: code,
                    modifiers: key.modifiers,
                });
            }
            return true;
        }
        // Raw events only claim shortcuts; text belongs to the processed event.
        let Some(text) = key.text else {
            return false;
        };
        if !key.down {
            return self.keyboard_focus();
        }
        // Unclaimed Cmd/Ctrl chords are key bindings, never text, even in a modal.
        if let KeyCode::Composed(text_value) = text.code {
            if !self.keyboard_focus() || text.command {
                return false;
            }
            self.ui_input(ui::UiInput::ImeCommit(text_value.clone()));
            return true;
        }
        let code = if text.command {
            key.shortcut
        } else {
            ui_key(text.code).filter(|code| !matches!(code, ui::Key::Function(_)))
        };
        let Some(code) = code else {
            return self.keyboard_focus();
        };
        if !self.keyboard_focus() {
            return false;
        }
        // Navigation bindings still belong to the terminal when the rail has focus.
        if text.command && (!self.app.is_modal() || matches!(code, ui::Key::Character(_))) {
            return false;
        }
        self.ui_input(ui::UiInput::Key {
            key: code,
            modifiers: key.modifiers,
        });
        true
    }

    pub(super) fn handle_input(&mut self, input: Input<'_>) -> bool {
        match input {
            Input::RawKey(key) => self.handle_key(DecodedKey::raw(key)),
            Input::Key(key) => self.handle_key(DecodedKey::processed(key)),
            Input::Mouse(event, geometry) => {
                let bounds = geometry.ui_bounds(self.content_page() || self.overlay_surface());
                let inside = bounds.contains(event.coords.x as f32, event.coords.y as f32);
                let modal = self.app.is_modal();
                let in_sidebar = geometry
                    .sidebar
                    .contains(event.coords.x as f32, event.coords.y as f32);
                if !in_sidebar && !modal && !self.pointer_captured {
                    if matches!(event.kind, MouseEventKind::Press(_)) {
                        self.app.ui_mut().release_focus();
                        self.cancel_paste();
                    } else if matches!(event.kind, MouseEventKind::Move) {
                        self.ui_input(ui::UiInput::PointerMove {
                            x: u16::MAX,
                            y: u16::MAX,
                        });
                    }
                    return false;
                }
                let x = (event.coords.x as f32 - bounds.x - self.surface.offset.0)
                    / geometry.cell_width.max(1.);
                let y = (event.coords.y as f32 - bounds.y - self.surface.offset.1)
                    / geometry.cell_height.max(1.);
                self.app
                    .ui_mut()
                    .set_pointer_fraction(x.rem_euclid(1.), y.rem_euclid(1.));
                let (x, y) = (x.floor(), y.floor());
                let (x, y) = if inside
                    && x >= 0.
                    && y >= 0.
                    && x < self.surface.columns as f32
                    && y < self.surface.rows.len() as f32
                {
                    (x as u16, y as u16)
                } else {
                    (u16::MAX, u16::MAX)
                };
                if geometry.header_height > 0.
                    && (event.coords.y as f32) < bounds.y + geometry.header_height
                    && !self.pointer_captured
                    && !self.app.ui().has_overlay()
                    && self.app.ui().hit_test(x, y).is_none()
                {
                    return false;
                }
                let mods = modifiers(event.modifiers);
                let input = match event.kind {
                    MouseEventKind::Press(button) => {
                        self.pointer_captured = true;
                        ui::UiInput::PointerDown {
                            x,
                            y,
                            button: mouse_button(button),
                            modifiers: mods,
                        }
                    }
                    MouseEventKind::Release(button) => {
                        self.pointer_captured = false;
                        ui::UiInput::PointerUp {
                            x,
                            y,
                            button: mouse_button(button),
                        }
                    }
                    MouseEventKind::Move => ui::UiInput::PointerMove { x, y },
                    MouseEventKind::VertWheel(rows) => ui::UiInput::Scroll {
                        x,
                        y,
                        rows: -(rows as i32),
                    },
                    _ => return inside || modal,
                };
                self.ui_input(input);
                true
            }
            Input::Composition(status) => {
                if !self.keyboard_focus() {
                    return false;
                }
                self.ui_input(ui::UiInput::ImePreedit {
                    text: match status {
                        window::DeadKeyStatus::Composing(text) => text.clone(),
                        _ => String::new(),
                    },
                    cursor: None,
                });
                true
            }
            Input::Focus(focused) => {
                if !focused {
                    self.pointer_captured = false;
                }
                self.cancel_paste();
                self.app.set_focus(focused);
                if focused {
                    self.app.refresh_storage();
                    self.storage();
                }
                false
            }
        }
    }
}

fn modifiers(m: window::Modifiers) -> ui::Modifiers {
    ui::Modifiers {
        shift: m.contains(window::Modifiers::SHIFT),
        control: m.contains(window::Modifiers::CTRL),
        alt: m.contains(window::Modifiers::ALT),
        super_key: m.contains(window::Modifiers::SUPER),
    }
}
fn mouse_button(button: MousePress) -> ui::MouseButton {
    match button {
        MousePress::Right => ui::MouseButton::Right,
        MousePress::Middle => ui::MouseButton::Middle,
        _ => ui::MouseButton::Left,
    }
}

// Raw and physical keys have not been encoded as Ctrl-letter bytes.
fn shortcut_key(key: &KeyCode, mods: ui::Modifiers) -> Option<ui::Key> {
    if let KeyCode::Physical(code) = key {
        return shortcut_key(&code.to_key_code(), mods);
    }
    let mut key = ui_key(key)?;
    if let (ui::Key::Character(c), true) = (&mut key, mods.shift) {
        *c = match *c {
            '<' => ',',
            '!' => '1',
            '@' => '2',
            '#' => '3',
            '$' => '4',
            '%' => '5',
            '^' => '6',
            '&' => '7',
            '*' => '8',
            '(' => '9',
            ')' => '0',
            '>' => '.',
            '?' => '/',
            ':' => ';',
            '"' => '\'',
            '_' => '-',
            '+' => '=',
            '{' => '[',
            '}' => ']',
            '|' => '\\',
            '~' => '`',
            c => c,
        };
    }
    Some(key)
}

fn ui_key(key: &KeyCode) -> Option<ui::Key> {
    Some(match key {
        KeyCode::Char('\t') => ui::Key::Tab,
        KeyCode::Char('\r' | '\n') => ui::Key::Enter,
        KeyCode::Char('\u{1b}') => ui::Key::Escape,
        KeyCode::Char('\u{8}') => ui::Key::Backspace,
        KeyCode::Char('\u{7f}') => ui::Key::Delete,
        KeyCode::Char(c) => ui::Key::Character(*c),
        KeyCode::LeftArrow => ui::Key::Left,
        KeyCode::RightArrow => ui::Key::Right,
        KeyCode::UpArrow => ui::Key::Up,
        KeyCode::DownArrow => ui::Key::Down,
        KeyCode::Home => ui::Key::Home,
        KeyCode::End => ui::Key::End,
        KeyCode::PageUp => ui::Key::PageUp,
        KeyCode::PageDown => ui::Key::PageDown,
        KeyCode::Function(2) => ui::Key::F2,
        KeyCode::Function(10) => ui::Key::F10,
        KeyCode::Function(n) => ui::Key::Function(*n),
        _ => return None,
    })
}

fn logical_shortcut_code(key: &KeyCode, mods: ui::Modifiers) -> Cow<'_, KeyCode> {
    // Without raw metadata Ctrl+H arrives as BS; decode Ctrl-letter bytes except Tab and Enter.
    match key {
        KeyCode::Char(c)
            if mods.control
                && ('\u{1}'..='\u{1a}').contains(c)
                && !matches!(c, '\t' | '\r' | '\n') =>
        {
            Cow::Owned(KeyCode::Char(char::from(*c as u8 + b'a' - 1)))
        }
        _ => Cow::Borrowed(key),
    }
}

fn logical_shortcut_key(key: &KeyCode, mods: ui::Modifiers) -> Option<ui::Key> {
    shortcut_key(&logical_shortcut_code(key, mods), mods)
}

fn editor_clipboard_key(key: &KeyCode, mods: ui::Modifiers) -> Option<ui::Key> {
    if !mods.command() || mods.alt {
        return None;
    }
    logical_shortcut_key(key, mods).filter(|key| {
        matches!(
            key,
            ui::Key::Character('a' | 'A' | 'c' | 'C' | 'x' | 'X' | 'v' | 'V')
        )
    })
}

#[cfg(test)]
#[path = "../tests/keys.rs"]
mod tests;

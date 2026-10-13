//! Clipboard sequencing belongs to the input stream, including queued IME and pointer events.
use super::*;
use std::{borrow::Cow, collections::VecDeque};
use window::{KeyCode, MouseEventKind, MousePress};

const PASTE_TIMEOUT: Duration = Duration::from_secs(1);
pub(super) const MAX_PASTE_INPUTS: usize = 128;
pub(super) const MAX_PASTE_INPUT_BYTES: usize = 16 * 1024;

struct PendingPaste {
    token: u64,
    deadline: Instant,
    inputs: VecDeque<ui::UiInput>,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct InputState {
    epoch: u64,
    pending: Option<PendingPaste>,
}

/// Host delivery stays with the adapter; the input stream decides which events
/// can be delivered and when a clipboard result is still valid.
enum Delivery {
    Deferred,
    Input(ui::UiInput),
    Replay(PasteReplay, Option<ui::UiInput>),
}

struct PasteReplay {
    text: Option<String>,
    failed: bool,
    revision: u64,
    inputs: VecDeque<ui::UiInput>,
    pointer_gesture: bool,
    started: bool,
}

impl InputState {
    pub fn request_paste(&mut self, now: Instant) -> u64 {
        self.pending = Some(PendingPaste {
            token: self.epoch,
            deadline: now + PASTE_TIMEOUT,
            inputs: VecDeque::new(),
            bytes: 0,
        });
        self.epoch
    }

    pub fn cancel_paste(&mut self) {
        self.pending = None;
        self.advance();
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.deadline)
    }

    fn advance(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn delivered(&mut self, input: &ui::UiInput, pointer_captured: bool) {
        // Passive pointer motion and composition cleanup retain an outstanding paste.
        let changes_input = match input {
            ui::UiInput::PointerMove { .. } => pointer_captured,
            ui::UiInput::ImePreedit { text, .. } => !text.is_empty(),
            _ => true,
        };
        if changes_input {
            self.advance();
        }
    }

    fn route(
        &mut self,
        input: ui::UiInput,
        now: Instant,
        app: &WindowApp,
        pointer_captured: bool,
    ) -> Delivery {
        if self.pending.is_none() {
            return Delivery::Input(input);
        }
        if !app.ui().text_input_active() {
            self.cancel_paste();
        } else if self.defer_until_paste(&input, app, pointer_captured) {
            let bytes = match &input {
                ui::UiInput::Text(text)
                | ui::UiInput::Paste(text)
                | ui::UiInput::ImeCommit(text)
                | ui::UiInput::ImePreedit { text, .. } => text.len(),
                _ => 0,
            };
            let pending = self.pending.as_mut().unwrap();
            if now < pending.deadline
                && pending.inputs.len() < MAX_PASTE_INPUTS
                && bytes <= MAX_PASTE_INPUT_BYTES.saturating_sub(pending.bytes)
            {
                pending.bytes += bytes;
                pending.inputs.push_back(input);
                return Delivery::Deferred;
            }
            return self
                .finish(None, Some(input), app)
                .map_or(Delivery::Deferred, |replay| Delivery::Replay(replay, None));
        } else if !matches!(&input, ui::UiInput::PointerMove { .. })
            && !matches!(&input, ui::UiInput::ImePreedit { text, .. } if text.is_empty())
        {
            self.cancel_paste();
        } else if let Some(replay) = self.expire(now, app) {
            return Delivery::Replay(replay, Some(input));
        }
        Delivery::Input(input)
    }

    fn defer_until_paste(
        &self,
        input: &ui::UiInput,
        app: &WindowApp,
        pointer_captured: bool,
    ) -> bool {
        match input {
            ui::UiInput::Key { key, modifiers } => {
                *key != ui::Key::Escape
                    && !ui::matches_shortcut(&app.model().settings, key, *modifiers)
            }
            ui::UiInput::PointerDown { x, y, .. } => {
                app.ui().hit_test(*x, *y).is_some_and(|hit| match &hit.id {
                    ui::ElementId::Editor
                    | ui::ElementId::SettingsSearch
                    | ui::ElementId::Submit => true,
                    ui::ElementId::Menu(id) => {
                        matches!(id.as_str(), "cut" | "copy" | "paste" | "select-all")
                    }
                    _ => false,
                })
            }
            ui::UiInput::PointerMove { .. } => pointer_captured,
            ui::UiInput::ImePreedit { text, .. } => {
                !text.is_empty()
                    || self.pending.as_ref().is_some_and(|pending| {
                        pending.inputs.iter().any(|input| {
                            matches!(
                                input,
                                ui::UiInput::ImePreedit { .. } | ui::UiInput::ImeCommit(_)
                            )
                        })
                    })
            }
            ui::UiInput::PointerUp { .. }
            | ui::UiInput::Scroll { .. }
            | ui::UiInput::Text(_)
            | ui::UiInput::Paste(_)
            | ui::UiInput::ImeCommit(_) => true,
            ui::UiInput::Focus(_) | ui::UiInput::Visibility(_) | ui::UiInput::RecordShortcut(_) => {
                false
            }
        }
    }

    fn expire(&mut self, now: Instant, app: &WindowApp) -> Option<PasteReplay> {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.finish(None, None, app)
        } else {
            None
        }
    }

    fn complete(
        &mut self,
        token: u64,
        text: Option<String>,
        now: Instant,
        app: &WindowApp,
    ) -> Option<PasteReplay> {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            return self.finish(None, None, app);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.token == token)
        {
            self.finish(text, None, app)
        } else {
            None
        }
    }

    fn finish(
        &mut self,
        text: Option<String>,
        trailing: Option<ui::UiInput>,
        app: &WindowApp,
    ) -> Option<PasteReplay> {
        let mut pending = self.pending.take()?;
        if pending.token != self.epoch || !app.ui().text_input_active() {
            return None;
        }
        self.advance();
        pending.inputs.extend(trailing);
        Some(PasteReplay {
            failed: text.is_none(),
            text,
            revision: app.model().revision,
            inputs: pending.inputs,
            pointer_gesture: false,
            started: false,
        })
    }
}

impl PasteReplay {
    fn next(&mut self, app: &WindowApp) -> Option<ui::UiInput> {
        // Stop once replay submits or leaves the editor, but finish an in-flight
        // pointer gesture so a deferred mouse-up cannot escape into another editor.
        if self.started && !self.pointer_gesture && !self.same_editor(app) {
            return None;
        }
        self.started = true;
        while let Some(input) = self.inputs.pop_front() {
            // A failed read must not save the old value or launch further clipboard
            // work from a deadline repaint. Keep typed text and caret edits for retry.
            if self.failed && !safe_after_failure(&input) {
                continue;
            }
            match &input {
                ui::UiInput::PointerDown { .. } => self.pointer_gesture = true,
                ui::UiInput::PointerUp { .. } => self.pointer_gesture = false,
                _ => {}
            }
            return Some(input);
        }
        None
    }

    fn same_editor(&self, app: &WindowApp) -> bool {
        app.model().revision == self.revision && app.ui().text_input_active()
    }
}

fn safe_after_failure(input: &ui::UiInput) -> bool {
    match input {
        ui::UiInput::Text(_) | ui::UiInput::ImeCommit(_) | ui::UiInput::ImePreedit { .. } => true,
        ui::UiInput::Key { key, modifiers } => {
            matches!(
                key,
                ui::Key::Character(_)
                    | ui::Key::Backspace
                    | ui::Key::Delete
                    | ui::Key::Left
                    | ui::Key::Right
                    | ui::Key::Home
                    | ui::Key::End
            ) && !(modifiers.command()
                && matches!(key, ui::Key::Character('c' | 'C' | 'x' | 'X' | 'v' | 'V')))
        }
        _ => false,
    }
}

impl Adapter {
    pub(super) fn ui_input(&mut self, input: ui::UiInput) {
        self.ui_input_at(input, Instant::now());
    }

    pub(super) fn ui_input_at(&mut self, input: ui::UiInput, now: Instant) {
        match self
            .input
            .route(input, now, &self.app, self.pointer_captured)
        {
            Delivery::Deferred => {}
            Delivery::Input(input) => self.deliver_ui_input(input),
            Delivery::Replay(replay, input) => {
                self.replay_paste(replay);
                if let Some(input) = input {
                    self.deliver_ui_input(input);
                }
            }
        }
    }

    pub(super) fn cancel_paste(&mut self) {
        self.input.cancel_paste();
    }

    pub(super) fn show_terminal(&mut self, command: Command) {
        self.cancel_paste();
        self.app.ui_mut().interrupt();
        self.commands.push(command);
    }

    pub(super) fn expire_paste(&mut self, now: Instant) {
        if let Some(replay) = self.input.expire(now, &self.app) {
            self.replay_paste(replay);
        }
    }

    pub(super) fn complete_paste(&mut self, token: u64, text: Option<String>) {
        if let Some(replay) = self.input.complete(token, text, Instant::now(), &self.app) {
            self.replay_paste(replay);
        }
    }

    fn replay_paste(&mut self, mut replay: PasteReplay) {
        if let Some(text) = replay.text.take() {
            self.deliver_ui_input(ui::UiInput::Paste(text));
        } else {
            log::warn!("tabs: clipboard read failed or timed out");
        }
        while let Some(input) = replay.next(&self.app) {
            self.ui_input(input);
        }
        if replay.failed && replay.same_editor(&self.app) {
            self.app
                .ui_mut()
                .clipboard_failed("Clipboard unavailable. Try pasting again.");
        }
    }

    pub(super) fn deliver_ui_input(&mut self, input: ui::UiInput) {
        self.input.delivered(&input, self.pointer_captured);
        let before = self.app.model().revision;
        if matches!(
            input,
            ui::UiInput::Key { .. } | ui::UiInput::PointerUp { .. }
        ) {
            self.sync_foreign();
        }
        self.app.ui_mut().set_clock(self.epoch.elapsed());
        let result = self.app.input(input);
        self.apply(result);
        self.finish_update(before);
    }
}

/// Shortcut identity comes from raw metadata when present; text and IME retain
/// the processed event's key and modifiers. These representations are distinct
/// even when the host reports them together in one event.
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
            commands::shortcut_label(&key, input.native_modifiers)
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
pub(super) fn shortcut_key(key: &KeyCode, mods: ui::Modifiers) -> Option<ui::Key> {
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
    // Without raw metadata, Ctrl+H arrives as BS. Keep the host's Tab/Enter
    // conventions while decoding other processed Ctrl-letter bytes.
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

pub(super) fn logical_shortcut_key(key: &KeyCode, mods: ui::Modifiers) -> Option<ui::Key> {
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

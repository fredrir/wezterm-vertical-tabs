//! Clipboard sequencing belongs to the input stream, including queued IME and pointer events.
use super::*;
use std::collections::VecDeque;

const PASTE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_PASTE_INPUTS: usize = 128;
const MAX_PASTE_INPUT_BYTES: usize = 16 * 1024;

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

/// The input stream decides what is delivered and whether a clipboard result is still valid.
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
        // Stop once replay leaves the editor, but finish a pointer gesture so its mouse-up stays.
        if self.started && !self.pointer_gesture && !self.same_editor(app) {
            return None;
        }
        self.started = true;
        while let Some(input) = self.inputs.pop_front() {
            // A failed read keeps typed text and caret edits, never submits or clipboard work.
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

    fn ui_input_at(&mut self, input: ui::UiInput, now: Instant) {
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

    fn deliver_ui_input(&mut self, input: ui::UiInput) {
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

#[cfg(test)]
#[path = "../tests/paste.rs"]
mod tests;

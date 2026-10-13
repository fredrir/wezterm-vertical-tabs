use super::range::{Range, number};
use crate::input::TextEditor;
use vtabs_core::{Intent, Model};

struct Entry {
    key: String,
    editor: TextEditor,
}

#[derive(Default)]
enum Mode {
    #[default]
    Idle,
    Recording,
    Entry(Entry),
    Slider {
        key: String,
        value: f64,
    },
}

#[derive(Default)]
pub(crate) struct Editing {
    mode: Mode,
}

impl Editing {
    pub fn cancel(&mut self) {
        self.mode = Mode::Idle;
    }
    pub fn recording(&self) -> bool {
        matches!(self.mode, Mode::Recording)
    }
    pub fn toggle_recording(&mut self) {
        self.mode = if self.recording() {
            Mode::Idle
        } else {
            Mode::Recording
        };
    }
    pub fn stop_recording(&mut self) {
        if self.recording() {
            self.cancel();
        }
    }
    fn entry(&self) -> Option<&Entry> {
        match &self.mode {
            Mode::Entry(entry) => Some(entry),
            _ => None,
        }
    }
    fn entry_mut(&mut self) -> Option<&mut Entry> {
        match &mut self.mode {
            Mode::Entry(entry) => Some(entry),
            _ => None,
        }
    }
    pub fn has_entry(&self) -> bool {
        self.entry().is_some()
    }
    pub fn entry_key(&self) -> Option<&str> {
        self.entry().map(|entry| entry.key.as_str())
    }
    pub fn editor(&self) -> Option<&TextEditor> {
        self.entry().map(|entry| &entry.editor)
    }
    pub fn editor_mut(&mut self) -> Option<&mut TextEditor> {
        self.entry_mut().map(|entry| &mut entry.editor)
    }
    pub fn begin_entry(&mut self, model: &Model, key: &str, typed: Option<char>) -> bool {
        let Some(range) = Range::for_key(key) else {
            return false;
        };
        if typed.is_none() && self.entry().is_some_and(|entry| entry.key == key) {
            return false;
        }
        let mut editor = TextEditor::new(range.entry_text(number(model, key)));
        editor.select_all();
        if let Some(typed) = typed {
            editor = TextEditor::new(range.sanitize(&typed.to_string()));
        }
        self.mode = Mode::Entry(Entry {
            key: key.into(),
            editor,
        });
        true
    }
    pub fn cancel_entry(&mut self) {
        if self.entry().is_some() {
            self.cancel();
        }
    }
    pub fn sanitize_entry(&mut self) {
        let Some(entry) = self.entry_mut() else {
            return;
        };
        let Some(range) = Range::for_key(&entry.key) else {
            return;
        };
        let clean = range.sanitize(entry.editor.text());
        if clean != entry.editor.text() {
            entry.editor = TextEditor::new(clean);
        }
    }
    pub fn commit_entry(&mut self) -> Option<Intent> {
        self.entry()?;
        let Mode::Entry(entry) = std::mem::take(&mut self.mode) else {
            unreachable!();
        };
        let range = Range::for_key(&entry.key)?;
        let value = range.parse(entry.editor.text())?;
        Some(Intent::SetSetting {
            key: entry.key,
            value: range.json(value),
        })
    }
    pub fn step_entry(&mut self, model: &Model, steps: i32) -> bool {
        let Some(entry) = self.entry_mut() else {
            return false;
        };
        let Some(range) = Range::for_key(&entry.key) else {
            return false;
        };
        let current = range
            .parse(entry.editor.text())
            .unwrap_or_else(|| number(model, &entry.key));
        entry.editor = TextEditor::new(range.entry_text(range.step(current, steps)));
        true
    }
    pub fn slider_value(&self, key: &str) -> Option<f64> {
        match &self.mode {
            Mode::Slider {
                key: dragged,
                value,
            } if dragged == key => Some(*value),
            _ => None,
        }
    }
    pub fn preview_slider(&mut self, key: &str, value: f64) {
        self.mode = Mode::Slider {
            key: key.into(),
            value,
        };
    }
    pub fn commit_slider(&mut self, key: &str) -> Option<Intent> {
        if !matches!(self.mode, Mode::Slider { .. }) {
            return None;
        }
        let Mode::Slider {
            key: dragged,
            value,
        } = std::mem::take(&mut self.mode)
        else {
            unreachable!();
        };
        if dragged != key {
            return None;
        }
        Some(Intent::SetSetting {
            key: dragged,
            value: Range::for_key(key)?.json(value),
        })
    }
}

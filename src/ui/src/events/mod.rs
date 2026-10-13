mod drag_drop;
mod editor;
mod keyboard;
mod pointer;

pub(crate) use drag_drop::{DropMotion, DropTarget};
pub(crate) use editor::EditorSlot;
pub(crate) use pointer::{Pointer, Press};

use crate::SidebarUi;
use crate::actions::Action;
use crate::input::{MouseButton, UiInput};
use crate::intent::UiIntent;
use crate::overlays::{MenuItem, Overlay};
use vtabs_core::Model;

impl SidebarUi {
    pub fn event(&mut self, model: &Model, event: UiInput) -> Vec<UiIntent> {
        self.prepare(model, self.frame.now);
        let mut intents = Vec::new();
        match event {
            UiInput::Focus(focused) => {
                self.host.focused = focused;
                if !focused {
                    self.suspend();
                } else if self.caret_blinks() {
                    self.reset_caret();
                }
                self.frame.dirty = true;
            }
            UiInput::Visibility(visible) => {
                self.host.visible = visible;
                if !visible {
                    self.suspend();
                } else {
                    self.frame.dirty = true;
                    if self.caret_blinks() {
                        self.reset_caret();
                    }
                }
            }
            UiInput::PointerMove { x, y } => self.pointer_move(model, x, y),
            UiInput::PointerDown {
                x,
                y,
                button,
                modifiers,
            } => self.pointer_down(model, x, y, button, modifiers, &mut intents),
            UiInput::PointerUp {
                x,
                y,
                button: MouseButton::Left,
            } => self.pointer_up(model, x, y, &mut intents),
            UiInput::PointerUp { .. } => {}
            UiInput::Scroll { x, y, rows } => self.scroll(model, x, y, rows),
            UiInput::RecordShortcut(shortcut) => {
                self.record_shortcut(model, shortcut, &mut intents)
            }
            UiInput::Text(text) | UiInput::Paste(text) | UiInput::ImeCommit(text) => {
                self.insert_text(&text)
            }
            UiInput::ImePreedit { text, cursor } => self.set_preedit(&text, cursor),
            UiInput::Key { key, modifiers } => self.key(model, key, modifiers, &mut intents),
        }
        intents
    }

    pub fn clipboard_failed(&mut self, message: &str) {
        if matches!(self.overlays.current(), Some(Overlay::Form(_))) {
            self.set_error(message);
        } else {
            self.push_menu(
                message,
                vec![MenuItem::new("dismiss", "Dismiss", Action::Close)],
            );
        }
    }

    pub fn text_input_active(&self) -> bool {
        self.active_editor().is_some() || self.editor_menu_target().is_some()
    }

    fn suspend(&mut self) {
        self.cancel_effects();
        self.caret.stop();
        self.tooltip.hide();
        self.pointer.cancel_drag();
        self.pointer.press = None;
        self.pointer.origin = None;
    }
}

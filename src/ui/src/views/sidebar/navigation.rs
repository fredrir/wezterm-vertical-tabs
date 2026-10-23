use crate::SidebarUi;
use crate::intent::UiIntent;
use crate::views::sidebar::Slot;
use vtabs_core::{Intent, Model};

impl SidebarUi {
    pub fn activate_index(&mut self, model: &Model, index: isize, intents: &mut Vec<UiIntent>) {
        let Some(slot) = self.settings_slot(model) else {
            self.hide_settings();
            intents.push(UiIntent::Domain(Intent::ActivateIndex(index)));
            return;
        };
        let total = model.visible_ids().len() as isize + 1;
        let position = if index < 0 { total + index } else { index };
        if position == slot {
            self.open_settings();
        } else {
            self.hide_settings();
            intents.push(UiIntent::Domain(Intent::ActivateIndex(
                position - isize::from(position > slot),
            )));
        }
    }

    pub fn activate_relative(
        &mut self,
        model: &Model,
        delta: isize,
        wrap: bool,
        intents: &mut Vec<UiIntent>,
    ) {
        let Some(slot) = self.settings_slot(model) else {
            intents.push(UiIntent::Domain(Intent::ActivateRelative { delta, wrap }));
            return;
        };
        let count = model.visible_ids().len() as isize;
        let current = if self.settings.open {
            slot
        } else {
            let at = model
                .selected_tab
                .and_then(|id| model.visible_ids().iter().position(|tab| *tab == id))
                .unwrap_or(0) as isize;
            at + isize::from(at >= slot)
        };
        let next = if wrap {
            (current + delta).rem_euclid(count + 1)
        } else {
            (current + delta).clamp(0, count)
        };
        self.activate_index(model, next, intents);
    }

    /// Host tab moves reach the settings tab through the same order as the sidebar.
    pub fn move_settings(&mut self, model: &Model, index: usize) {
        self.prepare(model, self.frame.now);
        let mut slots = self.sidebar.slots(model, self.settings.listed);
        if !slots.contains(&Slot::Settings) {
            return;
        }
        slots.retain(|slot| *slot != Slot::Settings);
        slots.insert(index.min(slots.len()), Slot::Settings);
        self.sidebar.settle_settings(&slots);
        self.sidebar.reveal_settings = true;
        self.frame.dirty = true;
    }

    pub fn move_settings_relative(&mut self, model: &Model, delta: isize) {
        if let Some(slot) = self.settings_slot(model) {
            self.move_settings(model, slot.saturating_add(delta).max(0) as usize);
        }
    }

    fn settings_slot(&mut self, model: &Model) -> Option<isize> {
        self.prepare(model, self.frame.now);
        self.sidebar.ensure_rows(model, self.settings.listed);
        self.sidebar
            .settings_place
            .filter(|_| self.settings.listed)
            .map(|place| place.slot as isize)
    }
}

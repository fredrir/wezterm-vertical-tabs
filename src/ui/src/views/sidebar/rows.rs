use crate::components::list;
use crate::views::sidebar::Sidebar;
use std::collections::HashMap;
use vtabs_core::{Model, TabId};

pub(crate) const GROUP_GAP: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarRow {
    Tab {
        id: TabId,
        number: usize,
    },
    Folder {
        index: usize,
        count: usize,
    },
    Gap,
    NewTab,
    Settings {
        number: usize,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SettingsPlace {
    pub anchor: Option<TabId>,
    pub slot: usize,
}

impl Sidebar {
    fn place_settings(&mut self, model: &Model) -> usize {
        let visible = model.visible_ids();
        let first_open = visible
            .iter()
            .position(|id| model.tabs.get(id).is_some_and(|tab| !tab.pinned))
            .unwrap_or(visible.len());
        let slot = match self.settings_place {
            None => visible.len(),
            Some(SettingsPlace { anchor: None, .. }) => 0,
            Some(SettingsPlace {
                anchor: Some(anchor),
                slot,
            }) => visible
                .iter()
                .position(|id| *id == anchor)
                .map_or(slot.saturating_sub(1), |at| at + 1),
        }
        .clamp(first_open, visible.len());
        self.settings_place = Some(SettingsPlace {
            anchor: slot.checked_sub(1).map(|at| visible[at]),
            slot,
        });
        slot
    }

    pub fn ensure_rows(&mut self, model: &Model, settings_listed: bool) {
        if self.rows_revision == Some((model.revision, settings_listed)) {
            return;
        }
        let slot = settings_listed.then(|| self.place_settings(model));
        let number = |index: usize| index + 1 + usize::from(slot.is_some_and(|slot| index >= slot));
        self.rows.clear();
        self.rows
            .reserve(model.visible_ids().len() + model.folders.len() + 2);
        let mut folders = HashMap::with_capacity(model.selected_folders().count());
        folders.extend(
            model
                .selected_folders()
                .map(|folder| (folder.id.as_str(), (0usize, 0..0))),
        );
        for tab in model.tabs.values() {
            if let Some(folder) = tab.folder_id.as_deref().and_then(|id| folders.get_mut(id)) {
                folder.0 += 1;
            }
        }
        let collapsed = model
            .spaces
            .iter()
            .any(|space| space.id == model.selected_space && space.collapsed);
        for (index, id) in model.visible_ids().iter().enumerate() {
            let Some(tab) = model.tabs.get(id) else {
                continue;
            };
            let row = SidebarRow::Tab {
                id: *id,
                number: number(index),
            };
            if tab.pinned && tab.folder_id.is_none() && !collapsed {
                self.rows.push(row);
            }
            if let Some((_, range)) = tab.folder_id.as_deref().and_then(|id| folders.get_mut(id)) {
                if range.start == range.end {
                    range.start = index;
                }
                range.end = index + 1;
            }
        }
        for (index, folder) in model.folders.iter().enumerate() {
            if folder.space_id != model.selected_space || collapsed {
                continue;
            }
            let (count, range) = folders.remove(folder.id.as_str()).unwrap_or_default();
            self.rows.push(SidebarRow::Folder { index, count });
            if !folder.collapsed {
                self.rows.extend(range.map(|index| SidebarRow::Tab {
                    id: model.visible_ids()[index],
                    number: number(index),
                }));
            }
        }
        if !self.rows.is_empty() {
            self.rows.push(SidebarRow::Gap);
        }
        self.rows.push(SidebarRow::NewTab);
        let settings = slot.map(|slot| SidebarRow::Settings { number: slot + 1 });
        for (index, id) in model.visible_ids().iter().enumerate() {
            if slot == Some(index) {
                self.rows.extend(settings);
            }
            if model.tabs.get(id).is_some_and(|tab| !tab.pinned) {
                self.rows.push(SidebarRow::Tab {
                    id: *id,
                    number: number(index),
                });
            }
        }
        if slot == Some(model.visible_ids().len()) {
            self.rows.extend(settings);
        }
        self.rows_revision = Some((model.revision, settings_listed));
    }

    pub fn row_height(&self, model: &Model) -> u16 {
        if self.list.width < 12 || !model.settings.cards || self.list.height < 4 {
            1
        } else if model.settings.show_metadata {
            3
        } else {
            2
        }
    }

    pub(crate) fn row_span(&self, model: &Model, row: SidebarRow) -> u16 {
        if row == SidebarRow::Gap {
            GROUP_GAP
        } else {
            self.row_height(model)
        }
    }

    pub fn rows_fitting(&self, model: &Model, start: usize) -> usize {
        let mut used = 0;
        self.rows
            .iter()
            .skip(start)
            .take_while(|row| {
                used += self.row_span(model, **row);
                used <= self.list.height
            })
            .count()
            .max(1)
    }

    pub fn visible_rows(&self, model: &Model) -> usize {
        self.rows_fitting(model, self.scroll)
    }

    pub(crate) fn max_scroll(&self, model: &Model) -> usize {
        let mut used = 0;
        let hidden = self
            .rows
            .iter()
            .rev()
            .take_while(|row| {
                used += self.row_span(model, **row);
                used <= self.list.height
            })
            .count()
            .max(1);
        self.rows.len().saturating_sub(hidden)
    }

    pub fn reveal_row(&mut self, model: &Model, at: usize) {
        self.scroll = list::reveal(self.scroll, at, |start| {
            start + self.rows_fitting(model, start)
        });
    }

    pub fn ensure_tab_visible(&mut self, model: &Model, id: TabId, settings_listed: bool) {
        self.ensure_rows(model, settings_listed);
        let collapsed_folder = if let Some(tab) = model.tabs.get(&id)
            && let Some(folder) = &tab.folder_id
        {
            model
                .folders
                .iter()
                .position(|f| &f.id == folder && f.collapsed)
        } else {
            None
        };
        if let Some(at) = self.rows.iter().position(|row| match row {
            SidebarRow::Folder { index, .. } => collapsed_folder == Some(*index),
            SidebarRow::Tab { id: tab, .. } => collapsed_folder.is_none() && *tab == id,
            SidebarRow::Gap | SidebarRow::NewTab | SidebarRow::Settings { .. } => false,
        }) {
            self.reveal_row(model, at);
        }
    }
}

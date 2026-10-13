use super::Model;
use crate::{HostCommand, SpaceId, TabId};
use std::collections::BTreeMap;

impl Model {
    pub(super) fn project_space(&self, space: &str, include_hidden: bool, out: &mut Vec<TabId>) {
        let mut grouped: BTreeMap<&str, Vec<TabId>> = BTreeMap::new();
        let mut normal = Vec::new();
        for id in &self.order {
            let Some(tab) = self.tabs.get(id) else {
                continue;
            };
            if tab.space_id != space || (!include_hidden && self.hidden.contains(id)) {
                continue;
            }
            if let Some(folder) = tab.folder_id.as_deref() {
                grouped.entry(folder).or_default().push(*id);
            } else if tab.pinned {
                out.push(*id);
            } else {
                normal.push(*id);
            }
        }
        for folder in &self.folders {
            if let Some(tabs) = grouped.remove(folder.id.as_str()) {
                out.extend(tabs);
            }
        }
        out.extend(normal);
    }
    pub(super) fn rebuild_visible(&mut self) {
        self.clean_folder_membership();
        self.projection_revision = self.projection_revision.wrapping_add(1);
        let mut visible = std::mem::take(&mut self.visible);
        visible.clear();
        self.project_space(&self.selected_space, false, &mut visible);
        self.visible = visible;
        if self
            .selected_tab
            .is_none_or(|id| !self.visible.contains(&id))
        {
            self.selected_tab = self.visible.first().copied();
            if let Some(id) = self.selected_tab {
                let tab = self.tabs.get_mut(&id).unwrap();
                tab.unread = false;
                tab.bell = false;
            }
        }
    }
    fn clean_folder_membership(&mut self) {
        let folders = self
            .folders
            .iter()
            .map(|folder| (folder.id.as_str(), folder.space_id.as_str()))
            .collect::<BTreeMap<_, _>>();
        for tab in self.tabs.values_mut() {
            if let Some(id) = tab.folder_id.as_deref() {
                if folders.get(id).copied() == Some(tab.space_id.as_str()) {
                    tab.pinned = true;
                } else {
                    tab.folder_id = None;
                }
            }
        }
    }
    pub(super) fn activate(&mut self, id: TabId) {
        let was_hidden = self.hidden.remove(&id);
        if let Some(tab) = self.tabs.get_mut(&id) {
            let space_changed = self.selected_space != tab.space_id;
            self.selected_space = tab.space_id.clone();
            tab.unread = false;
            tab.bell = false;
            self.selected_tab = Some(id);
            self.last_tabs.insert(self.selected_space.clone(), id);
            self.mru.retain(|t| *t != id);
            self.mru.push_front(id);
            if space_changed || was_hidden {
                self.rebuild_visible();
            }
        }
    }
    pub(super) fn select_space(&mut self, id: SpaceId) {
        self.selected_space = id;
        self.rebuild_visible();
        let last = self
            .last_tabs
            .get(&self.selected_space)
            .copied()
            .filter(|id| self.visible.contains(id));
        self.selected_tab = last.or_else(|| self.visible.first().copied());
        if let Some(id) = self.selected_tab {
            self.activate(id);
        }
    }
    pub(super) fn select_space_and_activate(
        &mut self,
        id: SpaceId,
        commands: &mut Vec<HostCommand>,
    ) {
        self.select_space(id);
        commands.extend(self.selected_tab.map(HostCommand::Activate));
    }
}

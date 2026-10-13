use super::Change;
use crate::{Error, HostCommand, Model, SpaceId, TabId};
use std::collections::BTreeSet;

impl Model {
    pub(super) fn activate_tab(
        &mut self,
        id: TabId,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        let mut change = Change::Memory;
        if let Some(folder_id) = self.tabs[&id].folder_id.as_deref()
            && let Some(folder) = self
                .folders
                .iter_mut()
                .find(|folder| folder.id == folder_id)
            && folder.collapsed
        {
            folder.collapsed = false;
            change = Change::Durable;
        }
        let tab = &self.tabs[&id];
        if (tab.pinned || tab.folder_id.is_some())
            && let Some(space) = self
                .spaces
                .iter_mut()
                .find(|space| space.id == tab.space_id && space.collapsed)
        {
            space.collapsed = false;
            change = Change::Durable;
        }
        self.activate(id);
        commands.push(HostCommand::Activate(id));
        Ok(change)
    }
    pub(super) fn index_target(&self, index: isize) -> Option<TabId> {
        let n = self.visible.len() as isize;
        let index = if index < 0 { n + index } else { index };
        (0..n)
            .contains(&index)
            .then(|| self.visible[index as usize])
    }
    pub(super) fn relative_target(&self, delta: isize, wrap: bool) -> Option<TabId> {
        let n = self.visible.len() as isize;
        if n == 0 {
            return None;
        }
        let at = self
            .selected_tab
            .and_then(|id| self.visible.iter().position(|v| *v == id))
            .unwrap_or(0) as isize;
        let i = at.saturating_add(delta);
        let i = if wrap {
            i.rem_euclid(n)
        } else {
            i.clamp(0, n - 1)
        };
        Some(self.visible[i as usize])
    }
    pub(super) fn last_target(&self) -> Option<TabId> {
        self.mru
            .iter()
            .find(|id| Some(**id) != self.selected_tab && self.visible.contains(id))
            .copied()
    }
    pub(super) fn new_tab(&self, commands: &mut Vec<HostCommand>) -> Change {
        commands.push(HostCommand::Spawn {
            space_id: self.selected_space.clone(),
            launch: self.default_launch(),
            folder_id: None,
        });
        Change::None
    }
    pub(super) fn close_tab(
        &self,
        id: TabId,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        commands.push(HostCommand::Close(id));
        Ok(Change::None)
    }
    pub(super) fn close_others(
        &self,
        id: TabId,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        commands.extend(
            self.visible
                .iter()
                .filter(|other| **other != id && !self.tabs[other].pinned)
                .map(|id| HostCommand::Close(*id)),
        );
        Ok(Change::None)
    }
    pub(super) fn close_below(
        &self,
        id: TabId,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        let below = self
            .visible
            .iter()
            .position(|other| *other == id)
            .map_or(&[][..], |at| &self.visible[at + 1..]);
        commands.extend(
            below
                .iter()
                .filter(|other| !self.tabs[other].pinned)
                .map(|id| HostCommand::Close(*id)),
        );
        Ok(Change::None)
    }
    pub(super) fn rename_tab(
        &mut self,
        id: TabId,
        title: String,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        if title.chars().count() > 512 || title.chars().any(char::is_control) {
            return Err(Error("Invalid tab title".into()));
        }
        self.tabs.get_mut(&id).unwrap().title_override =
            (!title.is_empty()).then_some(title.clone());
        commands.push(HostCommand::Rename { id, title });
        Ok(Change::Memory)
    }
    pub(super) fn pin_tab(&mut self, id: TabId, pinned: bool) -> Result<Change, Error> {
        self.require_tab(id)?;
        let tab = self.tabs.get_mut(&id).unwrap();
        tab.pinned = pinned;
        if !pinned {
            tab.folder_id = None;
        }
        Ok(Change::Durable)
    }
    pub(super) fn move_tab(
        &mut self,
        id: TabId,
        index: usize,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        let Some(at) = self.visible.iter().position(|v| *v == id) else {
            return Err(Error("Cannot reorder a hidden tab".into()));
        };
        let pinned = self.tabs[&id].pinned;
        let folder = self.tabs[&id].folder_id.clone();
        let mut visible = self.visible.clone();
        visible.remove(at);
        let peers = visible
            .iter()
            .enumerate()
            .filter_map(|(at, id)| {
                let tab = &self.tabs[id];
                (tab.pinned == pinned && tab.folder_id == folder).then_some(at)
            })
            .collect::<Vec<_>>();
        let start = peers.first().copied().unwrap_or(at);
        let end = peers.last().map_or(start, |at| at + 1);
        let index = index.clamp(start, end);
        visible.insert(index, id);
        let visible_set = self.visible.iter().copied().collect::<BTreeSet<_>>();
        let mut reordered = visible.iter();
        for entry in &mut self.order {
            if visible_set.contains(entry) {
                *entry = *reordered.next().unwrap();
            }
        }
        commands.push(HostCommand::Reorder {
            visible_order: visible,
        });
        Ok(Change::Durable)
    }
    pub(super) fn assign_tab(
        &mut self,
        id: TabId,
        space_id: SpaceId,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        self.require_space(&space_id)?;
        let tab = self.tabs.get_mut(&id).unwrap();
        if tab.space_id != space_id {
            tab.folder_id = None;
        }
        tab.space_id = space_id;
        tab.manual_assignment = true;
        if self.selected_tab == Some(id) {
            self.select_space_and_activate(self.selected_space.clone(), commands);
        }
        Ok(Change::Durable)
    }
    pub(super) fn return_to_auto(&mut self, id: TabId) -> Result<Change, Error> {
        self.require_tab(id)?;
        let tab = self.tabs.get_mut(&id).unwrap();
        tab.manual_assignment = false;
        tab.folder_id = None;
        tab.space_id = self.spaces[0].id.clone();
        self.hook_routes.remove(&id);
        self.route_tab(id);
        Ok(Change::Durable)
    }
    pub(super) fn move_to_new_window(
        &self,
        id: TabId,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(id)?;
        commands.push(HostCommand::MoveTabToNewWindow(id));
        Ok(Change::None)
    }
}

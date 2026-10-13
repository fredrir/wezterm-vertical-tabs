use super::Model;
use crate::{Error, HostTab, Space, Tab, TabId, routing};
use std::collections::BTreeSet;

impl Model {
    /// Only a changed active ID follows a space, so an empty space never shows a hidden tab.
    pub fn reconcile(
        &mut self,
        incoming: Vec<HostTab>,
        active: Option<TabId>,
        follow_active: bool,
    ) -> Result<bool, Error> {
        let topology_changed = !incoming
            .iter()
            .map(|tab| tab.id)
            .eq(self.order.iter().copied());
        let mut ids = BTreeSet::new();
        if topology_changed {
            for tab in &incoming {
                if !ids.insert(tab.id) {
                    return Err(Error("Duplicate host tab identity".into()));
                }
            }
        }
        let before = self.revision;
        let old_selected = self.selected_tab;
        let old_position = if topology_changed {
            self.visible
                .iter()
                .position(|id| Some(*id) == old_selected)
                .unwrap_or(0)
        } else {
            0
        };
        let mut changed = topology_changed;
        let mut projection_changed = topology_changed;
        if topology_changed {
            let mut removed = false;
            for id in self.order.iter().copied().filter(|id| !ids.contains(id)) {
                removed = true;
                self.tabs.remove(&id);
                self.hook_routes.remove(&id);
                self.hidden.remove(&id);
            }
            if removed {
                self.mru.retain(|id| ids.contains(id));
                self.last_tabs.retain(|_, id| ids.contains(id));
            }
            self.order.clear();
            self.order.extend(incoming.iter().map(|tab| tab.id));
        }
        for mut tab in incoming {
            if Some(tab.id) == self.selected_tab {
                tab.unread = false;
                tab.bell = false;
            }
            let id = tab.id;
            if let Some(old) = self.tabs.get_mut(&id) {
                if !old.reconcile_host_metadata(tab) {
                    continue;
                }
                self.hook_routes.remove(&id);
            } else {
                self.insert_tab(tab.into());
                projection_changed = true;
            }
            projection_changed |= self.route_tab(id);
            changed = true;
        }
        if projection_changed {
            self.rebuild_visible();
        }
        let selected_closed = old_selected.is_some_and(|id| !self.tabs.contains_key(&id));
        if follow_active
            && !selected_closed
            && let Some(id) = active.filter(|id| self.tabs.contains_key(id))
            && (self.selected_tab != Some(id) || self.tabs[&id].space_id != self.selected_space)
        {
            self.activate(id);
            changed = true;
        }
        if selected_closed {
            self.selected_tab = self
                .mru
                .iter()
                .find(|id| self.visible.contains(id))
                .or_else(|| {
                    self.visible
                        .get(old_position.min(self.visible.len().saturating_sub(1)))
                })
                .copied();
            if let Some(id) = self.selected_tab {
                self.activate(id);
            }
            changed = true;
        }
        if changed {
            self.touch();
        }
        Ok(self.revision != before)
    }

    /// Initializes an empty window from a trusted transfer, never from a host snapshot.
    pub fn import_transferred_tab(&mut self, tab: Tab) -> Result<(), Error> {
        if !self.tabs.is_empty() {
            return Err(Error(
                "Window transfer must initialize an empty window".into(),
            ));
        }
        let id = tab.id;
        self.insert_tab(tab);
        self.order.push(id);
        self.route_tab(id);
        self.rebuild_visible();
        self.activate(id);
        self.touch();
        Ok(())
    }

    pub fn restore_tab_membership(
        &mut self,
        id: TabId,
        space: &str,
        manual: bool,
        pinned: bool,
        folder_id: Option<&str>,
    ) -> Result<(), Error> {
        self.require_tab(id)?;
        if self.require_space(space).is_err() {
            return Ok(());
        }
        let folder_id = folder_id.filter(|id| {
            self.folders
                .iter()
                .any(|folder| folder.id == *id && folder.space_id == space)
        });
        let pinned = pinned || folder_id.is_some();
        let tab = self.tabs.get_mut(&id).unwrap();
        if tab.space_id == space
            && tab.manual_assignment == manual
            && tab.pinned == pinned
            && tab.folder_id.as_deref() == folder_id
        {
            return Ok(());
        }
        tab.space_id = space.into();
        tab.manual_assignment = manual;
        tab.pinned = pinned;
        tab.folder_id = folder_id.map(str::to_owned);
        self.rebuild_visible();
        self.touch();
        Ok(())
    }

    pub fn assign_spawn_membership(&mut self, id: TabId, space: &str) -> Result<(), Error> {
        self.require_tab(id)?;
        self.require_space(space)?;
        self.tabs.get_mut(&id).unwrap().space_id = space.into();
        self.route_tab(id);
        self.rebuild_visible();
        self.touch();
        Ok(())
    }

    fn insert_tab(&mut self, mut tab: Tab) {
        if tab.space_id.is_empty() {
            tab.space_id = self.selected_space.clone();
        }
        self.tabs.insert(tab.id, tab);
    }

    pub(super) fn route_tab(&mut self, id: TabId) -> bool {
        let Some(tab) = self.tabs.get(&id) else {
            return false;
        };
        if tab.manual_assignment && self.spaces.iter().any(|s| s.id == tab.space_id) {
            return false;
        }
        let route = self
            .hook_routes
            .get(&id)
            .and_then(Clone::clone)
            .or_else(|| routing::route(&self.spaces, &self.templates, tab));
        let selected = route.unwrap_or_else(|| {
            if !tab.space_id.is_empty() && self.spaces.iter().any(|s| s.id == tab.space_id) {
                tab.space_id.clone()
            } else {
                self.selected_space.clone()
            }
        });
        if !self.spaces.iter().any(|s| s.id == selected) {
            let mut space = Space::new(&selected, &selected);
            if let Some(template) = self
                .templates
                .iter()
                .find(|t| routing::expand(&t.id, tab).as_deref() == Some(&selected))
            {
                space.name =
                    routing::expand(&template.name, tab).unwrap_or_else(|| selected.clone());
                space.icon = template.icon.clone();
                space.accent = template.accent.clone();
                space.template = Some(template.id.clone());
            }
            self.spaces.push(space);
        }
        let tab = self.tabs.get_mut(&id).unwrap();
        if tab.space_id == selected {
            false
        } else {
            tab.space_id = selected;
            true
        }
    }

    pub(super) fn reroute(&mut self) {
        for id in self.order.clone() {
            if self
                .tabs
                .get(&id)
                .is_some_and(|t| !self.spaces.iter().any(|s| s.id == t.space_id))
            {
                let t = self.tabs.get_mut(&id).unwrap();
                t.manual_assignment = false;
                t.space_id.clear();
            }
            self.route_tab(id);
        }
        self.rebuild_visible();
    }
}

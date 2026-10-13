use super::Change;
use crate::{
    Error, HostCommand, MAX_SPACES, Model, Space, SpaceId,
    model::{clean_name, next_id},
    routing::{self, RoutingRule},
    settings,
};

impl Model {
    pub(super) fn switch_space(
        &mut self,
        id: SpaceId,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_space(&id)?;
        if self.selected_space == id {
            return Ok(Change::None);
        }
        self.select_space_and_activate(id, commands);
        Ok(Change::Durable)
    }
    pub(super) fn create_space(&mut self, name: String) -> Result<Change, Error> {
        let name = clean_name(name)?;
        if self.spaces.len() >= MAX_SPACES {
            return Err(Error(format!("At most {MAX_SPACES} spaces are supported")));
        }
        let id = next_id(&self.space_namespace, "space", &mut self.next_space, |id| {
            self.spaces.iter().any(|s| s.id == id)
        });
        self.spaces.push(Space::new(&id, name));
        self.select_space(id);
        Ok(Change::Durable)
    }
    pub(super) fn rename_space(&mut self, id: &str, name: String) -> Result<Change, Error> {
        let name = clean_name(name)?;
        self.require_space(id)?;
        self.spaces.iter_mut().find(|s| s.id == id).unwrap().name = name;
        Ok(Change::Durable)
    }
    pub(super) fn edit_space(
        &mut self,
        id: &str,
        icon: String,
        accent: Option<String>,
        rules: Vec<RoutingRule>,
    ) -> Result<Change, Error> {
        self.require_space(id)?;
        if icon.chars().count() > 16 {
            return Err(Error("Icon exceeds 16 characters".into()));
        }
        if accent.as_deref().is_some_and(|c| !settings::valid_color(c)) {
            return Err(Error("Accent must be #RRGGBB".into()));
        }
        routing::validate_rules(&rules).map_err(Error)?;
        let space = self.spaces.iter_mut().find(|s| s.id == id).unwrap();
        space.icon = icon;
        space.accent = accent;
        space.rules = rules;
        self.reroute();
        Ok(Change::Durable)
    }
    pub(super) fn delete_space(
        &mut self,
        id: SpaceId,
        destination: Option<SpaceId>,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_space(&id)?;
        if self.spaces.len() == 1 {
            return Err(Error("The final space cannot be deleted".into()));
        }
        let occupied = self.tabs.values().any(|t| t.space_id == id);
        if occupied && destination.is_none() {
            return Err(Error("Choose a destination for the space's tabs".into()));
        }
        if let Some(dest) = &destination {
            self.require_space(dest)?;
            if dest == &id {
                return Err(Error("Destination must be another space".into()));
            }
        }
        let fallback = destination
            .unwrap_or_else(|| self.spaces.iter().find(|s| s.id != id).unwrap().id.clone());
        for tab in self.tabs.values_mut().filter(|t| t.space_id == id) {
            tab.space_id = fallback.clone();
            tab.manual_assignment = true;
            tab.folder_id = None;
        }
        self.spaces.retain(|s| s.id != id);
        self.folders.retain(|folder| folder.space_id != id);
        self.last_tabs.remove(&id);
        if self.selected_space == id {
            self.select_space_and_activate(fallback, commands);
        }
        Ok(Change::Durable)
    }
    pub(super) fn move_space(&mut self, id: &str, index: usize) -> Result<Change, Error> {
        self.require_space(id)?;
        let at = self.spaces.iter().position(|s| s.id == id).unwrap();
        let space = self.spaces.remove(at);
        self.spaces.insert(index.min(self.spaces.len()), space);
        Ok(Change::Durable)
    }
    pub(super) fn toggle_space(&mut self, id: &str) -> Result<Change, Error> {
        self.require_space(id)?;
        if let Some(space) = self.spaces.iter_mut().find(|space| space.id == id) {
            space.collapsed = !space.collapsed;
        }
        Ok(Change::Durable)
    }
}

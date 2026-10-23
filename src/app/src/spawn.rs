use crate::{
    Command, Update, WindowApp,
    core::{Error, HostCommand, Intent, TabId},
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpawnToken {
    pub request_id: u64,
    pub space_id: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    selection_epoch: u64,
}

impl WindowApp {
    pub fn reserve_spawn(&mut self) -> SpawnToken {
        self.reserve(self.model.selected_space.clone(), None)
    }
    pub fn reserve_spawn_in(&mut self, space_id: &str) -> Result<SpawnToken, Error> {
        if !self.model.spaces.iter().any(|s| s.id == space_id) {
            return Err(Error("Spawn space no longer exists".into()));
        }
        Ok(self.reserve(space_id.into(), None))
    }
    pub fn reserve_spawn_in_folder(&mut self, folder_id: &str) -> Result<SpawnToken, Error> {
        let space = self
            .model
            .folders
            .iter()
            .find(|folder| folder.id == folder_id)
            .map(|folder| folder.space_id.clone())
            .ok_or_else(|| Error("Spawn folder no longer exists".into()))?;
        Ok(self.reserve(space, Some(folder_id.into())))
    }
    fn reserve(&mut self, space_id: String, folder_id: Option<String>) -> SpawnToken {
        self.spawn_sequence = self.spawn_sequence.wrapping_add(1);
        let token = SpawnToken {
            request_id: self.spawn_sequence,
            space_id,
            folder_id,
            selection_epoch: self.selection_epoch,
        };
        self.pending_spawns.insert(token.request_id, token.clone());
        token
    }
    pub fn cancel_spawn(&mut self, token: &SpawnToken) {
        if self.pending_spawns.get(&token.request_id) == Some(token) {
            self.pending_spawns.remove(&token.request_id);
        }
    }
    pub fn spawn_completed_with_token(
        &mut self,
        tab_id: TabId,
        token: SpawnToken,
    ) -> Result<Update, Error> {
        if self.pending_spawns.get(&token.request_id) != Some(&token) || self.teardown {
            return Ok(Update::default());
        }
        self.pending_spawns.remove(&token.request_id);
        let space = if self.model.spaces.iter().any(|s| s.id == token.space_id) {
            token.space_id
        } else {
            self.model.selected_space.clone()
        };
        self.model.assign_spawn_membership(tab_id, &space)?;
        if let Some(folder_id) = token.folder_id
            && self
                .model
                .folders
                .iter()
                .any(|folder| folder.id == folder_id && folder.space_id == space)
        {
            self.model.dispatch(Intent::AssignFolder {
                tab_id,
                folder_id: Some(folder_id),
            })?;
        }
        self.storage.observe(&self.model, self.now);
        if token.selection_epoch == self.selection_epoch {
            return self.dispatch(Intent::ActivateTab(tab_id));
        }
        let mut out = Update::default();
        if let Some(id) = self
            .model
            .selected_tab
            .filter(|id| Some(*id) != self.host_active)
        {
            out.commands.push(Command::Host(HostCommand::Activate(id)));
        }
        Ok(out)
    }
}

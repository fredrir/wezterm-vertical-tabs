use super::Change;
use crate::{
    Error, Folder, HostCommand, MAX_FOLDERS, Model, TabId,
    model::{clean_name, next_id},
};

impl Model {
    pub(super) fn create_folder(&mut self, name: String) -> Result<Change, Error> {
        let name = clean_name(name)?;
        if self.folders.len() >= MAX_FOLDERS {
            return Err(Error(format!(
                "At most {MAX_FOLDERS} folders are supported"
            )));
        }
        let id = next_id(
            &self.space_namespace,
            "folder",
            &mut self.next_folder,
            |id| self.folders.iter().any(|folder| folder.id == id),
        );
        self.folders.push(Folder {
            id,
            name,
            space_id: self.selected_space.clone(),
            collapsed: false,
        });
        Ok(Change::Durable)
    }
    pub(super) fn rename_folder(&mut self, id: &str, name: String) -> Result<Change, Error> {
        let name = clean_name(name)?;
        let at = self.require_folder(id)?;
        if self.folders[at].name == name {
            return Ok(Change::None);
        }
        self.folders[at].name = name;
        Ok(Change::Durable)
    }
    pub(super) fn toggle_folder(&mut self, id: &str) -> Result<Change, Error> {
        let at = self.require_folder(id)?;
        self.folders[at].collapsed = !self.folders[at].collapsed;
        Ok(Change::Durable)
    }
    pub(super) fn delete_folder(&mut self, id: &str) -> Result<Change, Error> {
        let at = self.require_folder(id)?;
        self.folders.remove(at);
        for tab in self.tabs.values_mut() {
            if tab.folder_id.as_deref() == Some(id) {
                tab.folder_id = None;
            }
        }
        Ok(Change::Durable)
    }
    pub(super) fn assign_folder(
        &mut self,
        tab_id: TabId,
        folder_id: Option<String>,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        self.require_tab(tab_id)?;
        let space = folder_id
            .as_deref()
            .map(|id| {
                self.require_folder(id)
                    .map(|at| self.folders[at].space_id.clone())
            })
            .transpose()?;
        let tab = self.tabs.get_mut(&tab_id).unwrap();
        if tab.folder_id == folder_id {
            return Ok(Change::None);
        }
        if let Some(space) = space {
            tab.space_id = space;
            tab.manual_assignment = true;
            tab.pinned = true;
        }
        tab.folder_id = folder_id;
        if self.selected_tab == Some(tab_id) && tab.space_id != self.selected_space {
            self.activate_tab(tab_id, commands)?;
        }
        Ok(Change::Durable)
    }
    pub(super) fn move_folder(&mut self, id: &str, index: usize) -> Result<Change, Error> {
        let at = self.require_folder(id)?;
        let space = self.folders[at].space_id.clone();
        let positions = self
            .folders
            .iter()
            .enumerate()
            .filter_map(|(at, folder)| (folder.space_id == space).then_some(at))
            .collect::<Vec<_>>();
        let mut folders = positions
            .iter()
            .map(|at| self.folders[*at].clone())
            .collect::<Vec<_>>();
        let from = folders.iter().position(|folder| folder.id == id).unwrap();
        let to = index.min(folders.len().saturating_sub(1));
        if from == to {
            return Ok(Change::None);
        }
        let folder = folders.remove(from);
        folders.insert(to, folder);
        for (at, folder) in positions.into_iter().zip(folders) {
            self.folders[at] = folder;
        }
        Ok(Change::Durable)
    }
    pub(super) fn new_tab_in_folder(
        &self,
        id: String,
        commands: &mut Vec<HostCommand>,
    ) -> Result<Change, Error> {
        let at = self.require_folder(&id)?;
        commands.push(HostCommand::Spawn {
            space_id: self.folders[at].space_id.clone(),
            launch: self.default_launch(),
            folder_id: Some(id),
        });
        Ok(Change::None)
    }
}

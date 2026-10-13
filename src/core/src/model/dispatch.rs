mod folders;
mod settings;
mod spaces;
mod tabs;

use super::Model;
use crate::{Error, HostCommand, Intent, TabId, Transition};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    None,
    Memory,
    Durable,
}

impl Model {
    pub fn dispatch(&mut self, intent: Intent) -> Result<Transition, Error> {
        let rebuild_projection = !matches!(
            &intent,
            Intent::ActivateTab(_)
                | Intent::RenameTab { .. }
                | Intent::RenameFolder { .. }
                | Intent::ToggleFolder(_)
                | Intent::ToggleSpace(_)
                | Intent::SetSetting { .. }
                | Intent::ResetSetting(_)
                | Intent::ResetSettings
                | Intent::SetRail(_)
        );
        let mut commands = Vec::new();
        let change = match intent {
            Intent::SelectSpace(id) => self.switch_space(id, &mut commands)?,
            Intent::CreateSpace { name } => self.create_space(name)?,
            Intent::RenameSpace { id, name } => self.rename_space(&id, name)?,
            Intent::EditSpace {
                id,
                icon,
                accent,
                rules,
            } => self.edit_space(&id, icon, accent, rules)?,
            Intent::DeleteSpace { id, destination } => {
                self.delete_space(id, destination, &mut commands)?
            }
            Intent::MoveSpace { id, index } => self.move_space(&id, index)?,
            Intent::ToggleSpace(id) => self.toggle_space(&id)?,
            Intent::CreateFolder { name } => self.create_folder(name)?,
            Intent::RenameFolder { id, name } => self.rename_folder(&id, name)?,
            Intent::ToggleFolder(id) => self.toggle_folder(&id)?,
            Intent::DeleteFolder(id) => self.delete_folder(&id)?,
            Intent::AssignFolder { tab_id, folder_id } => {
                self.assign_folder(tab_id, folder_id, &mut commands)?
            }
            Intent::MoveFolder { id, index } => self.move_folder(&id, index)?,
            Intent::ActivateTab(id) => self.activate_tab(id, &mut commands)?,
            Intent::ActivateIndex(index) => return self.activate_target(self.index_target(index)),
            Intent::ActivateRelative { delta, wrap } => {
                return self.activate_target(self.relative_target(delta, wrap));
            }
            Intent::ActivateLast => return self.activate_target(self.last_target()),
            Intent::NewTab => self.new_tab(&mut commands),
            Intent::NewTabInFolder(id) => self.new_tab_in_folder(id, &mut commands)?,
            Intent::CloseTab(id) => self.close_tab(id, &mut commands)?,
            Intent::CloseOthers(id) => self.close_others(id, &mut commands)?,
            Intent::CloseBelow(id) => self.close_below(id, &mut commands)?,
            Intent::RenameTab { id, title } => self.rename_tab(id, title, &mut commands)?,
            Intent::PinTab { id, pinned } => self.pin_tab(id, pinned)?,
            Intent::MoveTab { id, index } => self.move_tab(id, index, &mut commands)?,
            Intent::AssignTab { id, space_id } => self.assign_tab(id, space_id, &mut commands)?,
            Intent::ReturnToAuto(id) => self.return_to_auto(id)?,
            Intent::MoveTabToNewWindow(id) => self.move_to_new_window(id, &mut commands)?,
            Intent::SetSetting { key, value } => self.set_setting(key, value)?,
            Intent::ResetSetting(key) => self.reset_setting(key)?,
            Intent::ResetSettings => self.reset_settings()?,
            Intent::SetRail(rail) => self.set_rail(rail),
            Intent::CustomAction(action) => {
                commands.push(HostCommand::CustomAction(action));
                Change::None
            }
        };
        if change != Change::None {
            if rebuild_projection {
                self.rebuild_visible();
            }
            self.touch();
        }
        Ok(Transition {
            commands,
            durable_changed: change == Change::Durable,
        })
    }

    fn activate_target(&mut self, target: Option<TabId>) -> Result<Transition, Error> {
        match target {
            Some(id) => self.dispatch(Intent::ActivateTab(id)),
            None => Ok(Transition::default()),
        }
    }
}

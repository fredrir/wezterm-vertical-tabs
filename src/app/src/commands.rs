use crate::{
    Command, Update, WindowApp,
    core::{Error, HostCommand, PaneId, TabId},
    ui::{HostAction, UiIntent},
};

impl WindowApp {
    fn owns_pane(&self, tab: TabId, pane: PaneId) -> bool {
        self.model
            .tabs
            .get(&tab)
            .is_some_and(|entry| entry.panes.iter().any(|entry| entry.id == pane))
    }
    pub(crate) fn run(&mut self, intents: Vec<UiIntent>) -> Result<Update, Error> {
        let mut update = Update::default();
        for intent in intents {
            match intent {
                UiIntent::Host(HostAction::OpenCommands) => {
                    update.commands.push(Command::OpenCommands)
                }
                UiIntent::Host(HostAction::RunCommand(id)) => {
                    update.commands.push(Command::RunCommand(id))
                }
                UiIntent::Host(HostAction::QuickTerminal) => {
                    update.commands.push(Command::QuickTerminal)
                }
                UiIntent::Host(HostAction::SearchTabs) => update.commands.push(Command::SearchTabs),
                UiIntent::Refresh => {
                    self.refresh_storage();
                    update.commands.push(Command::Refresh);
                }
                UiIntent::Domain(intent) => update.merge(self.dispatch(intent)?),
                UiIntent::SetClipboard(text) => update.commands.push(Command::SetClipboard(text)),
                UiIntent::RequestClipboard => update.commands.push(Command::RequestClipboard),
                UiIntent::Host(HostAction::Custom(action)) => update
                    .commands
                    .push(Command::Host(HostCommand::CustomAction(action))),
                UiIntent::Host(HostAction::MoveTabToNewWindow(id)) => {
                    self.model
                        .tabs
                        .get(&id)
                        .ok_or_else(|| Error("Tab no longer exists".into()))?;
                    update
                        .commands
                        .push(Command::Host(HostCommand::MoveTabToNewWindow(id)));
                }
                UiIntent::Host(HostAction::CloseTab(id)) => {
                    if self.model.tabs.contains_key(&id) {
                        update.commands.push(Command::ConfirmClose(id));
                    }
                }
                UiIntent::Host(HostAction::ClosePane(tab, pane)) => {
                    if self.owns_pane(tab, pane) {
                        update.commands.push(Command::ConfirmClosePane(tab, pane));
                    }
                }
                UiIntent::Host(HostAction::KillPane(tab, pane)) => {
                    if self.owns_pane(tab, pane) {
                        update
                            .commands
                            .push(Command::Host(HostCommand::KillPane(pane)));
                    }
                }
                UiIntent::Host(HostAction::DetachPane { tab, pane, index }) => {
                    if self.owns_pane(tab, pane) {
                        update
                            .commands
                            .push(Command::Host(HostCommand::DetachPane { pane, index }));
                    }
                }
                UiIntent::Host(HostAction::JoinPane { pane, tab }) => {
                    let owner = self
                        .model
                        .tabs
                        .values()
                        .find(|entry| entry.panes.iter().any(|entry| entry.id == pane))
                        .map(|entry| entry.id);
                    if owner.is_some_and(|owner| owner != tab) && self.model.tabs.contains_key(&tab)
                    {
                        update
                            .commands
                            .push(Command::Host(HostCommand::JoinPane { pane, tab }));
                    }
                }
                UiIntent::Host(HostAction::JoinTab { source, tab }) => {
                    if source != tab
                        && self.model.tabs.contains_key(&source)
                        && self.model.tabs.contains_key(&tab)
                    {
                        update
                            .commands
                            .push(Command::Host(HostCommand::JoinTab { source, tab }));
                    }
                }
                UiIntent::Host(HostAction::ShowTab { window, tab }) => {
                    update.commands.push(Command::ShowTab { window, tab });
                }
                UiIntent::Host(HostAction::AdoptPane { domain, pane }) => {
                    update.commands.push(Command::AdoptPane { domain, pane });
                }
                UiIntent::Host(HostAction::FocusPane(tab, pane)) => {
                    if self.owns_pane(tab, pane) {
                        update
                            .commands
                            .push(Command::Host(HostCommand::FocusPane { tab, pane }));
                    }
                }
            }
        }
        Ok(update)
    }
}

//! Typed completions between the GUI host and its adapter.
use super::*;

#[derive(Clone, Default)]
pub struct SpawnContext {
    pub new_window: bool,
    pub token: Option<app::SpawnToken>,
    pub launch: Option<core::LaunchSpec>,
    pub transfer: Option<app::WindowTransfer>,
}

pub enum Message {
    RefreshProfile(String),
    Error(String),
    CloseCheck {
        tab: core::TabId,
        pane: Option<core::PaneId>,
        process: Option<String>,
    },
    Initialize {
        tab: core::TabId,
        context: SpawnContext,
    },
    Spawned {
        tab: core::TabId,
        context: SpawnContext,
    },
    ShowTab(core::TabId),
    SpawnFailed(SpawnContext),
    Paste {
        token: u64,
        text: Option<String>,
    },
    Managed(Result<(), String>),
    Store {
        request_id: u64,
        result: Result<vtabs_store::Response, String>,
    },
    Action(core::Action),
    Adoptable {
        search: u64,
        panes: Vec<codec::AdoptablePane>,
    },
    Hooks(Vec<lua::HookCompletion>),
}

impl Message {
    pub fn hides_terminal(&self) -> bool {
        matches!(self, Self::Action(action) if !matches!(action, core::Action::Ui(core::UiAction::QuickTerminal | core::UiAction::QuickProgram(_))))
    }
}

impl Adapter {
    pub(super) fn handle_message(&mut self, message: Message) {
        let before = self.app.model().revision;
        let hook_completion = matches!(message, Message::Hooks(_));
        match message {
            Message::RefreshProfile(profile) => {
                if profile == self.app.model().profile {
                    self.app.refresh_storage();
                }
            }
            Message::Error(error) => self.app.ui_mut().set_error(error),
            Message::CloseCheck { tab, pane, process } => self.close_checked(tab, pane, process),
            Message::Initialize { tab, context } => {
                if let Some(mut transfer) = context.transfer {
                    transfer.tab.id = tab;
                    if let Err(error) = self.app.import_transfer_before_snapshot(transfer) {
                        self.app.ui_mut().set_error(error.to_string());
                    }
                }
            }
            Message::Spawned { tab, context } => {
                if !context.new_window {
                    if let Some(token) = context.token {
                        let result = self.app.spawn_completed_with_token(tab, token);
                        self.apply(result);
                    }
                }
                if let Some(launch) = context.launch {
                    self.app.set_launch(tab, launch).ok();
                }
            }
            Message::ShowTab(id) => {
                self.app.ui_mut().dismiss();
                self.app.ui_mut().release_focus();
                if self.app.model().tabs.contains_key(&id) {
                    self.dispatch(core::Intent::ActivateTab(id));
                } else {
                    self.pending_show = Some(id);
                }
            }
            Message::SpawnFailed(context) => {
                if let Some(token) = context.token {
                    self.app.cancel_spawn(&token);
                }
            }
            Message::Paste { token, text } => {
                self.expire_paste(Instant::now());
                if self
                    .input
                    .pending
                    .as_ref()
                    .is_some_and(|pending| token == pending.token)
                {
                    self.finish_paste(text, None);
                }
            }
            Message::Managed(result) => {
                let saved = result.is_ok();
                self.app.complete_managed_write(result);
                if saved {
                    std::thread::spawn(config::reload);
                }
            }
            Message::Store { request_id, result } => {
                self.outstanding = None;
                match result {
                    Ok(response) => {
                        let result = self.app.complete_storage(response);
                        self.apply(result);
                    }
                    Err(error) => self.app.storage_failed(request_id, error),
                }
            }
            Message::Action(action) => {
                self.cancel_paste();
                match action {
                    core::Action::Ui(core::UiAction::Settings) => self.app.open_settings(),
                    core::Action::Ui(core::UiAction::CreateSpace) => self.app.open_create_space(),
                    core::Action::Ui(core::UiAction::Navigator) => self.open_tab_navigator(),
                    core::Action::Ui(core::UiAction::TabActions) => self.app.open_tab_actions(),
                    core::Action::Ui(core::UiAction::QuickTerminal) => {
                        self.command(app::Command::QuickTerminal)
                    }
                    core::Action::Ui(core::UiAction::QuickProgram(program)) => {
                        match terminal_program(*program) {
                            Ok(program) => {
                                self.show_terminal(Command::ToggleTerminalProgram(program))
                            }
                            Err(error) => self.app.ui_mut().set_error(error),
                        }
                    }
                    core::Action::Ui(core::UiAction::RetryStorage) => self.app.retry_storage(),
                    core::Action::Intent(intent) => self.dispatch(intent),
                }
            }
            Message::Adoptable { search, panes } => {
                if search == self.adoptable_search {
                    self.app.set_adoptable_panes(adoption::entries(
                        panes,
                        self.app.model().home.as_deref(),
                    ));
                }
            }
            Message::Hooks(results) => {
                self.hooks.pending = false;
                let selection = (
                    self.app.model().selected_space.clone(),
                    self.app.model().selected_tab,
                );
                if !lua::complete(&mut self.app, results) {
                    self.hooks.retry();
                }
                self.hooks.window_dirty |= selection
                    != (
                        self.app.model().selected_space.clone(),
                        self.app.model().selected_tab,
                    );
                self.schedule_hooks();
            }
        }
        if !hook_completion {
            self.hooks.window_dirty |= before != self.app.model().revision;
        }
        self.schedule_hooks();
        self.storage();
        self.publish();
    }
}

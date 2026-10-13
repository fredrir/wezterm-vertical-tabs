//! Sidebar host commands carried out on the mux: close, join, reorder, move and spawn.
use super::*;

impl Adapter {
    pub(super) fn host_command(&mut self, command: core::HostCommand) {
        use vtabs_app::core::HostCommand as C;
        match command {
            C::Activate(id) => self.commands.push(Command::Activate(id as usize)),
            C::Close(id) => self.commands.push(Command::Close(id as usize, false)),
            C::Rename { id, title } => {
                if let Some(tab) = mux::Mux::get().get_tab(id as usize) {
                    tab.set_title(&title);
                }
            }
            C::Spawn {
                space_id,
                launch,
                folder_id,
            } => {
                self.transfers.spawn_into(space_id, folder_id);
                self.commands.push(Command::Spawn(spawn(launch), false));
            }
            C::Reorder { visible_order } => self.reorder(visible_order),
            C::MoveTabToNewWindow(id) => self.move_to_new_window(id as usize),
            C::KillPane(pane) => {
                let mux = mux::Mux::get();
                if let Some((_, _, tab)) = mux.resolve_pane_id(pane as usize) {
                    if let Some(tab) = mux.get_tab(tab) {
                        tab.kill_pane(pane as usize);
                    }
                }
            }
            C::DetachPane { pane, index } => {
                self.transfers.place_detached(index, Instant::now());
                let window = self.window_id;
                self.rearrange(async move {
                    mux::Mux::get()
                        .move_pane_to_new_tab(pane as usize, Some(window), None)
                        .await
                        .map(|_| ())
                });
            }
            C::JoinPane { pane, tab } => self.join(vec![pane as usize], tab as usize),
            C::JoinTab { source, tab } => {
                let panes = mux::Mux::get()
                    .get_tab(source as usize)
                    .map(|tab| tab.iter_panes_ignoring_zoom())
                    .unwrap_or_default()
                    .iter()
                    .map(|entry| entry.pane.pane_id())
                    .collect();
                self.join(panes, tab as usize);
            }
            C::FocusPane { tab, pane } => {
                let mux = mux::Mux::get();
                if let (Some(tab), Some(pane)) =
                    (mux.get_tab(tab as usize), mux.get_pane(pane as usize))
                {
                    tab.set_active_pane(&pane);
                }
            }
            C::CustomAction(name) => self.commands.push(Command::Semantic(name)),
        }
    }
    /// Local panes answer at once; a client pane's server answers with a `CloseCheck` message.
    pub(super) fn check_close(&mut self, tab: core::TabId, pane: Option<core::PaneId>) {
        let Some(host) = mux::Mux::try_get().and_then(|mux| mux.get_tab(tab as usize)) else {
            return self.close_checked(tab, pane, None);
        };
        let reason = if pane.is_some() {
            mux::pane::CloseReason::Pane
        } else {
            mux::pane::CloseReason::Tab
        };
        let (remote, local): (Vec<_>, Vec<_>) = host
            .iter_panes_ignoring_zoom()
            .into_iter()
            .map(|entry| entry.pane)
            .filter(|entry| pane.is_none_or(|pane| entry.pane_id() as u64 == pane))
            .partition(|entry| {
                entry
                    .downcast_ref::<wezterm_client::pane::ClientPane>()
                    .is_some()
            });
        let busy = local
            .iter()
            .find(|entry| !entry.can_close_without_prompting(reason))
            .map(|entry| process_name(entry.as_ref()));
        if busy.is_some() || remote.is_empty() {
            return self.close_checked(tab, pane, busy);
        }
        let endpoint = self.endpoint(self.window_id);
        promise::spawn::spawn(async move {
            let mut busy = None;
            for entry in &remote {
                let Some(client) = entry.downcast_ref::<wezterm_client::pane::ClientPane>() else {
                    continue;
                };
                // A stock server cannot answer; keep upstream's cautious prompt for it.
                match client.close_info().await {
                    Ok(info) if !info.prompt => {}
                    Ok(info) => busy = Some(info.process),
                    Err(_) => busy = Some(String::new()),
                }
                if busy.is_some() {
                    break;
                }
            }
            if let Some(endpoint) = endpoint {
                endpoint.send(Message::CloseCheck {
                    tab,
                    pane,
                    process: busy,
                });
            }
        })
        .detach();
    }
    pub(super) fn close_checked(
        &mut self,
        tab: core::TabId,
        pane: Option<core::PaneId>,
        busy: Option<String>,
    ) {
        match (pane, busy) {
            (None, Some(process)) => self.app.ui_mut().confirm_close_tab(tab, &process),
            (None, None) => self.dispatch(core::Intent::CloseTab(tab)),
            (Some(pane), Some(process)) => {
                self.app.ui_mut().confirm_close_pane(tab, pane, &process)
            }
            (Some(pane), None) => {
                self.command(app::Command::Host(core::HostCommand::KillPane(pane)))
            }
        }
    }
    /// Each pane lands as a split beside the target tab's active pane.
    fn join(&mut self, panes: Vec<usize>, tab: usize) {
        let Some(beside) = mux::Mux::get()
            .get_tab(tab)
            .and_then(|tab| tab.get_active_pane())
            .map(|pane| pane.pane_id())
        else {
            return;
        };
        self.rearrange(async move {
            for pane in panes {
                mux::Mux::get()
                    .split_pane(
                        beside,
                        mux::tab::SplitRequest {
                            target_is_second: true,
                            ..Default::default()
                        },
                        mux::domain::SplitSource::MovePane(pane),
                        config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
                    )
                    .await?;
            }
            Ok(())
        });
    }
    /// The mux moves panes asynchronously; a refusal surfaces as the sidebar's error.
    pub(super) fn rearrange(
        &self,
        work: impl std::future::Future<Output = anyhow::Result<()>> + 'static,
    ) {
        let endpoint = self.endpoint(self.window_id);
        promise::spawn::spawn(async move {
            let Err(error) = work.await else { return };
            log::warn!("tabs: {error:#}");
            if let Some(endpoint) = endpoint {
                endpoint.send(Message::Error(format!("{error:#}")));
            }
        })
        .detach();
    }
    /// Applies the sidebar's order to every tab of the mux window, keeping its active tab.
    fn reorder(&mut self, visible_order: Vec<core::TabId>) {
        let visible: std::collections::HashSet<_> =
            visible_order.iter().map(|id| *id as usize).collect();
        let mut next = visible_order.into_iter();
        let order: Vec<usize> = self
            .transfers
            .hosts()
            .iter()
            .map(|id| {
                if visible.contains(id) {
                    next.next().unwrap() as usize
                } else {
                    *id
                }
            })
            .collect();
        if let Some(mut window) = mux::Mux::get().get_window_mut(self.window_id) {
            let active = window.get_active_tab().map(|t| t.tab_id());
            let known: std::collections::HashSet<_> =
                window.iter_tabs().map(|t| t.tab_id()).collect();
            if order.len() == known.len()
                && order
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    == known
            {
                for (index, id) in order.iter().enumerate() {
                    let old = window.iter_tabs().position(|t| t.tab_id() == *id).unwrap();
                    if old != index {
                        let tab = window.remove_tab_idx(old);
                        window.insert_tab_at_idx(index, &tab);
                    }
                }
                let index = window.iter_tabs().position(|t| Some(t.tab_id()) == active);
                if let Some(index) = index {
                    window.set_active_tab_idx_without_saving(index);
                }
            }
        }
        self.commands.push(Command::Resync);
    }
    /// The new window restores the tab's sidebar state from the exported transfer.
    fn move_to_new_window(&mut self, id: usize) {
        let mux = mux::Mux::get();
        let Some(tab) = mux.get_tab(id) else { return };
        let context = match self.app.export_transfer(id as u64) {
            Ok(transfer) => SpawnContext {
                new_window: true,
                transfer: Some(transfer),
                ..SpawnContext::default()
            },
            Err(error) => {
                log::warn!("tabs move: {error}");
                SpawnContext::default()
            }
        };
        if let Some(pane) = tab.get_active_pane() {
            if pane
                .downcast_ref::<wezterm_client::pane::ClientPane>()
                .is_some()
            {
                if tab.count_panes() != Some(1) {
                    self.message(Message::Error("WezTerm mux supports moving single-pane tabs; this split tab stays in its window".into()));
                    return;
                }
                let pane_id = pane.pane_id();
                let source_id = self.window_id;
                let workspace = mux
                    .get_window(source_id)
                    .map(|w| w.get_workspace().to_owned());
                promise::spawn::spawn(async move {
                    match mux::Mux::get()
                        .move_pane_to_new_tab(pane_id, None, workspace)
                        .await
                    {
                        Ok((tab, window)) => {
                            spawn_completed(window, tab.tab_id(), context);
                        }
                        Err(error) => log::error!("move tab: {error}"),
                    }
                })
                .detach();
                return;
            }
        }
        // This operation retains the tab and its entire split tree.
        let builder = mux.new_empty_window(Some(mux.active_workspace()), None);
        let destination = *builder;
        if let Some(mut source) = mux.get_window_mut(self.window_id) {
            source.remove_tab_id(id);
        }
        if let Err(error) = mux.add_tab_to_window(&tab, destination) {
            let _ = mux.add_tab_to_window(&tab, self.window_id);
            self.message(Message::Error(format!("move tab: {error}")));
            return;
        }
        spawn_completed(destination, id, context);
        drop(builder);
        mux.prune_dead_windows();
        self.commands.push(Command::Resync);
    }
}

fn process_name(pane: &dyn mux::pane::Pane) -> String {
    pane.get_foreground_process_name(mux::pane::CachePolicy::AllowStale)
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

fn spawn(launch: core::LaunchSpec) -> config::keyassignment::SpawnCommand {
    config::keyassignment::SpawnCommand {
        args: (!launch.args.is_empty()).then_some(launch.args),
        cwd: launch.cwd.map(Into::into),
        set_environment_variables: launch.env.into_iter().collect(),
        domain: launch
            .domain
            .map(config::keyassignment::SpawnTabDomain::DomainName)
            .unwrap_or(config::keyassignment::SpawnTabDomain::CurrentPaneDomain),
        ..Default::default()
    }
}

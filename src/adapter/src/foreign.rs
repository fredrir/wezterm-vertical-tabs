//! Tabs that live elsewhere: other windows, other workspaces and detached domains.
use super::*;

impl Adapter {
    /// Other windows list this one's tabs; republish only when they could have changed.
    pub(super) fn publish(&mut self) {
        let revision = self.app.model().revision;
        if self.published == Some(revision) {
            return;
        }
        self.published = Some(revision);
        let model = self.app.model();
        let home = model.home.as_deref();
        let window = self.window_id as u64;
        let tabs = model
            .spaces
            .iter()
            .flat_map(|space| {
                model
                    .space_tabs(&space.id)
                    .into_iter()
                    .map(move |id| ui::ForeignTab::new(window, &model.tabs[&id], home, &space.name))
            })
            .collect();
        directory::publish(self.window_id, tabs);
    }
    /// Windows shown in a GUI list themselves; the mux lists windows of other workspaces.
    pub(super) fn sync_foreign(&mut self) {
        let front_end = crate::frontend::try_front_end();
        let shown = |id| {
            front_end
                .as_ref()
                .is_some_and(|front_end| front_end.gui_window_for_mux_window(id).is_some())
        };
        let mut tabs = directory::foreign(self.window_id, shown);
        let Some(mux) = mux::Mux::try_get() else {
            return self.app.set_foreign_tabs(tabs);
        };
        let home = self.app.model().home.clone();
        for id in mux.iter_windows() {
            if id == self.window_id || shown(id) {
                continue;
            }
            let Some(window) = mux.get_window(id) else {
                continue;
            };
            let place = match window.get_workspace() {
                workspace
                    if workspace.starts_with(BACKING_WORKSPACE_PREFIX)
                        || workspace.starts_with(QUICK_WORKSPACE_PREFIX) =>
                {
                    continue;
                }
                DETACHED_WORKSPACE => "detached".to_owned(),
                workspace => workspace.to_owned(),
            };
            for tab in window.iter_tabs() {
                let Some(pane) = tab.get_active_pane() else {
                    continue;
                };
                let cwd = pane
                    .get_current_working_dir(mux::pane::CachePolicy::AllowStale)
                    .map(|url| url.path().to_string())
                    .unwrap_or_default();
                let location =
                    location::Location::of(&pane, &cwd, &self.local_host, &mut self.repos);
                let tab = core::Tab {
                    id: tab.tab_id() as u64,
                    title: Some(tab.get_title())
                        .filter(|title| !title.is_empty())
                        .unwrap_or_else(|| pane.get_title()),
                    repo_root: location.repo_root,
                    home: location.home,
                    remote: location.remote,
                    os: location.os,
                    cwd,
                    ..Default::default()
                };
                tabs.push(ui::ForeignTab::new(
                    id as u64,
                    &tab,
                    home.as_deref(),
                    &place,
                ));
            }
        }
        self.app.set_foreign_tabs(tabs);
    }
    pub(super) fn open_tab_navigator(&mut self) {
        self.sync_foreign();
        self.app.set_adoptable_panes(Vec::new());
        self.app.open_tab_navigator();
        self.adoptable_search += 1;
        if let Some(endpoint) = self.endpoint(self.window_id) {
            adoption::find(endpoint, self.adoptable_search);
        }
    }
    /// Detached tabs come into this window; others are shown where they live.
    pub(super) fn show_tab(&mut self, window: usize, tab: core::TabId) {
        if self.reattach(tab) {
            return;
        }
        let Some(gui) = crate::frontend::try_front_end()
            .and_then(|front_end| front_end.gui_window_for_mux_window(window))
        else {
            let mux = mux::Mux::get();
            let Some(workspace) = mux.get_window(window).map(|w| w.get_workspace().to_owned())
            else {
                return self.app.ui_mut().set_error("Window closed");
            };
            if workspace == DETACHED_WORKSPACE {
                return self.move_tab_here(tab);
            }
            if let Some(mut window) = mux.get_window_mut(window) {
                if let Some(index) = window.get_tab_idx_for_id(tab as usize) {
                    window.remember_and_set_active_tab_idx(index);
                }
            }
            return mux.set_active_workspace(&workspace);
        };
        gui.window.focus();
        WindowEndpoint::new(gui.window, window).send(Message::ShowTab(tab));
    }
    /// The tab's active pane becomes a tab here and its other panes rejoin it.
    fn move_tab_here(&mut self, tab: core::TabId) {
        let Some(host) = mux::Mux::get().get_tab(tab as usize) else {
            return;
        };
        let mut panes = host.iter_panes_ignoring_zoom();
        panes.sort_by_key(|entry| !entry.is_active);
        let mut panes = panes.into_iter().map(|entry| entry.pane.pane_id());
        let Some(first) = panes.next() else {
            return;
        };
        let rest: Vec<_> = panes.collect();
        let endpoint = self.endpoint(self.window_id);
        let window = self.window_id;
        self.rearrange(async move {
            let mux = mux::Mux::get();
            let (tab, _) = mux.move_pane_to_new_tab(first, Some(window), None).await?;
            for pane in rest {
                mux.split_pane(
                    first,
                    mux::tab::SplitRequest {
                        target_is_second: true,
                        ..Default::default()
                    },
                    mux::domain::SplitSource::MovePane(pane),
                    config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
                )
                .await?;
            }
            if let Some(endpoint) = endpoint {
                endpoint.send(Message::ShowTab(tab.tab_id() as u64));
            }
            Ok(())
        });
    }
    fn reattach(&mut self, tab: core::TabId) -> bool {
        let Some(domain) = directory::detached(|records| {
            let mut domain = None;
            for record in records {
                record.show = record.tab.id == tab;
                if record.show {
                    domain = Some(record.domain);
                }
            }
            domain
        }) else {
            return false;
        };
        let Some(domain) = mux::Mux::get().get_domain(domain) else {
            return true;
        };
        let window = self.window_id;
        promise::spawn::spawn(async move {
            if let Err(error) = domain.attach(Some(window)).await {
                log::warn!("tabs reattach: {error:#}");
            }
        })
        .detach();
        true
    }
    /// Only a client domain can reattach, and only while the mux still knows it.
    pub(super) fn remember_detached(&self, id: core::TabId) {
        if let Some(record) =
            self.transfers
                .detached_record(id, &self.app, self.window_id, |domain| {
                    mux::Mux::get()
                        .get_domain(domain)
                        .is_some_and(|domain| domain.state() == mux::domain::DomainState::Detached)
                })
        {
            directory::remember(record);
        }
    }
    /// Tabs gone from the mux window whose domain detached or vanished.
    pub(super) fn remember_departed(&self) {
        let host_tabs = self
            .transfers
            .hosts()
            .iter()
            .map(|id| *id as u64)
            .collect::<std::collections::HashSet<_>>();
        let mux = mux::Mux::get();
        let departed = self
            .app
            .model()
            .tabs
            .iter()
            .filter_map(|(id, tab)| {
                if host_tabs.contains(id) {
                    return None;
                }
                let detached = mux
                    .get_domain_by_name(&tab.domain)
                    .is_none_or(|domain| domain.state() == mux::domain::DomainState::Detached);
                detached.then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in departed {
            self.remember_detached(id);
        }
    }
    /// Reattached tabs return with new IDs; whichever window receives one restores it.
    pub(super) fn restore_reattached(&mut self) {
        let mux = mux::Mux::get();
        let model = self.app.model();
        let returned = directory::detached(|records| {
            let mut returned = Vec::new();
            for record in std::mem::take(records) {
                let Some(domain) = mux.get_domain(record.domain) else {
                    continue;
                };
                if domain.state() == mux::domain::DomainState::Detached {
                    records.push(record);
                    continue;
                }
                let Some(id) = wezterm_client::domain::ClientDomain::get_client_inner_for_domain(
                    record.domain,
                )
                .ok()
                .and_then(|inner| inner.remote_to_local_tab_id(record.remote)) else {
                    continue;
                };
                if model.tabs.contains_key(&(id as u64)) {
                    returned.push((id as u64, record));
                } else if mux.get_tab(id).is_some() {
                    records.push(record);
                }
            }
            returned
        });
        let mut show = None;
        for (id, record) in returned {
            let result = self.app.restore_tab(id, &record.tab);
            self.apply(result);
            if record.show {
                show = Some(id);
            }
        }
        if let Some(id) = show {
            self.dispatch(core::Intent::ActivateTab(id));
        }
    }
}

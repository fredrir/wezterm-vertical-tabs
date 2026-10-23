//! What the mux reports about a window's tabs, read once per host snapshot.
use super::*;

pub struct TabInfo {
    pub id: usize,
    pub title: String,
    pub cwd: String,
    pub domain: String,
    pub host: String,
    pub user: String,
    pub process: String,
    pub unread: bool,
    pub bell: bool,
    pub user_vars: HashMap<String, String>,
}

pub struct Facts {
    pub revision: u64,
    pub tabs: Vec<TabInfo>,
    pub active: Option<usize>,
    pub focused: bool,
    pub config_epoch: usize,
}

impl Facts {
    /// SSH domains come from the window's configuration, so its overrides apply.
    pub fn of(snapshot: Snapshot) -> Self {
        let mux = mux::Mux::get();
        let tabs = snapshot
            .tabs
            .iter()
            .map(|tab| {
                let pane = tab.get_active_pane();
                let title = tab.get_title();
                let cwd = pane
                    .as_ref()
                    .and_then(|p| p.get_current_working_dir(mux::pane::CachePolicy::AllowStale));
                let domain = pane
                    .as_ref()
                    .and_then(|p| mux.get_domain(p.domain_id()))
                    .map(|d| d.domain_name().to_string())
                    .unwrap_or_default();
                let ssh = snapshot
                    .config
                    .ssh_domains
                    .iter()
                    .flatten()
                    .find(|ssh| ssh.name == domain);
                TabInfo {
                    id: tab.tab_id(),
                    title: if title.is_empty() {
                        pane.as_ref().map(|p| p.get_title()).unwrap_or_default()
                    } else {
                        title
                    },
                    cwd: cwd
                        .as_ref()
                        .map(|u| u.path().to_string())
                        .unwrap_or_default(),
                    host: cwd
                        .as_ref()
                        .and_then(|u| u.host_str())
                        .filter(|h| !h.is_empty())
                        .map(str::to_string)
                        .or_else(|| ssh.map(|s| s.remote_address.clone()))
                        .unwrap_or_default(),
                    user: cwd
                        .as_ref()
                        .map(|u| u.username())
                        .filter(|u| !u.is_empty())
                        .map(str::to_string)
                        .or_else(|| ssh.and_then(|s| s.username.clone()))
                        .unwrap_or_default(),
                    domain,
                    process: pane
                        .as_ref()
                        .and_then(|p| {
                            p.get_foreground_process_name(mux::pane::CachePolicy::AllowStale)
                        })
                        .unwrap_or_default(),
                    unread: pane.as_ref().is_some_and(|p| p.has_unseen_output()),
                    bell: snapshot.bells.contains(&tab.tab_id()),
                    user_vars: pane
                        .as_ref()
                        .map(|p| p.copy_user_vars())
                        .unwrap_or_default(),
                }
            })
            .collect();
        Self {
            revision: snapshot.revision,
            tabs,
            active: snapshot.active,
            focused: snapshot.focused,
            config_epoch: snapshot.config.generation(),
        }
    }
}

impl Adapter {
    pub(super) fn apply_facts(&mut self, snapshot: Facts) {
        if !snapshot.focused {
            self.cancel_paste();
        }
        // A pane dragged out to a position becomes a tab there once the mux reports it.
        let arrived = self.transfers.observe_hosts(
            snapshot.tabs.iter().map(|tab| tab.id).collect(),
            Instant::now(),
        );
        let before = self.app.model().revision;
        if self.config_generation != snapshot.config_epoch {
            self.config_generation = snapshot.config_epoch;
            self.hooks.configure(lua::hooks_enabled());
            self.cancel_paste();
            let config = lua::configuration();
            self.managed_path = config.managed_path.clone();
            let result = self.app.config(config.value());
            self.apply(result);
        }
        let repos = &mut self.repos;
        let local_host = &self.local_host;
        let mux = mux::Mux::get();
        let tabs = snapshot
            .tabs
            .into_iter()
            .map(|tab| host_tab(&mux, tab, local_host, repos))
            .collect::<Vec<_>>();
        self.hooks.observe(&tabs);
        self.remember_departed();
        self.transfers.observe_remotes(remote_tabs(&mux, &tabs));
        let result = self.app.update(app::HostSnapshot {
            revision: snapshot.revision,
            tabs,
            active_tab: snapshot.active.map(|id| id as u64),
            metrics: self.app.metrics(),
            focused: snapshot.focused,
            configuration_epoch: snapshot.config_epoch as u64,
        });
        self.apply(result);
        self.hooks.changed(before != self.app.model().revision);
        if let Some((id, index)) = arrived {
            self.dispatch(core::Intent::MoveTab { id, index });
        }
        self.restore_reattached();
        if let Some(id) = self.transfers.arrived_to_show(&self.app) {
            self.dispatch(core::Intent::ActivateTab(id));
        }
        self.schedule_hooks();
        self.storage();
        self.publish();
    }
}

fn host_tab(
    mux: &mux::Mux,
    tab: TabInfo,
    local_host: &str,
    repos: &mut repos::Repos,
) -> core::HostTab {
    let domain = mux.get_domain_by_name(&tab.domain);
    let launch = domain
        .as_ref()
        .filter(|domain| domain.spawnable())
        .map(|_| core::LaunchSpec {
            domain: Some(tab.domain.clone()),
            cwd: (!tab.cwd.is_empty()).then_some(tab.cwd.clone()),
            ..Default::default()
        });
    let host_tab = mux.get_tab(tab.id);
    let identities = host_tab
        .as_ref()
        .into_iter()
        .flat_map(|host| host.iter_panes_ignoring_zoom())
        .filter_map(|entry| {
            entry
                .pane
                .downcast_ref::<wezterm_client::pane::ClientPane>()?
                .persistence_identity()
                .map(|(session, entity)| core::TabIdentity { session, entity })
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let location = match host_tab.as_ref().and_then(|host| host.get_active_pane()) {
        Some(pane) => location::Location::of(&pane, &tab.cwd, local_host, repos),
        None => location::Location::unanswered(false, String::new(), &tab.cwd, repos),
    };
    let panes = host_tab
        .map(|host| host.iter_panes_ignoring_zoom())
        .filter(|panes| panes.len() > 1)
        .into_iter()
        .flatten()
        .map(|entry| {
            let cwd = entry
                .pane
                .get_current_working_dir(mux::pane::CachePolicy::AllowStale)
                .map(|url| url.path().to_string())
                .unwrap_or_default();
            let location = location::Location::of(&entry.pane, &cwd, local_host, repos);
            core::TabPane {
                id: entry.pane.pane_id() as u64,
                title: entry.pane.get_title(),
                cwd,
                repo_root: location.repo_root,
                home: location.home,
                active: entry.is_active,
                remote: location.remote,
                os: location.os,
                left: entry.left.min(usize::from(u16::MAX)) as u16,
                top: entry.top.min(usize::from(u16::MAX)) as u16,
                width: entry.width.min(usize::from(u16::MAX)) as u16,
                height: entry.height.min(usize::from(u16::MAX)) as u16,
            }
        })
        .collect();
    core::HostTab {
        id: tab.id as u64,
        identities,
        title: tab.title,
        cwd: tab.cwd.clone(),
        repo_root: location.repo_root,
        home: location.home,
        domain: tab.domain.clone(),
        process: tab.process,
        remote: location.remote,
        os: location.os,
        panes,
        unread: tab.unread,
        bell: tab.bell,
        icon: tab.user_vars.get("icon").cloned().unwrap_or_default(),
        launch,
        host: tab.user_vars.get("hostname").cloned().unwrap_or(tab.host),
        user: tab.user_vars.get("username").cloned().unwrap_or(tab.user),
    }
}

fn remote_tabs(
    mux: &mux::Mux,
    tabs: &[core::HostTab],
) -> HashMap<core::TabId, transfers::RemoteTab> {
    tabs.iter()
        .filter_map(|tab| {
            let domain = mux.get_domain_by_name(&tab.domain)?;
            let client = domain.downcast_ref::<wezterm_client::domain::ClientDomain>()?;
            mux.get_tab(tab.id as usize)?;
            Some((
                tab.id,
                transfers::RemoteTab {
                    domain: domain.domain_id(),
                    remote: client.local_to_remote_tab_id(tab.id as usize)?,
                },
            ))
        })
        .collect()
}

//! What the mux reports about a window's tabs, read once per host snapshot.
use crate::termwindow::ui_host::Snapshot;
use std::collections::HashMap;

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
    pub window_id: usize,
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
            window_id: snapshot.window_id,
            tabs,
            active: snapshot.active,
            focused: snapshot.focused,
            config_epoch: snapshot.config.generation(),
        }
    }
}

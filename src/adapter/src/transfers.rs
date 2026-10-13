//! Spawn destinations and tab arrivals kept between requests and host snapshots.
use super::*;

const PLACEMENT_TIMEOUT: Duration = Duration::from_secs(3);

enum SpawnDestination {
    Space(String),
    Folder(String),
}

#[derive(Clone, Copy)]
pub(super) struct RemoteTab {
    pub domain: usize,
    pub remote: usize,
}

#[derive(Default)]
pub(super) struct Transfers {
    host_tabs: Vec<usize>,
    remote_tabs: HashMap<core::TabId, RemoteTab>,
    next_spawn: Option<SpawnDestination>,
    placement: Option<(usize, Instant)>,
    pending_show: Option<core::TabId>,
}

impl Transfers {
    pub fn spawn_into(&mut self, space: String, folder: Option<String>) {
        self.next_spawn = Some(match folder {
            Some(folder) => SpawnDestination::Folder(folder),
            None => SpawnDestination::Space(space),
        });
    }

    pub fn reserve_spawn(
        &mut self,
        app: &mut WindowApp,
        new_window: bool,
    ) -> Option<app::SpawnToken> {
        if new_window {
            return None;
        }
        match self.next_spawn.take() {
            Some(SpawnDestination::Folder(folder)) => app.reserve_spawn_in_folder(&folder).ok(),
            Some(SpawnDestination::Space(space)) => app.reserve_spawn_in(&space).ok(),
            None => Some(app.reserve_spawn()),
        }
    }

    pub fn place_detached(&mut self, index: Option<usize>, now: Instant) {
        self.placement = index.map(|index| (index, now));
    }

    pub fn observe_hosts(
        &mut self,
        tabs: Vec<usize>,
        now: Instant,
    ) -> Option<(core::TabId, usize)> {
        let arrived = self
            .placement
            .take()
            .filter(|(_, since)| now.saturating_duration_since(*since) < PLACEMENT_TIMEOUT)
            .and_then(|placement| {
                let known: std::collections::HashSet<_> = self.host_tabs.iter().collect();
                match tabs.iter().find(|id| !known.contains(id)) {
                    Some(id) => Some((*id as u64, placement.0)),
                    None => {
                        self.placement = Some(placement);
                        None
                    }
                }
            });
        self.host_tabs = tabs;
        arrived
    }

    pub fn hosts(&self) -> &[usize] {
        &self.host_tabs
    }

    pub fn observe_remotes(&mut self, tabs: HashMap<core::TabId, RemoteTab>) {
        self.remote_tabs = tabs;
    }

    pub fn detached_record(
        &self,
        id: core::TabId,
        app: &WindowApp,
        window_id: usize,
        detached: impl FnOnce(usize) -> bool,
    ) -> Option<directory::DetachedTab> {
        let remote = self.remote_tabs.get(&id)?;
        let tab = app.model().tabs.get(&id)?;
        if !detached(remote.domain) {
            return None;
        }
        Some(directory::DetachedTab {
            listing: ui::ForeignTab::new(
                window_id as u64,
                tab,
                app.model().home.as_deref(),
                format!("{}  detached", tab.domain),
            ),
            tab: tab.clone(),
            domain: remote.domain,
            remote: remote.remote,
            show: false,
        })
    }

    /// Completion may arrive before its tab is present in a host snapshot.
    pub fn show_when_present(&mut self, tab: core::TabId, app: &WindowApp) -> Option<core::TabId> {
        if app.model().tabs.contains_key(&tab) {
            Some(tab)
        } else {
            self.pending_show = Some(tab);
            None
        }
    }

    pub fn arrived_to_show(&mut self, app: &WindowApp) -> Option<core::TabId> {
        let tab = self.pending_show?;
        if app.model().tabs.contains_key(&tab) {
            self.pending_show = None;
            Some(tab)
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "../tests/transfers.rs"]
mod tests;

//! Remote pane operations: mirrored launches and this host's backing workspace.
use crate::client::Client;
use crate::domain::snapshot::{forget_panes, leaves};
use crate::domain::{ClientDomain, ClientInner};
use crate::pane::ClientPane;
use anyhow::anyhow;
use codec::{
    GetCodecVersionResponse, ListPanesResponse, MovePaneToNewTab, SpawnBesidePane, SpawnV2,
};
use config::keyassignment::SpawnTabDomain;
use mux::domain::Domain;
use mux::pane::{Pane, PaneId};
use mux::tab::{PaneEntry, Tab};
use mux::window::WindowId;
use mux::Mux;
use portable_pty::CommandBuilder;
use std::sync::Arc;
use wezterm_term::TerminalSize;

impl ClientDomain {
    /// Checks a local launch target before the caller changes its visible session.
    pub fn validate_spawn_beside_pane(&self, local_pane_id: PaneId) -> anyhow::Result<()> {
        self.remote_pane_for_spawn(local_pane_id).map(|_| ())
    }

    fn remote_pane_for_spawn(&self, local_pane_id: PaneId) -> anyhow::Result<PaneId> {
        anyhow::ensure!(
            !self.local_pane_layout(),
            "spawn beside a pane requires a mirrored client domain"
        );
        let pane = Mux::get()
            .get_pane(local_pane_id)
            .ok_or_else(|| anyhow!("pane {local_pane_id} is no longer open"))?;
        anyhow::ensure!(
            pane.domain_id() == self.local_domain_id,
            "pane {local_pane_id} does not belong to this client domain"
        );
        pane.downcast_ref::<ClientPane>()
            .map(ClientPane::remote_pane_id)
            .ok_or_else(|| anyhow!("not a client pane"))
    }

    /// Spawns beside a local pane's server-side process in `workspace`; returns its mirror.
    pub async fn spawn_beside_pane(
        &self,
        local_pane_id: PaneId,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
        workspace: String,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        let pane_id = self.remote_pane_for_spawn(local_pane_id)?;
        let inner = self
            .inner()
            .ok_or_else(|| anyhow!("domain has no assigned client"))?;
        let spawned = inner
            .client
            .spawn_beside_pane(SpawnBesidePane {
                pane_id,
                command,
                command_dir,
                size,
                workspace,
            })
            .await?;
        self.ensure_spawn_client(&inner)?;
        Self::resync_inner(Arc::clone(&inner)).await?;
        self.ensure_spawn_client(&inner)?;
        inner
            .remote_to_local_pane_id(spawned.pane_id)
            .and_then(|id| Mux::get().get_pane(id))
            .ok_or_else(|| anyhow!("pane {} not mirrored", spawned.pane_id))
    }

    fn ensure_spawn_client(&self, expected: &Arc<ClientInner>) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.inner()
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, expected)),
            "client domain detached or replaced while spawning a pane"
        );
        Ok(())
    }

    /// Remote panes this mux could show; see [`adoptable`].
    pub async fn adoptable_panes(&self) -> anyhow::Result<Vec<PaneEntry>> {
        let inner = self.adoption_inner()?;
        let panes = inner.client.list_panes().await?;
        Ok(adoptable(&inner, &panes))
    }

    /// Shows a remote pane in a new tab of `window` or a detached one, backed here for reattach.
    pub async fn adopt_pane(
        &self,
        remote_pane_id: PaneId,
        window: Option<WindowId>,
    ) -> anyhow::Result<(Arc<Tab>, WindowId, Arc<dyn Pane>)> {
        let inner = self.adoption_inner()?;
        let _serial = inner.topology.serial.lock().await;
        let panes = inner.client.list_panes().await?;
        let mut entry = adoptable(&inner, &panes)
            .into_iter()
            .find(|entry| entry.pane_id == remote_pane_id)
            .ok_or_else(|| anyhow!("remote pane {remote_pane_id} cannot be adopted"))?;
        let backing = mux::backing_workspace();
        if entry.workspace != backing {
            let moved = inner
                .client
                .move_pane_to_new_tab(MovePaneToNewTab {
                    pane_id: remote_pane_id,
                    window_id: None,
                    workspace_for_new_window: Some(backing),
                })
                .await?;
            entry.tab_id = moved.tab_id;
            entry.window_id = moved.window_id;
        }
        adopt_into(&inner, &entry, window)
    }

    fn adoption_inner(&self) -> anyhow::Result<Arc<ClientInner>> {
        anyhow::ensure!(
            self.local_pane_layout(),
            "adoption requires local_pane_layout"
        );
        self.inner()
            .ok_or_else(|| anyhow!("domain is not attached"))
    }

    /// The local mux owns the new tab; its pane is backed on the server.
    pub(super) async fn spawn_local_tab(
        &self,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
        window: WindowId,
    ) -> anyhow::Result<Arc<Tab>> {
        let pane = self.spawn_pane(size, command, command_dir).await?;
        let tab = Arc::new(Tab::new(&size));
        tab.assign_pane(&pane);
        let mux = Mux::get();
        mux.add_tab_no_panes(&tab);
        mux.add_tab_to_window(&tab, window)?;
        Ok(tab)
    }

    /// The server knows the moved pane by its remote id, within this domain only.
    pub(super) fn remote_pane_to_move(&self, pane_id: PaneId) -> anyhow::Result<PaneId> {
        let moved = Mux::get()
            .get_pane(pane_id)
            .ok_or_else(|| anyhow!("pane {pane_id} is invalid"))?;
        anyhow::ensure!(
            moved.domain_id() == self.domain_id(),
            "cannot move panes across client domains"
        );
        let moved = moved
            .downcast_ref::<ClientPane>()
            .ok_or_else(|| anyhow!("pane {pane_id} is not a ClientPane"))?;
        Ok(moved.remote_pane_id)
    }

    pub(super) async fn spawn_backing_pane(
        &self,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        anyhow::ensure!(
            self.config.local_pane_layout(),
            "client domain requires local_pane_layout"
        );
        let inner = self
            .inner()
            .ok_or_else(|| anyhow!("domain is not attached"))?;
        let _serial = inner.topology.serial.lock().await;
        let result = inner
            .client
            .spawn_v2(SpawnV2 {
                domain: SpawnTabDomain::DefaultDomain,
                window_id: None,
                size,
                command,
                command_dir,
                workspace: mux::backing_workspace(),
            })
            .await?;
        let pane: Arc<dyn Pane> = Arc::new(ClientPane::new(
            &inner,
            result.tab_id,
            result.pane_id,
            size,
            "wezterm",
        ));
        Mux::get().add_pane(&pane)?;
        // Subscribed while offscreen, so output arrives before the GUI's first render poll.
        let client = inner.client.clone();
        promise::spawn::spawn(async move {
            client
                .get_pane_render_changes(codec::GetPaneRenderChanges {
                    pane_id: result.pane_id,
                })
                .await
        })
        .detach();
        Ok(pane)
    }
}

const SESSION: &str = "session:";

impl ClientInner {
    pub(crate) fn server(&self) -> Option<String> {
        self.server.lock().unwrap().clone()
    }

    /// The running mux session; a restarted server starts a new one.
    pub(crate) fn session(&self) -> Option<String> {
        self.server()?.strip_prefix(SESSION).map(str::to_owned)
    }
}

async fn mux_session(client: &Client) -> Option<String> {
    let session = client.get_mux_session().await.ok()?.session;
    let valid = !session.is_empty()
        && session.len() <= 128
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    valid.then(|| format!("{SESSION}{session}"))
}

/// A running server has one session across routes; older servers only support adoption identity.
pub(super) async fn server_identity(
    local_pane_layout: bool,
    client: &Client,
    version: &GetCodecVersionResponse,
) -> Option<String> {
    if let Some(session) = mux_session(client).await {
        return Some(session);
    }
    if !local_pane_layout {
        return None;
    }
    let host = client
        .get_pane_location(codec::GetPaneLocation { pane_id: None })
        .await
        .map_err(|err| log::warn!("server location: {err:#}"))
        .ok()?;
    let config = version
        .config_file_path
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    Some(format!(
        "{}\0{}\0{config}",
        host.hostname,
        version.executable_path.display()
    ))
}

/// A reconnect can reach a restarted server, whose new panes reuse the dead panes' IDs.
pub(super) async fn follow_restart(inner: &ClientInner) {
    let Some(current) = mux_session(&inner.client).await else {
        return;
    };
    let previous = inner.server.lock().unwrap().replace(current.clone());
    if restarted(previous.as_deref(), &current) && inner.local_pane_layout {
        log::warn!(
            "domain {}: server restarted; dropping its panes",
            inner.local_domain_id
        );
        forget_panes(inner, |_| false);
    }
}

/// Only two sessions can tell a restart from an older server's identity.
fn restarted(previous: Option<&str>, current: &str) -> bool {
    previous.is_some_and(|previous| previous.starts_with(SESSION) && previous != current)
}

/// Whether a client domain of this mux shows the remote pane, by any route.
fn is_shown(inner: &ClientInner, remote_pane_id: PaneId) -> bool {
    Mux::get().iter_panes().iter().any(|pane| {
        pane.downcast_ref::<ClientPane>()
            .is_some_and(|pane| pane.shows(inner, remote_pane_id))
    })
}

/// Panes no domain here shows, minus those the server relays or backs for other hosts.
fn adoptable(inner: &ClientInner, panes: &ListPanesResponse) -> Vec<PaneEntry> {
    let backing = mux::backing_workspace();
    panes
        .tabs
        .iter()
        .flat_map(leaves)
        .filter(|entry| {
            entry.tty_name.is_some()
                && (entry.workspace == backing
                    || !entry.workspace.starts_with(mux::BACKING_WORKSPACE_PREFIX))
                && !is_shown(inner, entry.pane_id)
        })
        .cloned()
        .collect()
}

/// This host's backing tabs that nothing here shows, e.g. after this mux restarted.
pub(super) fn orphans(inner: &ClientInner, panes: &ListPanesResponse) -> Vec<PaneEntry> {
    if !inner.local_pane_layout {
        return vec![];
    }
    let backing = mux::backing_workspace();
    let mut entries = adoptable(inner, panes);
    entries.retain(|entry| entry.workspace == backing);
    entries
}

pub(super) fn restore_orphans(inner: &Arc<ClientInner>, orphans: Vec<PaneEntry>) {
    for entry in orphans {
        if let Err(err) = adopt_into(inner, &entry, None) {
            log::warn!("restoring remote pane {}: {err:#}", entry.pane_id);
        }
    }
}

fn adopt_into(
    inner: &Arc<ClientInner>,
    entry: &PaneEntry,
    window: Option<WindowId>,
) -> anyhow::Result<(Arc<Tab>, WindowId, Arc<dyn Pane>)> {
    let mux = Mux::get();
    let detached;
    let (window_id, size) = match window {
        Some(id) => {
            let size = mux
                .get_window(id)
                .ok_or_else(|| anyhow!("window {id} not found"))?
                .get_active_tab()
                .map(|tab| tab.get_size())
                .unwrap_or(entry.size);
            (id, size)
        }
        None => {
            detached = mux.new_empty_window(Some(mux::DETACHED_WORKSPACE.to_string()), None);
            (*detached, entry.size)
        }
    };
    let pane: Arc<dyn Pane> = Arc::new(ClientPane::new(
        inner,
        entry.tab_id,
        entry.pane_id,
        entry.size,
        &entry.title,
    ));
    let tab = Arc::new(Tab::new(&size));
    tab.assign_pane(&pane);
    pane.resize(size)?;
    mux.add_tab_and_active_pane(&tab)?;
    mux.add_tab_to_window(&tab, window_id)?;
    Ok((tab, window_id, pane))
}

#[cfg(test)]
#[path = "../../tests/vtabs/spawn.rs"]
mod vtabs_spawn_tests;

#[cfg(test)]
#[path = "../../tests/vtabs/session.rs"]
mod vtabs_session_tests;

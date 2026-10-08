//! Remote panes in this host's backing workspace: spawned, adopted and restored here.
use crate::client::Client;
use crate::domain::snapshot::leaves;
use crate::domain::{ClientDomain, ClientInner};
use crate::pane::ClientPane;
use anyhow::anyhow;
use codec::{GetCodecVersionResponse, ListPanesResponse, MovePaneToNewTab, SpawnV2};
use config::keyassignment::SpawnTabDomain;
use mux::pane::{Pane, PaneId};
use mux::tab::{PaneEntry, Tab};
use mux::window::WindowId;
use mux::Mux;
use portable_pty::CommandBuilder;
use std::sync::Arc;
use wezterm_term::TerminalSize;

impl ClientDomain {
    /// Remote panes this mux could show; see [`adoptable`].
    pub async fn adoptable_panes(&self) -> anyhow::Result<Vec<PaneEntry>> {
        let inner = self.adoption_inner()?;
        let panes = inner.client.list_panes().await?;
        Ok(adoptable(&inner, &panes))
    }

    /// Shows a remote pane in a new tab of `window`, or of a new detached window.
    /// A pane backed elsewhere first moves into a backing tab of this host, so a
    /// later attach restores it.
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
        // Subscribe while the replacement is still offscreen, so readiness and
        // prompt output arrive without waiting for the GUI's first render poll.
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

/// Identifies the server behind `client`, whichever route reached it.
pub(super) async fn server_identity(
    client: &Client,
    version: &GetCodecVersionResponse,
) -> Option<String> {
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

/// Whether a client domain of this mux shows the remote pane, by any route.
fn is_shown(inner: &ClientInner, remote_pane_id: PaneId) -> bool {
    Mux::get().iter_panes().iter().any(|pane| {
        pane.downcast_ref::<ClientPane>()
            .is_some_and(|pane| pane.shows(inner, remote_pane_id))
    })
}

/// Remote panes shown by no domain of this mux, excluding panes the server only
/// relays and tabs backing another host's panes.
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

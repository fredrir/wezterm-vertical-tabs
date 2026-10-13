//! Pane list snapshots: resyncs, warming new panes and refreshing a locally owned layout.
use crate::domain::{ClientDomain, ClientDomainConfig, ClientInner};
use crate::pane::ClientPane;
use codec::ListPanesResponse;
use mux::pane::{Pane, PaneId};
use mux::tab::{PaneEntry, PaneNode, TabId};
use mux::Mux;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

impl ClientDomainConfig {
    pub(super) fn local_pane_layout(&self) -> bool {
        match self {
            ClientDomainConfig::Unix(unix) => unix.local_pane_layout,
            ClientDomainConfig::Tls(tls) => tls.local_pane_layout,
            ClientDomainConfig::Ssh(ssh) => ssh.local_pane_layout,
        }
    }
}

impl ClientDomain {
    pub fn local_pane_layout(&self) -> bool {
        self.config.local_pane_layout()
    }

    pub(super) async fn resync_inner(inner: Arc<ClientInner>) -> anyhow::Result<()> {
        // Retain the old tab while its replacement screen is fetched.
        let _activity = mux::activity::Activity::new();
        let request = inner.topology.request();
        let _serial = inner.topology.serial.lock().await;
        if inner.topology.completed(request) {
            return Ok(());
        }
        loop {
            let (epoch, included) = inner.topology.ready().await;
            let panes = inner.client.list_panes().await?;
            let prepared = Self::prefetch_new_panes(&inner, &panes).await;
            if !inner.topology.accepts(epoch) {
                log::trace!("discard topology snapshot superseded by a local resize or kill");
                continue;
            }
            Self::process_pane_list(Arc::clone(&inner), panes, None, prepared)?;
            if inner.topology.finish(included) {
                return Ok(());
            }
        }
    }

    /// By remote ID: the mapping may be gone, or a stale snapshot may have re-added the pane.
    pub(crate) async fn remove_remote_pane(&self, remote_pane_id: PaneId) -> anyhow::Result<()> {
        self.resync().await?;
        let mux = Mux::get();
        let removed = mux.iter_panes().into_iter().filter(|pane| {
            pane.domain_id() == self.local_domain_id
                && pane
                    .downcast_ref::<ClientPane>()
                    .is_some_and(|client| client.remote_pane_id == remote_pane_id)
        });
        for pane in removed {
            if let Some(client) = pane.downcast_ref::<ClientPane>() {
                client.ignore_next_kill();
            }
            mux.remove_pane(pane.pane_id());
        }
        Ok(())
    }

    pub(super) async fn prefetch_new_panes(
        inner: &Arc<ClientInner>,
        panes: &ListPanesResponse,
    ) -> HashMap<PaneId, Arc<dyn Pane>> {
        let pending = panes
            .tabs
            .iter()
            // Only new panes in existing tabs; an initial attach renders incrementally.
            .filter(|root| {
                !inner.local_pane_layout
                    && root
                        .window_and_tab_ids()
                        .is_some_and(|(_, tab)| inner.remote_to_local_tab_id(tab).is_some())
            })
            .flat_map(leaves)
            .filter(|entry| {
                inner
                    .remote_to_local_pane_id(entry.pane_id)
                    .and_then(|id| Mux::get().get_pane(id))
                    .is_none()
            })
            .map(|entry| {
                let pane = Arc::new(ClientPane::new(
                    inner,
                    entry.tab_id,
                    entry.pane_id,
                    entry.size,
                    &entry.title,
                ));
                let working_dir = entry.working_dir.clone().map(Into::into);
                async move {
                    if let Err(err) = pane.prefetch(working_dir).await {
                        log::debug!("initial pane screen unavailable: {err:#}");
                    }
                    (pane.remote_pane_id, pane as Arc<dyn Pane>)
                }
            });
        futures::future::join_all(pending)
            .await
            .into_iter()
            .collect()
    }

    /// The local mux owns layout: refresh identities, never import or reparent remote tabs.
    pub(super) fn refresh_local_layout(inner: &ClientInner, panes: &ListPanesResponse) {
        let mut present = HashSet::new();
        for entry in panes.tabs.iter().flat_map(leaves) {
            present.insert(entry.pane_id);
            if let Some(pane) = inner
                .remote_to_local_pane_id(entry.pane_id)
                .and_then(|id| Mux::get().get_pane(id))
            {
                if let Some(pane) = pane.downcast_ref::<ClientPane>() {
                    pane.update_remote_tab_id(entry.tab_id);
                }
            }
        }
        let mux = Mux::get();
        for pane in mux.iter_panes() {
            if pane.domain_id() == inner.local_domain_id {
                if let Some(client) = pane.downcast_ref::<ClientPane>() {
                    if !present.contains(&client.remote_pane_id()) {
                        client.ignore_next_kill();
                        mux.remove_pane(pane.pane_id());
                    }
                }
            }
        }
        inner.expire_stale_mappings();
    }
}

/// Source tabs a resync moved panes out of.
#[derive(Default)]
pub(super) struct Relocations(HashSet<TabId>);

impl Relocations {
    /// The pane lives on in `tab`; its old tab only loses it, keeping any other panes there.
    pub(super) fn track(
        &mut self,
        inner: &ClientInner,
        pane: Arc<dyn Pane>,
        remote_tab: TabId,
        tab: TabId,
    ) -> Arc<dyn Pane> {
        let Some(client_pane) = pane.downcast_ref::<ClientPane>() else {
            return pane;
        };
        let prior = client_pane.remote_tab_id();
        if prior != remote_tab {
            let source = inner
                .remote_to_local_tab_id(prior)
                .filter(|id| *id != tab)
                .and_then(|id| Mux::get().get_tab(id));
            if let Some(source) = source.filter(|source| {
                source
                    .iter_panes_ignoring_zoom()
                    .iter()
                    .any(|p| p.pane.pane_id() == pane.pane_id())
            }) {
                source.remove_pane(pane.pane_id());
                self.0.insert(source.tab_id());
            }
        }
        client_pane.update_remote_tab_id(remote_tab);
        pane
    }

    /// After every tree and mapping applies, since applying a tree holds its tab's lock.
    pub(super) fn prune(self) {
        let mux = Mux::get();
        for source_id in self.0 {
            if mux
                .get_tab(source_id)
                .is_some_and(|source| source.iter_panes_ignoring_zoom().is_empty())
            {
                mux.remove_tab(source_id);
            }
        }
    }
}

pub(super) fn leaves(node: &PaneNode) -> Vec<&PaneEntry> {
    match node {
        PaneNode::Empty => vec![],
        PaneNode::Split { left, right, .. } => [leaves(left), leaves(right)].concat(),
        PaneNode::Leaf(entry) => vec![entry],
    }
}

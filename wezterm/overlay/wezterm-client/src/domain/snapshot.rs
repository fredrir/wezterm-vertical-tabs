//! Pane list snapshots: warming new panes and refreshing a locally owned layout.
use crate::domain::{ClientDomain, ClientInner};
use crate::pane::ClientPane;
use codec::ListPanesResponse;
use mux::pane::{Pane, PaneId};
use mux::tab::{PaneEntry, PaneNode};
use mux::Mux;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

impl ClientDomain {
    pub(super) async fn prefetch_new_panes(
        inner: &Arc<ClientInner>,
        panes: &ListPanesResponse,
    ) -> HashMap<PaneId, Arc<dyn Pane>> {
        let pending = panes
            .tabs
            .iter()
            // Only warm replacements/new splits in an existing tab. Initial
            // attachment can render incrementally as before.
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
                async move {
                    if let Err(err) = pane.prefetch().await {
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

    /// The local mux owns layout; refresh identities without importing or reparenting remote tabs, even over a reverse connection.
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

pub(super) fn leaves(node: &PaneNode) -> Vec<&PaneEntry> {
    match node {
        PaneNode::Empty => vec![],
        PaneNode::Split { left, right, .. } => [leaves(left), leaves(right)].concat(),
        PaneNode::Leaf(entry) => vec![entry],
    }
}

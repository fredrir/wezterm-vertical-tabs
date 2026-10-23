//! The server behind this pane: its identity, tab, screen, location and close policy.
use crate::domain::ClientInner;
use crate::pane::clientpane::ClientPane;
use crate::pane::renderable::hydrate_lines;
use codec::{
    GetLines, GetPaneCloseInfo, GetPaneCloseInfoResponse, GetPaneLocation, GetPaneLocationResponse,
    GetPaneRenderableDimensions, SendKeyDownIfBusy,
};
use mux::pane::{CachePolicy, Pane, PaneId};
use mux::tab::TabId;
use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use url::Url;
use wezterm_term::{KeyCode, KeyModifiers, StableRowIndex};

impl ClientPane {
    /// Stable while the mux survives. Pane identities survive adoption into a new backing tab.
    pub fn persistence_identity(&self) -> Option<(String, String)> {
        let session = self.client.session()?;
        let entity = if self.client.local_pane_layout {
            format!("pane:{}", self.remote_pane_id)
        } else {
            format!("tab:{}", self.remote_tab_id())
        };
        Some((session, entity))
    }

    pub(crate) fn update_remote_tab_id(&self, tab_id: TabId) {
        self.remote_tab_id.store(tab_id, Ordering::Relaxed);
    }

    pub(crate) fn remote_tab_id(&self) -> TabId {
        self.remote_tab_id.load(Ordering::Relaxed)
    }

    /// Whether this pane shows `remote_pane_id` of the server behind `inner`, by any route.
    pub(crate) fn shows(&self, inner: &ClientInner, remote_pane_id: PaneId) -> bool {
        self.remote_pane_id == remote_pane_id
            && (self.client.local_domain_id == inner.local_domain_id
                || inner
                    .server()
                    .is_some_and(|server| self.client.server() == Some(server)))
    }

    /// Load the visible screen, directory and owner before this proxy enters the local split tree.
    pub(crate) async fn prefetch(&self, working_dir: Option<Url>) -> anyhow::Result<()> {
        let location = {
            let renderable = self.renderable.lock();
            let mut inner = renderable.inner.borrow_mut();
            let known = working_dir.is_some();
            inner.working_dir = working_dir;
            known.then(|| inner.location.clone())
        };
        let screen = self.prefetch_screen();
        let Some(location) = location else {
            return screen.await;
        };
        futures::future::join(screen, location.load(&self.client, self.remote_pane_id))
            .await
            .0
    }

    async fn prefetch_screen(&self) -> anyhow::Result<()> {
        let state = self
            .client
            .client
            .get_dimensions(GetPaneRenderableDimensions {
                pane_id: self.remote_pane_id,
            })
            .await?;
        let top = state.dimensions.physical_top;
        let rows = state.dimensions.viewport_rows as StableRowIndex;
        let response = self
            .client
            .client
            .get_lines(GetLines {
                pane_id: self.remote_pane_id,
                lines: vec![top..top + rows],
            })
            .await?;
        let lines = hydrate_lines(
            Arc::clone(&self.client),
            self.remote_pane_id,
            response.lines,
        )
        .await;
        self.renderable.lock().inner.borrow_mut().seed(state, lines);
        Ok(())
    }

    /// The owner's latest answer, maybe for an older `cwd`; `None` if pending or a stock server.
    pub fn location(&self) -> Option<GetPaneLocationResponse> {
        self.renderable.lock().inner.borrow().location.get()
    }

    /// An intermediate mux answers for the pane's real owner rather than for itself.
    pub async fn relay_location(&self) -> anyhow::Result<GetPaneLocationResponse> {
        let cwd = self.get_current_working_dir(CachePolicy::AllowStale);
        let cwd = cwd.as_ref().map_or("", |url| url.path());
        if let Some(location) = self.location().filter(|location| location.cwd == cwd) {
            return Ok(location);
        }
        self.client
            .client
            .get_pane_location(GetPaneLocation {
                pane_id: Some(self.remote_pane_id),
            })
            .await
    }

    /// Asked at close time, since what runs in the pane changes; an error means a stock server.
    pub async fn close_info(&self) -> anyhow::Result<GetPaneCloseInfoResponse> {
        self.client
            .client
            .get_pane_close_info(GetPaneCloseInfo {
                pane_id: self.remote_pane_id,
            })
            .await
    }

    /// Whether the owner sent the key; spawn it before later keys so it reaches the server first.
    pub fn key_down_if_busy(
        &self,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> impl Future<Output = anyhow::Result<bool>> + 'static {
        let client = Arc::clone(&self.client);
        let pane_id = self.remote_pane_id;
        async move {
            let event = termwiz::input::KeyEvent { key, modifiers };
            let request = SendKeyDownIfBusy { pane_id, event };
            Ok(client.client.send_key_down_if_busy(request).await?.sent)
        }
    }
}

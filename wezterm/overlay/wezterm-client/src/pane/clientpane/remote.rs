//! Requests the server behind this pane answers: its screen, location and close policy.
use crate::pane::clientpane::ClientPane;
use crate::pane::renderable::hydrate_lines;
use codec::{
    GetLines, GetPaneCloseInfo, GetPaneCloseInfoResponse, GetPaneLocation, GetPaneLocationResponse,
    GetPaneRenderableDimensions,
};
use mux::pane::{CachePolicy, Pane};
use std::sync::Arc;
use wezterm_term::StableRowIndex;

impl ClientPane {
    /// Load the visible screen before this proxy enters the local split tree.
    pub(crate) async fn prefetch(&self) -> anyhow::Result<()> {
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

    /// The owner's latest answer, possibly for an earlier `cwd`; `None` while pending or from a stock server.
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
}

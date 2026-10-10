//! The owner reports its host, home and the pane's Git work tree whenever the directory changes.
use crate::domain::ClientInner;
use codec::{GetPaneLocation, GetPaneLocationResponse};
use mux::pane::PaneId;
use mux::{Mux, MuxNotification};
use parking_lot::Mutex;
use std::sync::Arc;
use wezterm_term::Alert;

#[derive(Default)]
struct State {
    resolved: Option<GetPaneLocationResponse>,
    requests: u64,
}

#[derive(Clone, Default)]
pub struct PaneLocation(Arc<Mutex<State>>);

impl PaneLocation {
    pub fn get(&self) -> Option<GetPaneLocationResponse> {
        self.0.lock().resolved.clone()
    }

    /// Stock servers reject the request, leaving the location unknown.
    pub fn refresh(
        &self,
        client: &Arc<ClientInner>,
        remote_pane_id: PaneId,
        local_pane_id: PaneId,
    ) {
        let location = self.clone();
        let client = Arc::clone(client);
        promise::spawn::spawn(async move {
            if location.load(&client, remote_pane_id).await {
                // Consumers re-read pane metadata on this alert.
                Mux::get().notify(MuxNotification::Alert {
                    pane_id: local_pane_id,
                    alert: Alert::CurrentWorkingDirectoryChanged,
                });
            }
        })
        .detach();
    }

    /// Whether this request's answer became the location.
    pub(crate) async fn load(&self, client: &Arc<ClientInner>, remote_pane_id: PaneId) -> bool {
        let request = {
            let mut state = self.0.lock();
            state.requests += 1;
            state.requests
        };
        let Ok(location) = client
            .client
            .get_pane_location(GetPaneLocation {
                pane_id: Some(remote_pane_id),
            })
            .await
        else {
            return false;
        };
        let mut state = self.0.lock();
        // Overtaken answers are stale; the last one keeps the host until the next lands.
        if state.requests != request {
            return false;
        }
        state.resolved = Some(location);
        true
    }
}

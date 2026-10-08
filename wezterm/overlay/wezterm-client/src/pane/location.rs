//! The owning server resolves a pane's home and Git work tree whenever its directory changes.
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
        let request = {
            let mut state = self.0.lock();
            state.requests += 1;
            state.requests
        };
        let state = Arc::clone(&self.0);
        let client = Arc::clone(client);
        promise::spawn::spawn(async move {
            let Ok(location) = client
                .client
                .get_pane_location(GetPaneLocation {
                    pane_id: remote_pane_id,
                })
                .await
            else {
                return;
            };
            {
                let mut state = state.lock();
                // An answer overtaken by a later directory change is already stale.
                if state.requests != request {
                    return;
                }
                state.resolved = Some(location);
            }
            // Consumers re-read pane metadata on this alert.
            Mux::get().notify(MuxNotification::Alert {
                pane_id: local_pane_id,
                alert: Alert::CurrentWorkingDirectoryChanged,
            });
        })
        .detach();
    }
}

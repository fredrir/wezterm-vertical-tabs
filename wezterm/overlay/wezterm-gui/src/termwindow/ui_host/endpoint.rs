use super::Message;
use crate::termwindow::{TermWindow, TermWindowNotif};
use window::{Window, WindowOps};

/// An async completion belongs to one mux window even if its native window switches workspaces.
#[derive(Clone)]
pub(crate) struct WindowEndpoint {
    window: Window,
    mux_window_id: usize,
}

impl WindowEndpoint {
    pub(crate) fn new(window: Window, mux_window_id: usize) -> Self {
        Self {
            window,
            mux_window_id,
        }
    }

    pub(crate) fn mux_window_id(&self) -> usize {
        self.mux_window_id
    }

    pub(crate) fn window(&self) -> &Window {
        &self.window
    }

    pub(crate) fn send(&self, message: Message) {
        let endpoint = self.clone();
        self.window
            .notify(TermWindowNotif::Apply(Box::new(move |tw| {
                endpoint.deliver(tw, message);
            })));
    }

    /// For a completion whose domain-specific follow-up must run after delivery.
    pub(crate) fn deliver(&self, window: &mut TermWindow, message: Message) {
        window.vtabs_message_for(self.mux_window_id, message);
    }
}

//! Quitting when only detached windows remain, as when the last window closes.
use super::{front_end, GuiFrontEnd};
use ::window::{Connection, ConnectionOps};
use mux::Mux;

impl GuiFrontEnd {
    pub(super) fn only_detached_windows_remain(&self) -> bool {
        let mux = Mux::get();
        let workspace = mux.active_workspace_for_client(&self.client_id);
        !mux.is_empty()
            && mux.iter_windows_in_workspace(&workspace).is_empty()
            && mux
                .iter_workspaces()
                .iter()
                .all(|w| mux::is_hidden_workspace(w) || mux.is_workspace_empty(w))
    }

    /// Quits as if the last window closed, once closing windows finish their own work.
    pub(super) fn quit_once_idle(&self) {
        if !config::configuration().quit_when_all_windows_are_closed {
            return;
        }
        promise::spawn::spawn_into_main_thread(async move {
            while mux::activity::Activity::count() > 0 {
                smol::Timer::after(std::time::Duration::from_millis(50)).await;
            }
            if front_end().only_detached_windows_remain() {
                log::trace!("Only detached windows remain, terminate gui");
                Connection::get().unwrap().terminate_message_loop();
            }
        })
        .detach();
    }
}

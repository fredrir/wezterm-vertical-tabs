//! Replacing a pane's process in place, keeping its split layout.
use crate::activity::Activity;
use crate::domain::DomainState;
use crate::pane::{CachePolicy, Pane, PaneId};
use crate::Mux;
use anyhow::anyhow;
use portable_pty::CommandBuilder;
use std::sync::Arc;
use wezterm_term::TerminalSize;

impl Mux {
    pub async fn replace_pane(
        &self,
        pane_id: PaneId,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
        domain: config::keyassignment::SpawnTabDomain,
        wait_for_ready: bool,
    ) -> anyhow::Result<(Arc<dyn Pane>, TerminalSize)> {
        let _activity = Activity::new();
        let (_, window_id, tab_id) = self
            .resolve_pane_id(pane_id)
            .ok_or_else(|| anyhow!("pane_id {pane_id} invalid"))?;
        let domain = self.resolve_spawn_tab_domain(Some(pane_id), &domain)?;
        if domain.state() == DomainState::Detached {
            domain.attach(Some(window_id)).await?;
        }
        let tab = self
            .get_tab(tab_id)
            .ok_or_else(|| anyhow!("tab {tab_id} disappeared"))?;
        let current = self
            .get_pane(pane_id)
            .ok_or_else(|| anyhow!("pane {pane_id} disappeared"))?;
        let size = tab.pane_size(pane_id)?;
        let cwd = self.resolve_cwd(
            command_dir,
            Some(Arc::clone(&current)),
            domain.domain_id(),
            CachePolicy::FetchImmediate,
        );
        let pane = domain.spawn_pane(size, command, cwd).await?;
        if let Some(config) = current.get_config() {
            pane.set_config(config);
        }
        let result = async {
            self.add_pane(&pane)?;
            if wait_for_ready {
                // Keep the old screen until the replacement has drawn its first
                // prompt. Older shells still work: the wait is bounded.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
                while !pane.copy_user_vars().contains_key("WEZTERM_PANE_READY")
                    && !pane.is_dead()
                    && std::time::Instant::now() < deadline
                {
                    smol::Timer::after(std::time::Duration::from_millis(5)).await;
                }
                anyhow::ensure!(
                    !pane.is_dead(),
                    "replacement pane exited before it was ready"
                );
            }
            anyhow::ensure!(self.get_tab(tab_id).is_some(), "tab {tab_id} disappeared");
            anyhow::ensure!(
                self.get_pane(pane_id).is_some(),
                "pane {pane_id} disappeared"
            );
            tab.replace_pane(pane_id, Arc::clone(&pane))
        }
        .await;
        match result {
            Ok(size) => {
                self.remove_pane(pane_id);
                Ok((pane, size))
            }
            Err(error) => {
                self.remove_pane(pane.pane_id());
                Err(error)
            }
        }
    }
}

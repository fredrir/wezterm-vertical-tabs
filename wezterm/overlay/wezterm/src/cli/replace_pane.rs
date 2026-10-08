use crate::cli::resolve_relative_cwd;
use clap::{Parser, ValueHint};
use config::keyassignment::SpawnTabDomain;
use mux::pane::PaneId;
use portable_pty::cmdbuilder::CommandBuilder;
use std::ffi::OsString;
use wezterm_client::client::Client;

#[derive(Debug, Parser, Clone)]
pub struct ReplacePane {
    /// Pane to replace; defaults to WEZTERM_PANE.
    #[arg(long)]
    pane_id: Option<PaneId>,

    /// Domain for the new shell; defaults to the current pane's domain.
    #[arg(long)]
    domain_name: Option<String>,

    /// Working directory for the new shell.
    #[arg(long, value_hint = ValueHint::DirPath)]
    cwd: Option<OsString>,

    /// Keep the old screen until WEZTERM_PANE_READY is set (up to one second).
    #[arg(long)]
    wait_for_ready: bool,

    /// Command to run instead of the domain's default shell.
    #[arg(value_hint = ValueHint::CommandWithArguments, num_args = 1..)]
    prog: Vec<OsString>,
}

impl ReplacePane {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let pane_id = client.resolve_pane_id(self.pane_id).await?;
        let spawned = client
            .replace_pane(codec::ReplacePane {
                pane_id,
                domain: self.domain_name.map_or(
                    SpawnTabDomain::CurrentPaneDomain,
                    SpawnTabDomain::DomainName,
                ),
                command: if self.prog.is_empty() {
                    None
                } else {
                    Some(CommandBuilder::from_argv(self.prog))
                },
                command_dir: resolve_relative_cwd(self.cwd)?,
                wait_for_ready: self.wait_for_ready,
            })
            .await?;
        println!("{}", spawned.pane_id);
        Ok(())
    }
}

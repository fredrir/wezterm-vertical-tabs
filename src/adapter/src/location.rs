//! The machine that owns a pane describes where its directory sits.
use super::repos::Repos;
use mux::pane::Pane;
use std::sync::Arc;
use wezterm_client::pane::ClientPane;

#[derive(Default)]
pub struct Location {
    pub remote: bool,
    pub os: String,
    /// `None` labels against this host's home.
    pub home: Option<String>,
    pub repo_root: Option<String>,
}

impl Location {
    /// Each pane's owning mux answers, relayed hop by hop like its directory.
    pub fn of(pane: &Arc<dyn Pane>, cwd: &str, local_host: &str, repos: &mut Repos) -> Self {
        let Some(client) = pane.downcast_ref::<ClientPane>() else {
            return Self::unanswered(false, String::new(), cwd, repos);
        };
        let Some(owner) = client.location() else {
            return Self::unanswered(true, String::new(), cwd, repos);
        };
        let remote = !owner.hostname.eq_ignore_ascii_case(local_host);
        // The host outlives a directory change; home and repository wait for the next answer.
        if owner.cwd != cwd {
            return Self::unanswered(remote, owner.os, cwd, repos);
        }
        Self {
            remote,
            os: owner.os,
            home: Some(owner.home),
            repo_root: owner.repo_root,
        }
    }

    /// Without the owner's answer only a directory on this host can be inspected.
    pub fn unanswered(remote: bool, os: String, cwd: &str, repos: &mut Repos) -> Self {
        Self {
            repo_root: (!remote)
                .then(|| repos.root(cwd).map(str::to_owned))
                .flatten(),
            remote,
            os,
            home: None,
        }
    }
}

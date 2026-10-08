//! PDUs this project adds; their ids live in `pdu!`.
use mux::pane::PaneId;
use mux::tab::{PaneEntry, TabId};
use mux::window::WindowId;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ReplacePane {
    pub pane_id: PaneId,
    pub command: Option<CommandBuilder>,
    pub command_dir: Option<String>,
    pub domain: config::keyassignment::SpawnTabDomain,
    pub wait_for_ready: bool,
}

/// A client cannot see what runs in a remote pane, so its owner judges the close.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneCloseInfo {
    pub pane_id: PaneId,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct GetPaneCloseInfoResponse {
    /// The owner's own close policy: skip list, `mux-is-process-stateful`, process tree.
    pub prompt: bool,
    pub process: String,
}

/// Only the machine that owns a pane can see its host, home and Git work tree.
/// Relayed hop by hop; stock servers reject it and keep upstream labels.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetPaneLocation {
    /// `None` asks about the server itself.
    pub pane_id: Option<PaneId>,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct GetPaneLocationResponse {
    /// The directory this answer describes; a stale answer names an older one.
    pub cwd: String,
    pub home: String,
    pub repo_root: Option<String>,
    /// The os-release `ID`, or the platform name where no such file exists.
    pub os: String,
    pub hostname: String,
}

/// The remote panes of a `local_pane_layout` domain that its mux could show.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ListAdoptablePanes {
    pub domain: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ListAdoptablePanesResponse {
    pub panes: Vec<PaneEntry>,
}

/// Shows an existing remote pane in a new tab and takes over its backing tab.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct AdoptPane {
    pub domain: String,
    pub remote_pane_id: PaneId,
    /// A new detached window when omitted.
    pub window_id: Option<WindowId>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct AdoptPaneResponse {
    pub pane_id: PaneId,
    pub tab_id: TabId,
    pub window_id: WindowId,
}

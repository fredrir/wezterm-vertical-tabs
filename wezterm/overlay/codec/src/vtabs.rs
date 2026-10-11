//! PDUs this project adds; their ids live in `pdu!`.
use mux::pane::PaneId;
use mux::tab::{PaneEntry, TabId};
use mux::window::WindowId;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use wezterm_term::TerminalSize;

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ReplacePane {
    pub pane_id: PaneId,
    pub command: Option<CommandBuilder>,
    pub command_dir: Option<String>,
    pub domain: config::keyassignment::SpawnTabDomain,
    pub wait_for_ready: bool,
}

/// Spawns in the pane's domain and directory, in a hidden tab; answered by `SpawnResponse`.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SpawnBesidePane {
    pub pane_id: PaneId,
    pub command: Option<CommandBuilder>,
    pub command_dir: Option<String>,
    pub size: TerminalSize,
    pub workspace: String,
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

/// The owner sends the key only while something runs in the pane, judged as a close is;
/// handled in arrival order, so it stays ahead of the `SendKeyDown`s that follow.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SendKeyDownIfBusy {
    pub pane_id: PaneId,
    pub event: termwiz::input::KeyEvent,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct SendKeyDownIfBusyResponse {
    pub sent: bool,
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

/// Remote panes a mux could show, once per server: through `domains` first, attached when
/// needed, then through every connected `local_pane_layout` domain.
#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct FindAdoptablePanes {
    pub domains: Vec<String>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct FindAdoptablePanesResponse {
    pub panes: Vec<AdoptablePane>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug, Clone)]
pub struct AdoptablePane {
    /// Adopts through this domain.
    pub domain: String,
    pub entry: PaneEntry,
    /// `None` from a stock server.
    pub location: Option<GetPaneLocationResponse>,
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

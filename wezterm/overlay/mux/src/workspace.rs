//! Hidden workspaces: detached panes, and tabs a mux keeps backing another host's panes.

/// Holds detached panes; the GUI never switches to it on its own.
pub const DETACHED_WORKSPACE: &str = "__detached";
/// Prefixes the workspace in which a mux keeps the tabs backing another host's panes.
pub const BACKING_WORKSPACE_PREFIX: &str = "__backing:";

/// The workspace a remote mux puts this host's backing tabs in, so it can hand them back.
pub fn backing_workspace() -> String {
    let host = hostname::get()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let host = host.split('.').next().unwrap_or_default();
    format!("{BACKING_WORKSPACE_PREFIX}{host}")
}

/// Workspaces a GUI never switches to on its own.
pub fn is_hidden_workspace(workspace: &str) -> bool {
    workspace == DETACHED_WORKSPACE || workspace.starts_with(BACKING_WORKSPACE_PREFIX)
}

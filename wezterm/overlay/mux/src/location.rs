//! Where a directory sits on the machine that owns it.
use std::path::Path;

/// Worktrees and submodules keep `.git` as a file, so existence is the test.
pub fn repo_root(cwd: &str) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    Path::new(cwd)
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .and_then(Path::to_str)
        .map(str::to_owned)
}

pub fn home() -> String {
    config::HOME_DIR.to_string_lossy().into_owned()
}

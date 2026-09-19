//! Git repository discovery for sidebar labels. Roots are cached per directory.
use std::{collections::HashMap, path::Path};

const CAPACITY: usize = 256;

#[derive(Default)]
pub struct Repos {
    known: HashMap<String, Option<String>>,
}

impl Repos {
    pub fn root(&mut self, cwd: &str) -> Option<&str> {
        if cwd.is_empty() {
            return None;
        }
        if self.known.len() >= CAPACITY && !self.known.contains_key(cwd) {
            self.known.clear();
        }
        self.known
            .entry(cwd.to_owned())
            .or_insert_with(|| git_root(cwd))
            .as_deref()
    }
}

// Worktrees and submodules keep `.git` as a file, so existence is the test.
fn git_root(cwd: &str) -> Option<String> {
    Path::new(cwd)
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .and_then(Path::to_str)
        .map(str::to_owned)
}

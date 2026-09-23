//! Git repository discovery for local sidebar labels. Roots are cached per directory.
use std::collections::HashMap;

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
            .or_insert_with(|| mux::location::repo_root(cwd))
            .as_deref()
    }
}

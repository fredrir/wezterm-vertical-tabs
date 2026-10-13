//! Retained hook metadata and bounded scheduling for one window.
use super::*;

pub(super) struct Hooks {
    pub metadata: HashMap<u64, core::Tab>,
    pub queued: HashMap<u64, core::Tab>,
    pub pending: bool,
    pub enabled: bool,
    pub window_dirty: bool,
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            metadata: HashMap::new(),
            queued: HashMap::new(),
            pending: false,
            enabled: false,
            window_dirty: true,
        }
    }
}

impl Hooks {
    pub fn configure(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.metadata.clear();
        self.window_dirty = true;
    }

    pub fn observe(&mut self, tabs: &[core::Tab]) {
        let ids: std::collections::HashSet<_> = tabs.iter().map(|tab| tab.id).collect();
        self.metadata.retain(|id, _| ids.contains(id));
        if self.enabled {
            for tab in tabs {
                if self.metadata.get(&tab.id) != Some(tab) {
                    self.metadata.insert(tab.id, tab.clone());
                    self.queued.insert(tab.id, tab.clone());
                }
            }
        }
    }

    pub fn retry(&mut self) {
        self.window_dirty = true;
        self.queued.extend(self.metadata.clone());
    }

    pub fn schedule(&mut self, window: Option<&Window>, app: &WindowApp, window_id: usize) {
        if !self.enabled {
            self.queued.clear();
            self.window_dirty = false;
            return;
        }
        if self.pending || (self.queued.is_empty() && !self.window_dirty) {
            return;
        }
        if let Some(window) = window {
            self.queued
                .retain(|id, _| app.model().tabs.contains_key(id));
            let mut ids = self.queued.keys().copied().collect::<Vec<_>>();
            ids.sort_unstable();
            let mut batch = Vec::with_capacity(ids.len().min(lua::MAX_HOOK_TABS));
            for id in ids.into_iter().take(lua::MAX_HOOK_TABS) {
                if let Some(tab) = self.queued.remove(&id) {
                    batch.push((app.hook_token(Some(tab.id)), tab));
                }
            }
            self.pending = true;
            self.window_dirty = false;
            lua::hooks(
                window.clone(),
                batch,
                lua::WindowHookContext::from_app(app),
                window_id,
            );
        }
    }
}

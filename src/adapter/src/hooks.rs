//! Retained hook metadata and bounded scheduling for one window.
use super::*;

pub(super) struct Hooks {
    metadata: HashMap<u64, core::HostTab>,
    queued: HashMap<u64, core::HostTab>,
    pending: bool,
    enabled: bool,
    window_dirty: bool,
}

pub(super) struct Batch {
    pub tabs: Vec<(app::HookToken, core::Tab)>,
    pub window: lua::WindowHookContext,
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
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn configure(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.metadata.clear();
        self.queued.clear();
        self.window_dirty = true;
    }

    pub fn observe(&mut self, tabs: &[core::HostTab]) {
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

    pub fn changed(&mut self, changed: bool) {
        self.window_dirty |= changed;
    }

    pub fn complete(&mut self, accepted: bool, selection_changed: bool) {
        self.pending = false;
        if !accepted {
            self.window_dirty = true;
            self.queued.extend(self.metadata.clone());
        }
        self.changed(selection_changed);
    }

    /// Claimed before dispatch so no second callback starts while this batch is in flight.
    pub fn take_batch(&mut self, app: &WindowApp) -> Option<Batch> {
        if !self.enabled {
            self.queued.clear();
            self.window_dirty = false;
            return None;
        }
        if self.pending || (self.queued.is_empty() && !self.window_dirty) {
            return None;
        }
        self.queued
            .retain(|id, _| app.model().tabs.contains_key(id));
        let mut ids = self.queued.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        let mut tabs = Vec::with_capacity(ids.len().min(lua::MAX_HOOK_TABS));
        for id in ids.into_iter().take(lua::MAX_HOOK_TABS) {
            if let Some(tab) = self.queued.remove(&id) {
                // Lua hooks keep the full Tab wire shape, defaults included.
                tabs.push((app.hook_token(Some(tab.id)), tab.into()));
            }
        }
        self.pending = true;
        self.window_dirty = false;
        Some(Batch {
            tabs,
            window: lua::WindowHookContext::from_app(app),
        })
    }
}

#[cfg(test)]
#[path = "../tests/hooks.rs"]
mod tests;

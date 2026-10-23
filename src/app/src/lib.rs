//! Per-window, single-event-loop coordinator for the statically linked UI.
mod commands;
mod config;
mod hooks;
mod managed;
mod persistence;
mod spawn;
mod transfer;
mod types;

pub use vtabs_core as core;
pub use vtabs_store as store;
pub use vtabs_ui as ui;

pub use hooks::{HookResult, HookToken};
pub use spawn::SpawnToken;
pub use transfer::WindowTransfer;
pub use types::{Command, HostSnapshot, Metrics, Projection, Update};

use core::{Error, HostCommand, Intent, Model, Tab, TabId};
use std::{collections::BTreeMap, time::Duration};
use ui::{FrameUpdate, SidebarUi, UiInput};

pub struct WindowApp {
    model: Model,
    ui: SidebarUi,
    metrics: Metrics,
    host_revision: Option<u64>,
    host_active: Option<TabId>,
    configuration_epoch: u64,
    focused: bool,
    now: Duration,
    storage: persistence::Persistence,
    errors: Vec<String>,
    teardown: bool,
    selection_epoch: u64,
    spawn_sequence: u64,
    pending_spawns: BTreeMap<u64, SpawnToken>,
    configured_spaces: Vec<core::Space>,
    configured_templates: Vec<core::SpaceTemplate>,
    managed: managed::ManagedFile,
    geometry_revision: u64,
}

impl Default for WindowApp {
    fn default() -> Self {
        Self::new("default")
    }
}
impl WindowApp {
    pub fn new(profile: impl Into<String>) -> Self {
        let mut model = Model::new(profile);
        static INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        model.set_space_namespace(format!("{:x}{epoch:x}{sequence:x}", std::process::id()));
        let storage = persistence::Persistence::new(&model);
        Self {
            model,
            ui: SidebarUi::new(),
            metrics: Metrics::default(),
            host_revision: None,
            host_active: None,
            configuration_epoch: 0,
            focused: true,
            now: Duration::ZERO,
            storage,
            errors: Vec::new(),
            teardown: false,
            selection_epoch: 0,
            spawn_sequence: 0,
            pending_spawns: BTreeMap::new(),
            configured_spaces: Vec::new(),
            configured_templates: Vec::new(),
            managed: managed::ManagedFile::default(),
            geometry_revision: 0,
        }
    }
    pub fn model(&self) -> &Model {
        &self.model
    }
    pub fn set_launch(&mut self, tab_id: TabId, launch: core::LaunchSpec) -> Result<(), Error> {
        self.model.set_launch(tab_id, launch)
    }
    pub fn set_home(&mut self, home: Option<String>) {
        self.model.set_home(home);
    }
    pub fn ui(&self) -> &SidebarUi {
        &self.ui
    }
    pub fn ui_mut(&mut self) -> &mut SidebarUi {
        &mut self.ui
    }
    pub fn metrics(&self) -> Metrics {
        self.metrics
    }
    pub fn now(&self) -> Duration {
        self.now
    }
    pub fn buffer(&self) -> &ui::Buffer {
        self.ui.buffer()
    }
    pub fn projection(&self) -> Projection<'_> {
        Projection {
            visible: self.model.visible_ids(),
            active: self.model.selected_tab,
        }
    }
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }
    pub fn is_modal(&self) -> bool {
        self.ui.is_modal()
    }
    /// Fast resize path: no tab metadata cloning, routing, persistence, or host commands.
    pub fn resize(&mut self, metrics: Metrics) -> Result<Update, Error> {
        validate_metrics(metrics)?;
        if self.set_metrics(metrics) {
            self.redraw();
        }
        Ok(Update::default())
    }
    pub fn set_focus(&mut self, focused: bool) {
        if self.focused != focused {
            self.focused = focused;
            let _ = self.ui.event(&self.model, UiInput::Focus(focused));
        }
    }

    pub fn update(&mut self, snapshot: HostSnapshot) -> Result<Update, Error> {
        if self.teardown {
            return Ok(Update::default());
        }
        if self.host_revision.is_some_and(|r| snapshot.revision < r) {
            return Err(Error("Stale snapshot".into()));
        }
        validate_metrics(snapshot.metrics)?;
        self.ui.prepare(&self.model, self.now);
        let old_active = self.model.selected_tab;
        let config_changed = self.configuration_epoch != snapshot.configuration_epoch;
        let incoming_spawn = !self.pending_spawns.is_empty()
            && snapshot
                .active_tab
                .is_some_and(|id| !self.model.tabs.contains_key(&id));
        let follow = !incoming_spawn
            && (self.host_revision.is_none() || self.host_active != snapshot.active_tab);
        self.model
            .reconcile(snapshot.tabs, snapshot.active_tab, follow)?;
        self.storage.reconcile_tabs(&mut self.model, self.now)?;
        self.ui.prepare(&self.model, self.now);
        // The host activated another tab; a closed tab beneath the page is not a switch away.
        if follow
            && old_active != self.model.selected_tab
            && old_active.is_some_and(|id| self.model.tabs.contains_key(&id))
        {
            self.ui.hide_settings();
        }
        self.host_active = snapshot.active_tab;
        self.host_revision = Some(snapshot.revision);
        self.configuration_epoch = snapshot.configuration_epoch;
        let grid_changed = self.set_metrics(snapshot.metrics);
        self.set_focus(snapshot.focused);
        if grid_changed || config_changed {
            self.redraw();
        }
        if old_active != self.model.selected_tab {
            self.selection_epoch = self.selection_epoch.wrapping_add(1);
        }
        // A close can leave another space's tab active; the host then activates our successor.
        let commands = if old_active.is_some_and(|id| !self.model.tabs.contains_key(&id))
            && self.model.selected_tab != self.host_active
        {
            self.model
                .selected_tab
                .map(|id| vec![Command::Host(HostCommand::Activate(id))])
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Update { commands })
    }

    pub fn dispatch(&mut self, intent: Intent) -> Result<Update, Error> {
        if self.teardown {
            return Ok(Update::default());
        }
        self.ui.prepare(&self.model, self.now);
        let old_active = self.model.selected_tab;
        let old_space = self.model.selected_space.clone();
        let activates = matches!(
            intent,
            Intent::ActivateTab(_)
                | Intent::ActivateIndex(_)
                | Intent::ActivateRelative { .. }
                | Intent::ActivateLast
        );
        let transition = self.model.dispatch(intent)?;
        self.ui.prepare(&self.model, self.now);
        // Every activation leaves the settings page, whichever surface asked for it.
        if activates {
            self.ui.hide_settings();
        }
        if old_active != self.model.selected_tab || old_space != self.model.selected_space {
            self.selection_epoch = self.selection_epoch.wrapping_add(1);
        }
        if transition.durable_changed {
            self.storage.observe(&self.model, self.now);
            self.managed.touch();
        }
        let commands = transition.commands.into_iter().map(Command::Host).collect();
        Ok(Update { commands })
    }
    pub fn input(&mut self, input: UiInput) -> Result<Update, Error> {
        if self.teardown {
            return Ok(Update::default());
        }
        let intents = self.ui.event(&self.model, input);
        self.run(intents)
    }
    /// Host key assignments reach the settings tab through the same index as shortcuts.
    pub fn activate_index(&mut self, index: isize) -> Result<Update, Error> {
        let mut intents = Vec::new();
        self.ui.activate_index(&self.model, index, &mut intents);
        self.run(intents)
    }
    pub fn activate_relative(&mut self, delta: isize, wrap: bool) -> Result<Update, Error> {
        let mut intents = Vec::new();
        self.ui
            .activate_relative(&self.model, delta, wrap, &mut intents);
        self.run(intents)
    }
    pub fn move_settings(&mut self, index: usize) {
        self.ui.move_settings(&self.model, index);
    }
    pub fn move_settings_relative(&mut self, delta: isize) {
        self.ui.move_settings_relative(&self.model, delta);
    }
    pub fn prepare_render(&mut self, now: Duration) {
        self.now = now;
        if !self.teardown {
            self.ui.prepare(&self.model, now);
        }
    }
    pub fn render(&mut self, now: Duration) -> Option<FrameUpdate> {
        self.prepare_render(now);
        if self.teardown {
            return None;
        }
        self.ui.render(
            &self.model,
            ui::Rect::new(0, 0, self.metrics.cols, self.metrics.rows),
            now,
        )
    }
    pub fn next_deadline(&self) -> Option<Duration> {
        if self.teardown {
            return None;
        }
        self.ui
            .next_deadline()
            .into_iter()
            .chain(self.storage.deadline())
            .min()
    }
    pub fn open_settings(&mut self) {
        self.ui.prepare(&self.model, self.now);
        self.ui.open_settings();
    }
    pub fn open_create_space(&mut self) {
        self.ui.open_create_space();
    }
    pub fn open_tab_navigator(&mut self) {
        self.ui.open_tab_navigator(&self.model);
    }
    pub fn open_tab_actions(&mut self) {
        self.ui.open_tab_actions(&self.model);
    }
    pub fn open_space_actions(&mut self) {
        self.ui.open_space_actions(&self.model);
    }
    pub fn set_foreign_tabs(&mut self, tabs: Vec<ui::ForeignTab>) {
        self.ui.set_foreign_tabs(tabs);
    }
    pub fn set_adoptable_panes(&mut self, panes: Vec<ui::AdoptablePane>) {
        self.ui.set_adoptable_panes(panes);
    }
    /// A tab that returns under a new ID takes back the place and name it left with.
    pub fn restore_tab(&mut self, id: TabId, previous: &Tab) -> Result<Update, Error> {
        self.model.restore_tab_membership(
            id,
            &previous.space_id,
            previous.manual_assignment,
            previous.pinned,
            previous.folder_id.as_deref(),
        )?;
        self.storage.observe(&self.model, self.now);
        match previous.title_override.clone() {
            Some(title) => self.dispatch(Intent::RenameTab { id, title }),
            None => Ok(Update::default()),
        }
    }
    /// Stops interaction and effects; pending storage writes may still be drained.
    pub fn teardown(&mut self) {
        self.teardown = true;
        self.ui.dismiss();
        self.ui.cancel_effects();
        self.storage.retry(self.now);
    }
    /// Returns whether the cell grid changed.
    fn set_metrics(&mut self, metrics: Metrics) -> bool {
        if self.metrics != metrics {
            self.geometry_revision = self.geometry_revision.wrapping_add(1);
        }
        let grid_changed = (self.metrics.cols, self.metrics.rows) != (metrics.cols, metrics.rows);
        self.metrics = metrics;
        grid_changed
    }
    fn redraw(&mut self) {
        self.ui.cancel_effects();
        self.ui.invalidate();
    }
}

fn validate_metrics(metrics: Metrics) -> Result<(), Error> {
    if metrics.cell_width <= 0.
        || metrics.cell_height <= 0.
        || !metrics.cell_width.is_finite()
        || !metrics.cell_height.is_finite()
        || !metrics.dpi.is_finite()
        || metrics.dpi <= 0.
    {
        Err(Error("Invalid font metrics".into()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/folders.rs"]
mod folders_tests;

#[cfg(test)]
#[path = "../tests/lifecycle.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "../tests/managed.rs"]
mod managed_tests;

#[cfg(test)]
#[path = "../tests/persistence.rs"]
mod persistence_tests;

#[cfg(test)]
#[path = "../tests/tab_persistence.rs"]
mod tab_persistence_tests;

#[cfg(test)]
#[path = "../tests/transfer.rs"]
mod transfer_tests;

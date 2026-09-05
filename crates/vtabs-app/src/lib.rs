//! Per-window, single-event-loop coordinator for the statically linked native UI.
mod persistence;

pub use vtabs_core as core;
pub use vtabs_store as store;
pub use vtabs_ui as ui;

use core::{Error, HostCommand, Intent, Model, Tab, TabId};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};
use ui::{FrameUpdate, NativeUiAction, SidebarUi, UiInput, UiIntent};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub dpi: f32,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            cols: 32,
            rows: 40,
            pixel_width: 256,
            pixel_height: 640,
            cell_width: 8.,
            cell_height: 16.,
            dpi: 96.,
        }
    }
}

/// Revision belongs to the host, and covers topology, active ID, geometry and configuration.
#[derive(Clone, Debug)]
pub struct NativeSnapshot {
    pub revision: u64,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<TabId>,
    pub metrics: Metrics,
    pub focused: bool,
    pub configuration_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Refresh,
    Host(HostCommand),
    SetClipboard(String),
    RequestClipboard,
}
#[derive(Clone, Debug, Default)]
pub struct Update {
    pub commands: Vec<Command>,
    pub model_changed: bool,
    pub layout_changed: bool,
    pub projection_changed: bool,
}
impl Update {
    fn merge(&mut self, other: Self) {
        self.commands.extend(other.commands);
        self.model_changed |= other.model_changed;
        self.layout_changed |= other.layout_changed;
        self.projection_changed |= other.projection_changed;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Projection<'a> {
    pub revision: u64,
    pub host_revision: u64,
    pub visible: &'a [TabId],
    pub active: Option<TabId>,
}

/// Tokens prevent an async semantic completion from changing a newer native view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HookToken {
    pub host_revision: u64,
    pub model_revision: u64,
    pub configuration_epoch: u64,
    pub tab_id: Option<TabId>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum HookResult {
    Route(Option<String>),
    Title(String),
    Filter(bool),
    Theme(BTreeMap<String, Value>),
    Footer(String),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpawnToken {
    pub request_id: u64,
    pub space_id: String,
    #[serde(default)]
    pub folder_id: Option<String>,
    selection_epoch: u64,
}

/// Serialized only for an explicit in-GUI move. No content or pane layout is represented.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowTransfer {
    pub version: u32,
    pub profile: String,
    pub private: bool,
    pub spaces: Vec<core::Space>,
    #[serde(default)]
    pub folders: Vec<core::Folder>,
    pub templates: Vec<core::SpaceTemplate>,
    pub preferences: BTreeMap<String, Value>,
    pub configuration: BTreeMap<String, Value>,
    pub configured_spaces: Vec<core::Space>,
    pub configured_templates: Vec<core::SpaceTemplate>,
    pub tab: core::Tab,
}

pub struct WindowApp {
    model: Model,
    ui: SidebarUi,
    metrics: Metrics,
    host_revision: Option<u64>,
    native_active: Option<TabId>,
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
    geometry_revision: u64,
}

impl Default for WindowApp {
    fn default() -> Self {
        Self::new("default", false)
    }
}
impl WindowApp {
    pub fn new(profile: impl Into<String>, private: bool) -> Self {
        let mut model = Model::new(profile, private);
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
            native_active: None,
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
            geometry_revision: 0,
        }
    }
    pub fn model(&self) -> &Model {
        &self.model
    }
    /// Called by generic provider initialization before accepting a new window snapshot.
    pub fn set_private(&mut self, private: bool) {
        if self.model.private != private {
            self.model.set_private(private);
            self.storage = persistence::Persistence::new(&self.model);
            self.ui.invalidate();
        }
    }
    pub fn set_launch(&mut self, tab_id: TabId, launch: core::LaunchSpec) -> Result<(), Error> {
        self.model.set_launch(tab_id, launch)
    }
    pub fn acknowledge_tab_departure(&mut self, tab_id: TabId) {
        if self.model.acknowledge_tab_departure(tab_id) {
            self.ui.invalidate();
        }
    }
    pub fn export_transfer(&self, id: TabId) -> Result<WindowTransfer, Error> {
        let tab = self
            .model
            .tabs
            .get(&id)
            .ok_or_else(|| Error("Moved tab no longer exists".into()))?;
        Ok(WindowTransfer {
            version: 1,
            profile: self.model.profile.clone(),
            private: self.model.private,
            spaces: self.model.spaces.clone(),
            folders: self.model.folders.clone(),
            templates: self.model.templates.clone(),
            preferences: self.model.persisted_settings().clone(),
            configuration: self.model.configured_settings().clone(),
            configured_spaces: self.configured_spaces.clone(),
            configured_templates: self.configured_templates.clone(),
            tab: tab.clone(),
        })
    }
    pub fn import_transfer_before_snapshot(
        &mut self,
        transfer: WindowTransfer,
    ) -> Result<(), Error> {
        if transfer.version != 1 {
            return Err(Error("Unsupported window transfer contract".into()));
        }
        if !self.model.tabs.is_empty() {
            return Err(Error(
                "Window transfer must initialize an empty native window".into(),
            ));
        }
        if transfer.profile.is_empty()
            || transfer.profile.len() > 128
            || transfer.profile.chars().any(char::is_control)
        {
            return Err(Error("Invalid transferred profile".into()));
        }
        let mut candidate = WindowApp::new(transfer.profile, transfer.private);
        candidate
            .model
            .load_catalog(transfer.spaces, transfer.templates)?;
        candidate.model.load_folders(transfer.folders)?;
        candidate.model.load_preferences(transfer.preferences)?;
        candidate.model.apply_config(transfer.configuration)?;
        candidate.configured_spaces = transfer.configured_spaces;
        candidate.configured_templates = transfer.configured_templates;
        candidate.merge_configured_catalog()?;
        let id = transfer.tab.id;
        candidate
            .model
            .reconcile(vec![transfer.tab], Some(id), true)?;
        candidate.storage = persistence::Persistence::new(&candidate.model);
        // Preserve current public edits while a source-window helper is still draining.
        candidate.storage.observe(&candidate.model, Duration::ZERO);
        candidate
            .ui
            .set_config_owned(candidate.model.config_owned.iter().cloned());
        *self = candidate;
        Ok(())
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
    pub fn storage_deadline(&self) -> Option<Duration> {
        self.storage.deadline()
    }
    pub fn buffer(&self) -> &ui::Buffer {
        self.ui.buffer()
    }
    pub fn projection(&self) -> Projection<'_> {
        Projection {
            revision: self.model.revision,
            host_revision: self.host_revision.unwrap_or(0),
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
        let changed = self.metrics != metrics;
        if changed {
            self.geometry_revision = self.geometry_revision.wrapping_add(1);
        }
        if (self.metrics.cols, self.metrics.rows) != (metrics.cols, metrics.rows) {
            self.ui.cancel_effects();
            self.ui.invalidate();
        }
        self.metrics = metrics;
        Ok(Update {
            layout_changed: changed,
            ..Update::default()
        })
    }
    pub fn set_focus(&mut self, focused: bool) {
        if self.focused != focused {
            self.focused = focused;
            let _ = self.ui.event(&self.model, UiInput::Focus(focused));
        }
    }

    pub fn update(&mut self, snapshot: NativeSnapshot) -> Result<Update, Error> {
        if self.teardown {
            return Ok(Update::default());
        }
        if self.host_revision.is_some_and(|r| snapshot.revision < r) {
            return Err(Error("Stale native snapshot".into()));
        }
        validate_metrics(snapshot.metrics)?;
        let old_projection = self.model.projection_revision();
        let old_active = self.model.selected_tab;
        let grid_changed = (self.metrics.cols, self.metrics.rows)
            != (snapshot.metrics.cols, snapshot.metrics.rows);
        let layout_changed = self.metrics != snapshot.metrics;
        if layout_changed {
            self.geometry_revision = self.geometry_revision.wrapping_add(1);
        }
        let config_changed = self.configuration_epoch != snapshot.configuration_epoch;
        let incoming_spawn = !self.pending_spawns.is_empty()
            && snapshot
                .active_tab
                .is_some_and(|id| !self.model.tabs.contains_key(&id));
        let follow = !incoming_spawn
            && (self.host_revision.is_none() || self.native_active != snapshot.active_tab);
        let mut model_changed = self
            .model
            .reconcile(snapshot.tabs, snapshot.active_tab, follow)?;
        model_changed |= self.storage.restore_discovered(&mut self.model)?;
        self.native_active = snapshot.active_tab;
        self.host_revision = Some(snapshot.revision);
        self.configuration_epoch = snapshot.configuration_epoch;
        self.metrics = snapshot.metrics;
        if self.focused != snapshot.focused {
            self.focused = snapshot.focused;
            let _ = self.ui.event(&self.model, UiInput::Focus(snapshot.focused));
        }
        if grid_changed || config_changed {
            self.ui.cancel_effects();
            self.ui.invalidate();
        }
        let projection_changed = old_projection != self.model.projection_revision()
            || old_active != self.model.selected_tab;
        if old_active != self.model.selected_tab {
            self.selection_epoch = self.selection_epoch.wrapping_add(1);
        }
        // Native close may leave another space's tab active. The visible successor is the
        // application decision; the host runs this command after releasing its mux locks.
        let commands = if old_active.is_some_and(|id| !self.model.tabs.contains_key(&id))
            && self.model.selected_tab != self.native_active
        {
            self.model
                .selected_tab
                .map(|id| vec![Command::Host(HostCommand::Activate(id))])
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if model_changed {
            self.storage.observe_session(&self.model, self.now);
        }
        Ok(Update {
            commands,
            model_changed,
            layout_changed,
            projection_changed,
        })
    }

    pub fn dispatch(&mut self, intent: Intent) -> Result<Update, Error> {
        if self.teardown {
            return Ok(Update::default());
        }
        let old_projection = self.model.projection_revision();
        let old_active = self.model.selected_tab;
        let old_space = self.model.selected_space.clone();
        let transition = self.model.dispatch(intent)?;
        if old_active != self.model.selected_tab || old_space != self.model.selected_space {
            self.selection_epoch = self.selection_epoch.wrapping_add(1);
        }
        if transition.durable_changed {
            self.storage.observe(&self.model, self.now);
        }
        let commands = transition.commands.into_iter().map(Command::Host).collect();
        Ok(Update {
            commands,
            model_changed: transition.model_changed,
            layout_changed: transition.layout_changed,
            projection_changed: old_projection != self.model.projection_revision()
                || old_active != self.model.selected_tab,
        })
    }
    pub fn input(&mut self, input: UiInput) -> Result<Update, Error> {
        if self.teardown {
            return Ok(Update::default());
        }
        let intents = self.ui.event(&self.model, input);
        let mut update = Update::default();
        for intent in intents {
            match intent {
                UiIntent::Refresh => {
                    self.refresh_storage();
                    update.commands.push(Command::Refresh);
                }
                UiIntent::Domain(intent) => update.merge(self.dispatch(intent)?),
                UiIntent::SetClipboard(text) => update.commands.push(Command::SetClipboard(text)),
                UiIntent::RequestClipboard => update.commands.push(Command::RequestClipboard),
                UiIntent::Native(NativeUiAction::Custom(action)) => update
                    .commands
                    .push(Command::Host(HostCommand::CustomAction(action))),
                UiIntent::Native(NativeUiAction::MoveTabToNewWindow(id)) => {
                    self.model
                        .tabs
                        .get(&id)
                        .ok_or_else(|| Error("Tab no longer exists".into()))?;
                    update
                        .commands
                        .push(Command::Host(HostCommand::MoveTabToNewWindow(id)));
                }
            }
        }
        Ok(update)
    }
    pub fn render(&mut self, now: Duration) -> Option<FrameUpdate> {
        self.now = now;
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
        self.ui.open_settings();
    }
    pub fn open_create_space(&mut self) {
        self.ui.open_create_space();
    }
    pub fn open_tab_navigator(&mut self) {
        self.ui.open_tab_navigator(&self.model);
    }

    /// Call after the spawned tab exists in an accepted snapshot. Identity is explicit; never
    /// infer the spawn result by taking whichever tab is active when a future completes.
    pub fn spawn_completed(&mut self, tab_id: TabId, space_id: &str) -> Result<Update, Error> {
        self.model.assign_spawn(tab_id, space_id)?;
        self.storage.observe_session(&self.model, self.now);
        Ok(Update {
            commands: vec![Command::Host(HostCommand::Activate(tab_id))],
            model_changed: true,
            projection_changed: true,
            layout_changed: false,
        })
    }
    pub fn reserve_spawn(&mut self) -> SpawnToken {
        self.spawn_sequence = self.spawn_sequence.wrapping_add(1);
        let token = SpawnToken {
            request_id: self.spawn_sequence,
            space_id: self.model.selected_space.clone(),
            folder_id: None,
            selection_epoch: self.selection_epoch,
        };
        self.pending_spawns.insert(token.request_id, token.clone());
        token
    }
    pub fn reserve_spawn_in(&mut self, space_id: &str) -> Result<SpawnToken, Error> {
        if !self.model.spaces.iter().any(|s| s.id == space_id) {
            return Err(Error("Spawn space no longer exists".into()));
        }
        let mut token = self.reserve_spawn();
        token.space_id = space_id.into();
        self.pending_spawns.insert(token.request_id, token.clone());
        Ok(token)
    }
    pub fn reserve_spawn_in_folder(&mut self, folder_id: &str) -> Result<SpawnToken, Error> {
        let space = self
            .model
            .folders
            .iter()
            .find(|folder| folder.id == folder_id)
            .map(|folder| folder.space_id.clone())
            .ok_or_else(|| Error("Spawn folder no longer exists".into()))?;
        let mut token = self.reserve_spawn_in(&space)?;
        token.folder_id = Some(folder_id.into());
        self.pending_spawns.insert(token.request_id, token.clone());
        Ok(token)
    }
    pub fn cancel_spawn(&mut self, token: &SpawnToken) {
        if self.pending_spawns.get(&token.request_id) == Some(token) {
            self.pending_spawns.remove(&token.request_id);
        }
    }
    pub fn spawn_completed_with_token(
        &mut self,
        tab_id: TabId,
        token: SpawnToken,
    ) -> Result<Update, Error> {
        if self.pending_spawns.get(&token.request_id) != Some(&token) || self.teardown {
            return Ok(Update::default());
        }
        self.pending_spawns.remove(&token.request_id);
        let space = if self.model.spaces.iter().any(|s| s.id == token.space_id) {
            token.space_id
        } else {
            self.model.selected_space.clone()
        };
        self.model.assign_spawn_membership(tab_id, &space)?;
        if let Some(folder_id) = token.folder_id
            && self
                .model
                .folders
                .iter()
                .any(|folder| folder.id == folder_id && folder.space_id == space)
        {
            self.model.dispatch(Intent::AssignFolder {
                tab_id,
                folder_id: Some(folder_id),
            })?;
        }
        let mut out = Update {
            model_changed: true,
            projection_changed: true,
            ..Update::default()
        };
        if token.selection_epoch == self.selection_epoch {
            out.merge(self.dispatch(Intent::ActivateTab(tab_id))?);
        } else if let Some(id) = self
            .model
            .selected_tab
            .filter(|id| Some(*id) != self.native_active)
        {
            out.commands.push(Command::Host(HostCommand::Activate(id)));
        }
        self.storage.observe_session(&self.model, self.now);
        Ok(out)
    }
    pub fn apply_config(
        &mut self,
        values: BTreeMap<String, Value>,
        epoch: u64,
    ) -> Result<Update, Error> {
        if epoch < self.configuration_epoch {
            return Err(Error("Stale configuration".into()));
        }
        self.model.apply_config(values)?;
        self.configuration_epoch = epoch;
        self.ui
            .set_config_owned(self.model.config_owned.iter().cloned());
        self.ui.cancel_effects();
        self.ui.invalidate();
        Ok(Update {
            model_changed: true,
            layout_changed: true,
            ..Update::default()
        })
    }
    pub fn configure_spaces(
        &mut self,
        spaces: Vec<core::Space>,
        templates: Vec<core::SpaceTemplate>,
    ) -> Result<Update, Error> {
        let mut candidate = self.model.clone();
        candidate.load_catalog(spaces.clone(), templates.clone())?;
        let initial = self.model.tabs.is_empty()
            && self.model.spaces.len() == 1
            && self.model.spaces[0].id == core::DEFAULT_SPACE;
        self.configured_spaces = spaces;
        self.configured_templates = templates;
        if initial {
            self.model.load_catalog(
                self.configured_spaces.clone(),
                self.configured_templates.clone(),
            )?;
        } else {
            self.merge_configured_catalog()?;
        }
        self.ui.invalidate();
        Ok(Update {
            model_changed: true,
            projection_changed: true,
            ..Update::default()
        })
    }
    fn merge_configured_catalog(&mut self) -> Result<(), Error> {
        if self.configured_spaces.is_empty() && self.configured_templates.is_empty() {
            return Ok(());
        }
        let mut spaces = self.configured_spaces.clone();
        spaces.extend(
            self.model
                .spaces
                .iter()
                .filter(|space| !self.configured_spaces.iter().any(|s| s.id == space.id))
                .cloned(),
        );
        let mut templates = self.configured_templates.clone();
        templates.extend(
            self.model
                .templates
                .iter()
                .filter(|template| {
                    !self
                        .configured_templates
                        .iter()
                        .any(|t| t.id == template.id)
                })
                .cloned(),
        );
        if spaces != self.model.spaces || templates != self.model.templates {
            self.model.load_catalog(spaces, templates)?;
        }
        Ok(())
    }
    /// Lua registration sends a JSON-compatible table. The Rust schema validates every
    /// setting before mutation; callbacks are registered separately and return HookResult.
    pub fn config(&mut self, value: Value) -> Result<Update, Error> {
        #[derive(serde::Deserialize)]
        #[serde(default, deny_unknown_fields)]
        #[derive(Default)]
        struct Config {
            settings: BTreeMap<String, Value>,
            spaces: Option<Vec<core::Space>>,
            templates: Vec<core::SpaceTemplate>,
            profile: Option<String>,
        }

        let config: Config = serde_json::from_value(value)
            .map_err(|e| Error(format!("Native UI configuration: {e}")))?;
        if self.model.profile.is_empty()
            || self.model.profile.len() > 128
            || self.model.profile.chars().any(char::is_control)
        {
            return Err(Error("Profile must contain 1–128 printable bytes".into()));
        }
        if config.profile.is_some_and(|p| p != self.model.profile) {
            return Err(Error(
                "Profile is chosen when creating the window application".into(),
            ));
        }
        // Validate both layers on a detached model before publishing either.
        let mut candidate = self.model.clone();
        candidate.apply_config(config.settings)?;
        if let Some(spaces) = &config.spaces {
            let initial = self.model.tabs.is_empty()
                && self.model.spaces.len() == 1
                && self.model.spaces[0].id == core::DEFAULT_SPACE;
            let mut merged = spaces.clone();
            if !initial {
                merged.extend(
                    self.model
                        .spaces
                        .iter()
                        .filter(|s| !spaces.iter().any(|v| v.id == s.id))
                        .cloned(),
                );
            }
            let mut templates = config.templates.clone();
            templates.extend(
                self.model
                    .templates
                    .iter()
                    .filter(|t| !config.templates.iter().any(|v| v.id == t.id))
                    .cloned(),
            );
            candidate.load_catalog(merged, templates)?;
        } else if !config.templates.is_empty() {
            let mut templates = config.templates.clone();
            templates.extend(
                self.model
                    .templates
                    .iter()
                    .filter(|t| !config.templates.iter().any(|v| v.id == t.id))
                    .cloned(),
            );
            candidate.load_catalog(candidate.spaces.clone(), templates)?;
        }
        self.model = candidate;
        self.configured_spaces = config.spaces.unwrap_or_default();
        self.configured_templates = config.templates;
        self.configuration_epoch = self.configuration_epoch.wrapping_add(1);
        self.ui
            .set_config_owned(self.model.config_owned.iter().cloned());
        self.ui.cancel_effects();
        self.ui.invalidate();
        Ok(Update {
            model_changed: true,
            layout_changed: true,
            projection_changed: true,
            ..Update::default()
        })
    }
    pub fn hook_token(&self, tab_id: Option<TabId>) -> HookToken {
        HookToken {
            host_revision: self.host_revision.unwrap_or(0),
            model_revision: self.model.revision.wrapping_add(self.geometry_revision),
            configuration_epoch: self.configuration_epoch,
            tab_id,
        }
    }
    pub fn complete_hook(&mut self, token: HookToken, result: HookResult) -> Result<bool, Error> {
        self.complete_hook_batch(vec![(token, result)])
    }
    pub fn complete_hooks(
        &mut self,
        token: HookToken,
        results: Vec<HookResult>,
    ) -> Result<bool, Error> {
        self.complete_hook_batch(results.into_iter().map(|result| (token, result)).collect())
    }
    /// Validate every token against the same pre-transaction revision, then publish all
    /// semantic changes together. A bad item or stale token cannot publish a partial batch.
    pub fn complete_hook_batch(
        &mut self,
        batch: Vec<(HookToken, HookResult)>,
    ) -> Result<bool, Error> {
        if batch.len() > 4096 {
            return Err(Error("Hook batch exceeds supported limit".into()));
        }
        if self.teardown || batch.is_empty() {
            return Ok(false);
        }
        if batch.iter().any(|(token, _)| {
            token.host_revision != self.host_revision.unwrap_or(0)
                || token.model_revision != self.model.revision.wrapping_add(self.geometry_revision)
                || token.configuration_epoch != self.configuration_epoch
        }) {
            return Ok(false);
        }
        let mut candidate = self.model.clone();
        for (token, result) in batch {
            match result {
                HookResult::Route(space) => candidate.apply_route_hook(
                    token
                        .tab_id
                        .ok_or_else(|| Error("Route hook needs tab ID".into()))?,
                    space,
                )?,
                HookResult::Title(title) => candidate.apply_title_hook(
                    token
                        .tab_id
                        .ok_or_else(|| Error("Title hook needs tab ID".into()))?,
                    title,
                )?,
                HookResult::Filter(visible) => candidate.apply_filter_hook(
                    token
                        .tab_id
                        .ok_or_else(|| Error("Filter hook needs tab ID".into()))?,
                    visible,
                )?,
                HookResult::Footer(footer) => candidate.apply_footer_hook(footer)?,
                HookResult::Theme(values) => {
                    for (key, value) in &values {
                        if !matches!(
                            key.as_str(),
                            "accent"
                                | "background"
                                | "foreground"
                                | "muted"
                                | "selected_background"
                                | "private_accent"
                        ) {
                            return Err(Error("Theme hooks may only set theme colors".into()));
                        }
                        core::settings::validate_value(key, value).map_err(Error)?;
                    }
                    for (key, value) in values {
                        candidate.settings.set(&key, value).map_err(Error)?;
                    }
                }
            }
        }
        candidate.revision = self.model.revision.wrapping_add(1);
        self.model = candidate;
        self.ui.invalidate();
        Ok(true)
    }
    /// An incarnation is accepted only when the adapter can prove it identifies this mux
    /// session across attachments. Passing None intentionally disables live-state restore.
    pub fn set_verified_session(&mut self, incarnation: Option<String>) {
        self.storage.set_session(&self.model, incarnation);
    }
    pub fn set_window_identity(&mut self, window_id: u64) {
        self.storage.set_window_identity(window_id);
    }
    pub fn refresh_storage(&mut self) {
        self.storage.refresh(self.now);
    }
    pub fn take_storage_request(&mut self, now: Duration) -> Option<store::Request> {
        self.now = now;
        self.storage.take_request(&self.model, now)
    }
    pub fn complete_storage(&mut self, response: store::Response) -> Result<Update, Error> {
        let changed = self.storage.complete(&mut self.model, response, self.now)?;
        if changed {
            self.merge_configured_catalog()?;
            self.ui.invalidate();
        }
        Ok(Update {
            model_changed: changed,
            layout_changed: changed,
            projection_changed: changed,
            commands: Vec::new(),
        })
    }
    pub fn storage_failed(&mut self, request_id: u64, message: impl Into<String>) {
        let message = message.into();
        self.storage.failed(request_id, self.now);
        if !self.errors.contains(&message) {
            self.errors.push(message);
        }
    }
    pub fn retry_storage(&mut self) {
        self.storage.retry(self.now);
    }
    pub fn restart_storage_after_cancel(&mut self, request_id: u64) {
        self.storage.failed(request_id, self.now);
        self.storage.retry(self.now);
    }
    /// Prevent new interactions and effects; adapter may drain pending writes until its own
    /// bounded shutdown deadline using take_storage_request/complete_storage.
    pub fn teardown(&mut self) {
        self.teardown = true;
        self.ui.dismiss();
        self.ui.cancel_effects();
        self.storage.retry(self.now);
    }
    pub fn storage_pending(&self) -> bool {
        self.storage.pending()
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
        Err(Error("Invalid native font metrics".into()))
    } else {
        Ok(())
    }
}

use crate::{
    WindowApp,
    core::{self, Error, TabId},
    persistence,
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

/// Serialized only for an explicit in-GUI move. No content or pane layout is represented.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowTransfer {
    pub version: u32,
    pub profile: String,
    pub spaces: Vec<core::Space>,
    #[serde(default)]
    pub folders: Vec<core::Folder>,
    pub templates: Vec<core::SpaceTemplate>,
    pub preferences: BTreeMap<String, Value>,
    pub configuration: BTreeMap<String, Value>,
    pub configured_spaces: Vec<core::Space>,
    pub configured_templates: Vec<core::SpaceTemplate>,
    pub managed: Option<core::Managed>,
    pub tab: core::Tab,
}

impl WindowApp {
    pub fn export_transfer(&self, id: TabId) -> Result<WindowTransfer, Error> {
        let tab = self
            .model
            .tabs
            .get(&id)
            .ok_or_else(|| Error("Moved tab no longer exists".into()))?;
        Ok(WindowTransfer {
            version: 1,
            profile: self.model.profile.clone(),
            spaces: self.model.spaces.clone(),
            folders: self.model.folders.clone(),
            templates: self.model.templates.clone(),
            preferences: self.model.managed_settings().clone(),
            configuration: self.model.configured_settings().clone(),
            configured_spaces: self.configured_spaces.clone(),
            configured_templates: self.configured_templates.clone(),
            managed: self.managed.baseline().cloned(),
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
                "Window transfer must initialize an empty window".into(),
            ));
        }
        if !core::valid_id(&transfer.profile) {
            return Err(Error("Invalid transferred profile".into()));
        }
        let mut candidate = WindowApp::new(transfer.profile);
        candidate
            .model
            .load_catalog(transfer.spaces, transfer.templates)?;
        candidate.model.load_folders(transfer.folders)?;
        candidate
            .model
            .load_managed_settings(transfer.preferences)?;
        candidate.model.apply_config(transfer.configuration)?;
        candidate.configured_spaces = transfer.configured_spaces;
        candidate.configured_templates = transfer.configured_templates;
        if let Some(managed) = transfer.managed {
            candidate.managed.loaded(managed);
            candidate.managed.touch();
        }
        candidate.model.import_transferred_tab(transfer.tab)?;
        candidate.storage = persistence::Persistence::new(&candidate.model);
        // Preserve current public edits while a source-window helper is still draining.
        candidate.storage.observe(&candidate.model, Duration::ZERO);
        candidate.ui.set_chord_label(self.ui.chord_label());
        *self = candidate;
        Ok(())
    }
}

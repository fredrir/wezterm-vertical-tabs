use crate::{
    Update, WindowApp,
    core::{self, Error},
    managed,
};
use serde_json::Value;
use std::collections::BTreeMap;

impl WindowApp {
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
        self.redraw();
        Ok(Update::default())
    }
    fn managed_projection(&self) -> core::Managed {
        managed::ManagedFile::projection(
            &self.model,
            &self.configured_spaces,
            &self.configured_templates,
        )
    }
    /// Lua source for the managed settings file after a UI edit; one write is in flight.
    pub fn take_managed_write(&mut self) -> Option<String> {
        let (model, spaces, templates) = (
            &self.model,
            &self.configured_spaces,
            &self.configured_templates,
        );
        self.managed
            .take(|| managed::ManagedFile::projection(model, spaces, templates))
    }
    pub fn complete_managed_write(&mut self, result: Result<(), String>) {
        self.managed.written();
        if let Err(error) = result {
            self.errors.push(format!("Settings file: {error}"));
        }
    }
    /// Precedence: Rust defaults, then the managed file, then values written in the config.
    pub fn config(&mut self, value: Value) -> Result<Update, Error> {
        #[derive(serde::Deserialize, Default)]
        #[serde(default, deny_unknown_fields)]
        struct Config {
            settings: BTreeMap<String, Value>,
            spaces: Vec<core::Space>,
            templates: Vec<core::SpaceTemplate>,
            profile: Option<String>,
            managed: Option<core::Managed>,
        }

        let config: Config =
            serde_json::from_value(value).map_err(|e| Error(format!("UI configuration: {e}")))?;
        if !core::valid_id(&self.model.profile) {
            return Err(Error(format!(
                "Profile must contain 1-{} printable bytes",
                core::MAX_ID_BYTES
            )));
        }
        if config.profile.is_some_and(|p| p != self.model.profile) {
            return Err(Error(
                "Profile is chosen when creating the window application".into(),
            ));
        }
        let current = self.managed_projection();
        let file = config.managed.filter(|_| !self.managed.pending(&current));
        let loaded = file.is_some();
        let initial = self.model.tabs.is_empty()
            && self.model.spaces.len() == 1
            && self.model.spaces[0].id == core::DEFAULT_SPACE;
        let managed = file.unwrap_or_else(|| core::Managed {
            spaces: if initial { Vec::new() } else { current.spaces },
            ..current
        });

        let mut spaces = config.spaces.clone();
        for space in managed.spaces.iter().chain(
            self.model
                .spaces
                .iter()
                .filter(|space| space.template.is_some()),
        ) {
            if !spaces.iter().any(|s| s.id == space.id) {
                spaces.push(space.clone());
            }
        }
        if spaces.is_empty() {
            spaces.push(core::Space::new(core::DEFAULT_SPACE, "Home"));
        }
        for space in &mut spaces {
            if let Some(existing) = self.model.spaces.iter().find(|s| s.id == space.id) {
                space.collapsed = existing.collapsed;
            }
        }
        let mut templates = config.templates.clone();
        for template in managed.templates {
            if !templates.iter().any(|t| t.id == template.id) {
                templates.push(template);
            }
        }

        // Validate every layer on a detached model before publishing any of them.
        let mut candidate = self.model.clone();
        candidate.load_managed_settings(managed.settings)?;
        candidate.apply_config(config.settings)?;
        if spaces != candidate.spaces || templates != candidate.templates {
            candidate.load_catalog(spaces, templates)?;
        }
        self.model = candidate;
        self.configured_spaces = config.spaces;
        self.configured_templates = config.templates;
        if loaded {
            let projection = self.managed_projection();
            self.managed.loaded(projection);
        }
        self.configuration_epoch = self.configuration_epoch.wrapping_add(1);
        self.redraw();
        Ok(Update::default())
    }
}

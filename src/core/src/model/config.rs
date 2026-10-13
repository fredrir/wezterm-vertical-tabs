use super::{Model, clean_name};
use crate::{
    Error, Folder, MAX_FOLDERS, MAX_SPACES, Space,
    routing::{self, SpaceTemplate},
    settings::{self, Settings},
    valid_id,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl Model {
    pub fn load_catalog(
        &mut self,
        spaces: Vec<Space>,
        templates: Vec<SpaceTemplate>,
    ) -> Result<(), Error> {
        if spaces.is_empty() || spaces.len() > MAX_SPACES {
            return Err(Error(format!("Catalog needs 1-{MAX_SPACES} spaces")));
        }
        let mut seen = BTreeSet::new();
        for s in &spaces {
            if !valid_id(&s.id) || !seen.insert(s.id.clone()) {
                return Err(Error("Invalid or duplicate space ID".into()));
            }
            clean_name(s.name.clone())?;
            if s.accent
                .as_deref()
                .is_some_and(|s| !settings::valid_color(s))
            {
                return Err(Error("Invalid space accent".into()));
            }
            routing::validate_rules(&s.rules).map_err(Error)?;
        }
        for t in &templates {
            routing::validate_rules(&t.rules).map_err(Error)?;
        }
        self.spaces = spaces;
        self.templates = templates;
        self.folders
            .retain(|folder| seen.contains(&folder.space_id));
        if !seen.contains(&self.selected_space) {
            self.selected_space = self.spaces[0].id.clone();
        }
        self.reroute();
        self.touch();
        Ok(())
    }
    pub fn load_folders(&mut self, folders: Vec<Folder>) -> Result<(), Error> {
        if folders.len() > MAX_FOLDERS {
            return Err(Error(format!(
                "At most {MAX_FOLDERS} folders are supported"
            )));
        }
        let mut seen = BTreeSet::new();
        for folder in &folders {
            if !valid_id(&folder.id) || !seen.insert(&folder.id) {
                return Err(Error("Invalid or duplicate folder ID".into()));
            }
            clean_name(folder.name.clone())?;
            self.require_space(&folder.space_id)?;
        }
        if self.folders != folders {
            self.folders = folders;
            self.rebuild_visible();
            self.touch();
        }
        Ok(())
    }
    pub fn load_managed_settings(&mut self, values: BTreeMap<String, Value>) -> Result<(), Error> {
        validate(&values)?;
        self.managed_settings = values;
        self.rebuild_settings()?;
        self.touch();
        Ok(())
    }
    pub fn apply_config(&mut self, values: BTreeMap<String, Value>) -> Result<(), Error> {
        validate(&values)?;
        self.config_owned = values.keys().cloned().collect();
        self.lua_settings = values;
        self.rebuild_settings()?;
        self.hook_routes.clear();
        self.hidden.clear();
        self.footer.clear();
        for tab in self.tabs.values_mut() {
            tab.title_hook = None;
        }
        self.reroute();
        self.touch();
        Ok(())
    }
    pub(super) fn rebuild_settings(&mut self) -> Result<(), Error> {
        let mut settings = Settings::default();
        settings.merge(self.managed_settings.iter().chain(&self.lua_settings))?;
        if let Some(rail) = self.rail_override {
            settings.rail = rail;
        }
        self.settings = settings;
        Ok(())
    }
}

fn validate(values: &BTreeMap<String, Value>) -> Result<(), Error> {
    for (key, value) in values {
        settings::validate_value(key, value)?;
    }
    Ok(())
}

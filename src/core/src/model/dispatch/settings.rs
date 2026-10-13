use super::Change;
use crate::{Error, Model, RailMode, Settings};
use serde_json::Value;

impl Model {
    pub(super) fn set_setting(&mut self, key: String, value: Value) -> Result<Change, Error> {
        self.require_unowned(&key)?;
        self.settings.set(&key, value.clone())?;
        if key == "rail" {
            self.rail_override = None;
        }
        self.managed_settings.insert(key, value);
        Ok(Change::Durable)
    }
    pub(super) fn reset_setting(&mut self, key: String) -> Result<Change, Error> {
        self.require_unowned(&key)?;
        let value = Settings::default()
            .get(&key)
            .ok_or_else(|| Error(format!("Unknown setting: {key}")))?;
        self.managed_settings.remove(&key);
        if key == "rail" {
            self.rail_override = None;
        }
        self.settings.set(&key, value)?;
        Ok(Change::Durable)
    }
    pub(super) fn reset_settings(&mut self) -> Result<Change, Error> {
        self.managed_settings.clear();
        self.rail_override = None;
        self.rebuild_settings()?;
        Ok(Change::Durable)
    }
    // A session toggle: Lua keeps the startup value and nothing is written.
    pub(super) fn set_rail(&mut self, rail: RailMode) -> Change {
        self.rail_override = Some(rail);
        self.settings.rail = rail;
        Change::Memory
    }
    fn require_unowned(&self, key: &str) -> Result<(), Error> {
        if self.config_owned.contains(key) {
            Err(Error(format!("{key} is configured in Lua")))
        } else {
            Ok(())
        }
    }
}

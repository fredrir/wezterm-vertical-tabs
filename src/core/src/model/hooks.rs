use super::Model;
use crate::{Error, TabId, settings, valid_id};
use serde_json::Value;
use std::collections::BTreeMap;

impl Model {
    pub fn apply_route_hook(&mut self, id: TabId, space: Option<String>) -> Result<(), Error> {
        self.require_tab(id)?;
        if space.as_deref().is_some_and(|space| !valid_id(space)) {
            return Err(Error("Invalid hook space ID".into()));
        }
        self.hook_routes.insert(id, space);
        self.route_tab(id);
        self.rebuild_visible();
        self.touch();
        Ok(())
    }
    pub fn apply_title_hook(&mut self, id: TabId, title: String) -> Result<(), Error> {
        self.require_tab(id)?;
        if title.len() > 4096 || title.chars().any(char::is_control) {
            return Err(Error("Invalid hook title".into()));
        }
        self.tabs.get_mut(&id).unwrap().title_hook = Some(title);
        self.touch();
        Ok(())
    }
    pub fn apply_filter_hook(&mut self, id: TabId, visible: bool) -> Result<(), Error> {
        self.require_tab(id)?;
        if visible {
            self.hidden.remove(&id);
        } else {
            self.hidden.insert(id);
        }
        self.rebuild_visible();
        self.touch();
        Ok(())
    }
    pub fn apply_footer_hook(&mut self, footer: String) -> Result<(), Error> {
        if footer.len() > 4096
            || footer.lines().count() > 16
            || footer
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(Error(
                "Footer must contain at most 16 printable rows and 4096 bytes".into(),
            ));
        }
        self.footer = footer;
        self.touch();
        Ok(())
    }
    pub fn apply_theme_hook(&mut self, values: &BTreeMap<String, Value>) -> Result<(), Error> {
        for (key, value) in values {
            if !settings::is_theme(key) {
                return Err(Error("Theme hooks may only set theme colors".into()));
            }
            settings::validate_value(key, value)?;
        }
        self.settings.merge(values)?;
        self.touch();
        Ok(())
    }
}

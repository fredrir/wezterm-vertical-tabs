use crate::{Space, SpaceTemplate, lua};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The plugin-owned Lua file the settings UI rewrites; hand edits are reloaded by WezTerm.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Managed {
    #[schemars(with = "crate::settings::Overrides")]
    pub settings: BTreeMap<String, Value>,
    pub spaces: Vec<Space>,
    pub templates: Vec<SpaceTemplate>,
}

impl Managed {
    pub fn to_lua(&self) -> String {
        let spaces = self
            .spaces
            .iter()
            .map(|space| {
                let mut value = json!(space);
                let fields = value.as_object_mut().unwrap();
                fields.remove("collapsed");
                fields.retain(|_, v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()));
                value
            })
            .collect::<Vec<_>>();
        let value =
            json!({"settings": self.settings, "spaces": spaces, "templates": self.templates});
        format!(
            "-- Managed by vertical-tabs: the settings UI rewrites this file.\n\
             -- Edit freely; values set in apply_to_config take precedence.\n\
             ---@type TabsManaged\nreturn {}\n",
            lua::source(&value, 7)
        )
    }
}

//! Values exchanged with Lua; `gen-schema types` derives the plugin's annotations from them.
use crate::{Intent, Model, Settings, Space, SpaceId, TabId, settings};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

/// Sidebar surfaces opened by `vtabs.action`, besides domain intents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiAction {
    Settings,
    CreateSpace,
    Navigator,
    QuickTerminal,
    RetryStorage,
    #[serde(rename = "QuickTerminal")]
    QuickProgram(QuickProgram),
}

/// A fresh program in the quick terminal surface, in the active pane's domain; ends when hidden.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuickProgram {
    /// The domain's default program when empty.
    #[serde(default)]
    pub args: Vec<String>,
    /// The active pane's directory when omitted.
    pub cwd: Option<String>,
    #[serde(default)]
    pub set_environment_variables: BTreeMap<String, String>,
    /// Fraction of the window width, above 0 and at most 1; 0.75 when omitted.
    #[serde(default, deserialize_with = "fraction")]
    pub width: Option<f32>,
    /// Fraction of the window height, above 0 and at most 1; 0.75 when omitted.
    #[serde(default, deserialize_with = "fraction")]
    pub height: Option<f32>,
}

fn fraction<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f32>, D::Error> {
    let value = Option::<f32>::deserialize(deserializer)?;
    if value.is_some_and(|value| !(value > 0. && value <= 1.)) {
        return Err(serde::de::Error::custom(
            "size must be above 0 and at most 1",
        ));
    }
    Ok(value)
}

/// Argument to `vtabs.action` and `wezterm.vtabs.dispatch`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Action {
    Ui(UiAction),
    Intent(Intent),
}

/// Input to the window-level `theme` and `footer` hooks.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WindowContext {
    pub profile: String,
    pub private: bool,
    pub selected_space: SpaceId,
    pub space: Space,
    pub active_tab: Option<TabId>,
    #[schemars(with = "settings::Overrides")]
    pub settings: Settings,
}

impl WindowContext {
    pub fn of(model: &Model) -> Self {
        Self {
            profile: model.profile.clone(),
            private: model.private,
            selected_space: model.selected_space.clone(),
            space: model.selected_space().clone(),
            active_tab: model.selected_tab,
            settings: model.settings.clone(),
        }
    }
}

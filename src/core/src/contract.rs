//! Values exchanged with Lua; `gen-schema types` derives the plugin's annotations from them.
use crate::{Intent, Model, Settings, Space, SpaceId, TabId, settings};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Sidebar surfaces opened by `vtabs.action`, besides domain intents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiAction {
    Settings,
    CreateSpace,
    Navigator,
    RetryStorage,
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

//! Values exchanged with Lua; `gen-schema types` derives the plugin's annotations from them.
use crate::{Intent, Model, Settings, Space, SpaceId, TabId, WindowPadding, settings};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Sidebar surfaces opened by `vtabs.action`, besides domain intents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiAction {
    Settings,
    CreateSpace,
    Navigator,
    TabActions,
    QuickTerminal,
    RetryStorage,
    #[serde(rename = "QuickTerminal")]
    QuickProgram(Box<QuickProgram>),
}

/// A fresh program in the quick terminal surface; ends when hidden unless `preserve` is set.
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
    /// The active pane's domain when omitted.
    pub domain: Option<SpawnDomain>,
    /// Fraction of the window width, above 0 and at most 1; `quick_terminal_width` when omitted.
    #[serde(default, deserialize_with = "fraction")]
    pub width: Option<f32>,
    /// Fraction of the window height, above 0 and at most 1; `quick_terminal_height` when omitted.
    #[serde(default, deserialize_with = "fraction")]
    pub height: Option<f32>,
    /// Sides left out follow `quick_terminal_window_padding`.
    #[serde(default)]
    pub window_padding: WindowPadding,
    /// From 0 through 1; `quick_terminal_opacity` when omitted.
    #[serde(default, deserialize_with = "opacity")]
    pub window_background_opacity: Option<f32>,
    /// Backdrop blur radius in logical pixels, at most 64; `quick_terminal_blur` when omitted.
    #[serde(default, deserialize_with = "blur")]
    pub blur: Option<u16>,
    /// WezTerm `colors` over the terminal palette; `background` also fills the frame.
    pub colors: Option<serde_json::Map<String, Value>>,
    /// A WezTerm color, or `none`; `quick_terminal_border_color` when omitted.
    pub border_color: Option<String>,
    /// `quick_terminal_auto_hide` when omitted.
    pub auto_hide: Option<bool>,
    /// `quick_terminal_hide_on_escape` when omitted.
    pub hide_on_escape: Option<settings::HideOnEscape>,
    /// Keep the program running while hidden; it ends with the window.
    #[serde(default)]
    pub preserve: bool,
}

/// WezTerm's `SpawnTabDomain`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SpawnDomain {
    DefaultDomain,
    CurrentPaneDomain,
    DomainName(String),
    DomainId(usize),
}

fn bounded<'de, D: Deserializer<'de>, T: Deserialize<'de> + Copy>(
    deserializer: D,
    valid: impl Fn(T) -> bool,
    error: &str,
) -> Result<Option<T>, D::Error> {
    let value = Option::<T>::deserialize(deserializer)?;
    if value.is_some_and(|value| !valid(value)) {
        return Err(serde::de::Error::custom(error));
    }
    Ok(value)
}

fn fraction<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f32>, D::Error> {
    bounded(
        deserializer,
        |value: f32| settings::valid_fraction(value.into()),
        "size must be above 0 and at most 1",
    )
}

fn opacity<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f32>, D::Error> {
    bounded(
        deserializer,
        |value: f32| (0. ..=1.).contains(&value),
        "window_background_opacity must be from 0 through 1",
    )
}

fn blur<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u16>, D::Error> {
    bounded(
        deserializer,
        |value: u16| value <= 64,
        "blur must be at most 64",
    )
}

/// Argument to `vtabs.action` and `wezterm.vtabs.dispatch`.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Action {
    Ui(UiAction),
    Intent(Intent),
}

// Untagged derive hides why the matching variant failed, e.g. an out of range size.
impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let ui = match UiAction::deserialize(&value) {
            Ok(action) => return Ok(Self::Ui(action)),
            Err(error) => error,
        };
        let intent = match Intent::deserialize(&value) {
            Ok(intent) => return Ok(Self::Intent(intent)),
            Err(error) => error,
        };
        let name = match &value {
            Value::String(name) => Some(name),
            Value::Object(fields) => fields.keys().next(),
            _ => None,
        };
        // A nested enum, such as a program's `domain`, also reports an unknown variant.
        let unknown = |error: &serde_json::Error| {
            name.is_some_and(|name| {
                error
                    .to_string()
                    .starts_with(&format!("unknown variant `{name}`"))
            })
        };
        Err(serde::de::Error::custom(
            match (unknown(&ui), unknown(&intent)) {
                (true, true) => format!("unknown action {value}"),
                (false, _) => ui.to_string(),
                (true, false) => intent.to_string(),
            },
        ))
    }
}

/// Input to the window-level `theme` and `footer` hooks.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WindowContext {
    pub profile: String,
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
            selected_space: model.selected_space.clone(),
            space: model.selected_space().clone(),
            active_tab: model.selected_tab,
            settings: model.settings.clone(),
        }
    }
}

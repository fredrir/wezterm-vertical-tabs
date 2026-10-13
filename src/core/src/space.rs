use crate::routing::RoutingRule;
use serde::{Deserialize, Serialize};

pub type SpaceId = String;
pub const DEFAULT_SPACE: &str = "home";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub space_id: SpaceId,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Space {
    pub id: SpaceId,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
    #[serde(default)]
    pub template: Option<String>,
    /// Hides this space's pinned tabs and folders; indexes stay stable.
    #[serde(default)]
    pub collapsed: bool,
}

impl Space {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            icon: "◉".into(),
            accent: None,
            rules: Vec::new(),
            template: None,
            collapsed: false,
        }
    }
}

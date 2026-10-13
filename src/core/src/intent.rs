use crate::{LaunchSpec, PaneId, SpaceId, TabId, routing::RoutingRule, settings::RailMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Intent {
    SelectSpace(SpaceId),
    CreateSpace {
        name: String,
    },
    RenameSpace {
        id: SpaceId,
        name: String,
    },
    EditSpace {
        id: SpaceId,
        icon: String,
        accent: Option<String>,
        rules: Vec<RoutingRule>,
    },
    DeleteSpace {
        id: SpaceId,
        destination: Option<SpaceId>,
    },
    MoveSpace {
        id: SpaceId,
        index: usize,
    },
    ToggleSpace(SpaceId),
    CreateFolder {
        name: String,
    },
    RenameFolder {
        id: String,
        name: String,
    },
    ToggleFolder(String),
    DeleteFolder(String),
    AssignFolder {
        tab_id: TabId,
        folder_id: Option<String>,
    },
    MoveFolder {
        id: String,
        index: usize,
    },
    ActivateTab(TabId),
    ActivateIndex(isize),
    ActivateRelative {
        delta: isize,
        wrap: bool,
    },
    ActivateLast,
    NewTab,
    NewTabInFolder(String),
    CloseTab(TabId),
    CloseOthers(TabId),
    CloseBelow(TabId),
    RenameTab {
        id: TabId,
        title: String,
    },
    PinTab {
        id: TabId,
        pinned: bool,
    },
    MoveTab {
        id: TabId,
        index: usize,
    },
    AssignTab {
        id: TabId,
        space_id: SpaceId,
    },
    ReturnToAuto(TabId),
    SetSetting {
        #[cfg_attr(feature = "schema", schemars(with = "crate::settings::Key"))]
        key: String,
        value: Value,
    },
    ResetSetting(#[cfg_attr(feature = "schema", schemars(with = "crate::settings::Key"))] String),
    ResetSettings,
    SetRail(RailMode),
    MoveTabToNewWindow(TabId),
    CustomAction(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCommand {
    Activate(TabId),
    Spawn {
        space_id: SpaceId,
        launch: LaunchSpec,
        folder_id: Option<String>,
    },
    Close(TabId),
    Rename {
        id: TabId,
        title: String,
    },
    Reorder {
        visible_order: Vec<TabId>,
    },
    MoveTabToNewWindow(TabId),
    FocusPane {
        tab: TabId,
        pane: PaneId,
    },
    KillPane(PaneId),
    /// The pane leaves its tab for one of its own, optionally at a visible index.
    DetachPane {
        pane: PaneId,
        index: Option<usize>,
    },
    JoinPane {
        pane: PaneId,
        tab: TabId,
    },
    /// Every pane of `source` becomes a split of `tab`.
    JoinTab {
        source: TabId,
        tab: TabId,
    },
    CustomAction(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transition {
    pub commands: Vec<HostCommand>,
    pub durable_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}

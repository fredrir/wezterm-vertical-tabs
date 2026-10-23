//! Deterministic vertical-tab policies. No host, rendering, storage, or I/O dependencies.
mod contract;
pub mod distros;
mod intent;
pub mod keybinds;
mod limits;
mod location;
pub mod lua;
mod managed;
mod model;
pub mod padding;
mod routing;
pub mod settings;
mod space;
mod tab;

pub use contract::{Action, QuickProgram, SpawnDomain, UiAction, WindowContext};
pub use intent::{Error, HostCommand, Intent, Transition};
pub use limits::{MAX_FOLDERS, MAX_ID_BYTES, MAX_NAME_CHARS, MAX_SPACES, valid_id};
pub use managed::Managed;
pub use model::Model;
pub use padding::{Dimension, WindowPadding};
pub use routing::{MatchField, RoutingRule, SpaceTemplate};
pub use settings::{
    HideOnEscape, RailMode, SettingDescriptor, SettingKind, Settings, Side, TabTitle,
};
pub use space::{DEFAULT_SPACE, Folder, Space, SpaceId};
pub use tab::{HostTab, LaunchSpec, PaneId, Tab, TabId, TabIdentity, TabPane};

#[cfg(test)]
#[path = "../tests/folders.rs"]
mod folders_tests;

#[cfg(test)]
#[path = "../tests/invariants.rs"]
mod invariants_tests;

#[cfg(test)]
#[path = "../tests/lua.rs"]
mod lua_tests;

#[cfg(test)]
#[path = "../tests/location.rs"]
mod location_tests;

#[cfg(test)]
#[path = "../tests/projection.rs"]
mod projection_tests;

#[cfg(test)]
#[path = "../tests/reconcile.rs"]
mod reconcile_tests;

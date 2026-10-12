//! Deterministic vertical-tab policies. No host, rendering, storage, or I/O dependencies.
mod contract;
pub mod distros;
pub mod keybinds;
pub mod lua;
mod managed;
mod model;
pub mod padding;
mod routing;
pub mod settings;

pub use contract::{Action, QuickProgram, SpawnDomain, UiAction, WindowContext};
pub use managed::Managed;
pub use model::*;
pub use padding::{Dimension, WindowPadding};
pub use routing::{MatchField, RoutingRule, SpaceTemplate};
pub use settings::{
    HideOnEscape, RailMode, SettingDescriptor, SettingKind, Settings, Side, TabTitle,
};

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

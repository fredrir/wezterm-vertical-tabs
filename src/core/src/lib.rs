//! Deterministic vertical-tab policies. No host, rendering, storage, or I/O dependencies.
mod model;
mod routing;
pub mod settings;

pub use model::*;
pub use routing::{MatchField, RoutingRule, SpaceTemplate};
pub use settings::{RailMode, SettingDescriptor, SettingKind, Settings, Side};

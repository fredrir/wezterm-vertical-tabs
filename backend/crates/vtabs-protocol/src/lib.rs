pub mod command;
pub mod event;
pub mod limits;
pub mod payload;
pub mod types;

pub use command::Command;
pub use event::{
    CardPart, Event, Intent, Modifier, SettingsApplyMode, SettingsChange, SettingsPatch, Transport,
};
pub use types::{Button, Color, Mods, Mouse, MouseKind};

use base64::Engine as _;

/// Wire encoding for binary payloads (`raw`, `paste`, user-var values).
pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

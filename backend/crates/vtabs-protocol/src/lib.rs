pub mod command;
pub mod event;
pub mod limits;
pub mod types;
pub mod v2;

pub use command::Command;
pub use event::{
    CardPart, DoArgs, DoId, Event, Intent, Modifier, SettingsApplyMode, SettingsChange,
    SettingsPatch,
};
pub use types::{Button, Color, Mods, Mouse, MouseKind};

use base64::Engine as _;

/// Wire encoding for binary payloads (`raw`, `paste`, user-var values).
pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

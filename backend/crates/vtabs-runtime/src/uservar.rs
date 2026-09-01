use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

pub use vtabs_protocol::b64;

pub const DEFAULT_VAR: &str = "vtabs";
pub const ROLE_VAR: &str = "vtabs_role";
pub const TOKEN_VAR: &str = "vtabs_token";

/// The only thing a role changes: which title marker and `vtabs_role` value this pane advertises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    Sidebar,
    Settings,
}

impl Role {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "sidebar" => Some(Role::Sidebar),
            "settings" => Some(Role::Settings),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Sidebar => "sidebar",
            Role::Settings => "settings",
        }
    }

    pub fn title_prefix(self) -> &'static str {
        match self {
            Role::Sidebar => "wez-vtabs:",
            Role::Settings => "wez-vtabs-settings:",
        }
    }
}

pub fn set_user_var(name: &str, value: &str) -> String {
    format!("\x1b]1337;SetUserVar={name}={}\x07", b64(value.as_bytes()))
}

/// Per-process id for the title; never the auth token, which window titles would leak to the desktop.
pub fn nonce() -> String {
    let mut bytes = [0u8; 4];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut bytes);
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "{:08x}",
        u32::from_le_bytes(bytes) ^ std::process::id() ^ nanos
    )
}

/// Pane title the plugin looks for when a mux outlives the GUI; hex only, so it cannot break the OSC.
pub fn title_marker(role: Role, nonce: &str) -> String {
    let hex: String = nonce.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let nonce = if hex.is_empty() { "0" } else { &hex };
    let prefix = role.title_prefix();
    format!("\x1b]0;{prefix}{nonce}\x07\x1b]2;{prefix}{nonce}\x07")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_marker_keeps_hex_only() {
        assert_eq!(
            title_marker(Role::Sidebar, "a1b2"),
            "\x1b]0;wez-vtabs:a1b2\x07\x1b]2;wez-vtabs:a1b2\x07"
        );
        assert_eq!(
            title_marker(Role::Sidebar, "\x07;evil ~"),
            "\x1b]0;wez-vtabs:e\x07\x1b]2;wez-vtabs:e\x07"
        );
        assert!(title_marker(Role::Sidebar, "").starts_with("\x1b]0;wez-vtabs:0\x07"));
    }

    #[test]
    fn nonce_is_eight_hex_digits() {
        let n = nonce();
        assert_eq!(n.len(), 8);
        assert!(n.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}

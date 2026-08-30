use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

pub const DEFAULT_VAR: &str = "vtabs";
pub const ROLE_VAR: &str = "vtabs_role";
pub const ROLE: &str = "sidebar";
pub const TOKEN_VAR: &str = "vtabs_token";
pub const TITLE_PREFIX: &str = "wez-vtabs:";

pub fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
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
pub fn title_marker(nonce: &str) -> String {
    let hex: String = nonce.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let nonce = if hex.is_empty() { "0" } else { &hex };
    format!("\x1b]0;{TITLE_PREFIX}{nonce}\x07\x1b]2;{TITLE_PREFIX}{nonce}\x07")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_osc_1337_byte_exact() {
        assert_eq!(
            set_user_var("vtabs", r#"{"t":"ping"}"#),
            "\x1b]1337;SetUserVar=vtabs=eyJ0IjoicGluZyJ9\x07"
        );
    }

    #[test]
    fn role_var() {
        assert_eq!(
            set_user_var(ROLE_VAR, ROLE),
            "\x1b]1337;SetUserVar=vtabs_role=c2lkZWJhcg==\x07"
        );
    }

    #[test]
    fn title_marker_keeps_hex_only() {
        assert_eq!(
            title_marker("a1b2"),
            "\x1b]0;wez-vtabs:a1b2\x07\x1b]2;wez-vtabs:a1b2\x07"
        );
        assert_eq!(
            title_marker("\x07;evil ~"),
            "\x1b]0;wez-vtabs:e\x07\x1b]2;wez-vtabs:e\x07"
        );
        assert!(title_marker("").starts_with("\x1b]0;wez-vtabs:0\x07"));
    }

    #[test]
    fn nonce_is_eight_hex_digits() {
        let n = nonce();
        assert_eq!(n.len(), 8);
        assert!(n.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn pads_short_values() {
        assert_eq!(set_user_var("x", "a"), "\x1b]1337;SetUserVar=x=YQ==\x07");
    }
}

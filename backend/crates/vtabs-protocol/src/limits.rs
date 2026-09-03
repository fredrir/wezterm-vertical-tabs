pub const VERSION: u8 = 3;

/// Record-separator framing keeps control input disjoint from literal JSON-shaped keyboard input.
pub const CONTROL_PREFIX: &[u8] = b"\x1eVTABS ";
pub const CONTROL_TOKEN_MAX_BYTES: usize = 64;

pub fn valid_control_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= CONTROL_TOKEN_MAX_BYTES
        && token.bytes().all(|byte| byte.is_ascii_graphic())
}

pub const FRAME_MAX_SIDE: u32 = 16384;
pub const FRAME_MAX_AREA: u64 = 8192 * 8192;

/// Largest complete JSON command line accepted by the protocol.
pub const LINE_MAX: usize = 1024 * 1024;
/// Runaway input is abandoned at this bound even before a line terminator arrives.
pub const PARSER_BUFFER_MAX: usize = LINE_MAX * 2;

/// Inbox session ids are directory basenames: `inbox-<pid>-<nonce>`, word characters and dashes.
pub const INBOX_SESSION_MAX_BYTES: usize = 64;
/// Inbox message names are the zero-padded sequence number plus `.msg`.
pub const INBOX_SEQ_DIGITS: usize = 8;
/// One inbox file carries one `send_raw` batch: several records, each bounded by `LINE_MAX`.
pub const INBOX_FILE_MAX_BYTES: usize = LINE_MAX * 6;

/// Exact terminal bytes retained on a key event for safe forwarding to the content pane.
pub const FORWARDED_KEY_MAX_BYTES: usize = 16;
/// Padded base64 bytes needed to carry the largest forwarded key.
pub const FORWARDED_KEY_MAX_ENCODED_BYTES: usize = FORWARDED_KEY_MAX_BYTES.div_ceil(3) * 4;

/// Decoded bracketed-paste payload accepted and forwarded as one paste.
pub const PASTE_MAX_BYTES: usize = 64 * 1024;
/// Padded base64 bytes needed to carry the largest accepted paste.
pub const PASTE_MAX_ENCODED_BYTES: usize = PASTE_MAX_BYTES.div_ceil(3) * 4;

pub const MODEL_MAX_TABS: usize = 200;
pub const MENU_MAX_ITEMS: usize = 64;
pub const MODEL_MAX_FIELDS: usize = 512;

pub const MODEL_MAX_SPACES: usize = 32;

/// The persisted settings body must remain readable on the next boot. This is generated into Lua
/// and enforced before Rust publishes a settings commit and before Lua writes it.
pub const SETTINGS_BODY_MAX_BYTES: usize = 512 * 1024;

/// macOS integrated-titlebar height in points. Rust owns the geometry constant; Lua consumes its
/// generated mirror for host padding so the two sides cannot drift.
pub const TITLEBAR_PT: u32 = 28;

pub const FX_MAX_MS: u64 = 2000;
pub const FX_MIN_FPS: u32 = 15;
pub const FX_MAX_FPS: u32 = 60;

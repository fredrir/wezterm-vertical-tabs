//! One home for every bound both sides of the wire must agree on; `gen-lua` mirrors it to Lua.

/// Protocol version carried by `ready`.
pub const VERSION: u8 = 2;

/// `wez-vtabs frame` image bounds; vtabs-zen keeps its own copy, a wez-vtabs test pins equality.
pub const FRAME_MAX_SIDE: u32 = 16384;
pub const FRAME_MAX_AREA: u64 = 8192 * 8192;

/// v2 wire bounds (§2.6); a breach drops the whole command, never half-applies it.
pub const LINE_MAX: usize = 64 * 1024;
pub const MODEL_MAX_TABS: usize = 200;
pub const MENU_MAX_ITEMS: usize = 64;
pub const MODEL_MAX_FIELDS: usize = 512;

/// `fx`: an out-of-range duration or rate is clamped, since a fade is cosmetic.
pub const FX_MAX_MS: u64 = 2000;
pub const FX_MIN_FPS: u32 = 15;
pub const FX_MAX_FPS: u32 = 60;

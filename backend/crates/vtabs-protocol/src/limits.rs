//! One home for every bound both sides of the wire must agree on; `gen-lua` mirrors it to Lua.

/// Protocol version carried by `ready`.
pub const VERSION: u8 = 1;

pub const ANIM_MAX_DATA: usize = 24 * 1024;
pub const ANIM_MAX_ROWS: usize = 128;
pub const ANIM_MAX_MS: u64 = 2000;
pub const ANIM_MIN_FPS: u32 = 15;
pub const ANIM_MAX_FPS: u32 = 60;

/// `wez-vtabs frame` image bounds; vtabs-zen keeps its own copy, a wez-vtabs test pins equality.
pub const FRAME_MAX_SIDE: u32 = 16384;
pub const FRAME_MAX_AREA: u64 = 8192 * 8192;

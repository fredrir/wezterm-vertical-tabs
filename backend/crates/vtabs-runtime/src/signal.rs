//! SIGWINCH without a signal crate: the handler does nothing but flip a flag.

use std::sync::atomic::{AtomicBool, Ordering};

static RESIZED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
const SIGWINCH: i32 = 28;

#[cfg(unix)]
unsafe extern "C" {
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

#[cfg(unix)]
extern "C" fn on_resize(_: i32) {
    RESIZED.store(true, Ordering::Relaxed);
}

/// False when the platform has no SIGWINCH, so the caller must keep polling the size.
pub fn watch_resize() -> bool {
    #[cfg(unix)]
    {
        unsafe { signal(SIGWINCH, on_resize) };
        true
    }
    #[cfg(not(unix))]
    false
}

pub fn resized() -> bool {
    RESIZED.swap(false, Ordering::Relaxed)
}

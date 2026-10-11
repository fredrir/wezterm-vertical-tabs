//! Without a screen, a connection tab would open in whichever client window asked to connect.
use super::ConnectionUI;
use std::sync::atomic::{AtomicBool, Ordering};

static HEADLESS: AtomicBool = AtomicBool::new(false);

impl ConnectionUI {
    pub fn set_headless() {
        HEADLESS.store(true, Ordering::Relaxed);
    }

    pub(super) fn is_headless() -> bool {
        HEADLESS.load(Ordering::Relaxed)
    }
}

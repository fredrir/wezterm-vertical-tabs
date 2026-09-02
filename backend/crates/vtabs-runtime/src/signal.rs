//! Portable SIGWINCH registration. `signal-hook` owns the async-signal-safe handler and its
//! non-blocking self-pipe; this module only forwards notifications to the event loop.

/// Installs the handler; `wake` runs on its own thread once per resize until it returns false.
/// False when the platform has no SIGWINCH, so the caller must keep polling the size.
pub fn watch_resize(wake: Box<dyn Fn() -> bool + Send>) -> bool {
    #[cfg(unix)]
    {
        use signal_hook::{consts::SIGWINCH, iterator::Signals};

        let Ok(mut signals) = Signals::new([SIGWINCH]) else {
            return false;
        };
        std::thread::spawn(move || {
            for _ in signals.forever() {
                if !wake() {
                    break;
                }
            }
        });
        true
    }
    #[cfg(not(unix))]
    {
        let _ = wake;
        false
    }
}

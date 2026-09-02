//! SIGWINCH without a signal crate: the handler writes one byte to a pipe, and a thread reading
//! that pipe wakes the event loop, so a resize is seen when it lands rather than at the next tick.

use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(unix)]
const SIGWINCH: i32 = 28;

static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

#[cfg(unix)]
unsafe extern "C" {
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    fn pipe(fds: *mut i32) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    fn write(fd: i32, buf: *const u8, count: usize) -> isize;
}

/// Async-signal-safe: one `write` and nothing else. A full pipe drops the byte, and one pending
/// wake is all the reader needs.
#[cfg(unix)]
extern "C" fn on_resize(_: i32) {
    let fd = WAKE_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte = 1u8;
        unsafe { write(fd, &byte, 1) };
    }
}

/// Installs the handler; `wake` runs on its own thread once per resize until it returns false.
/// False when the platform has no SIGWINCH, so the caller must keep polling the size.
pub fn watch_resize(wake: Box<dyn Fn() -> bool + Send>) -> bool {
    #[cfg(unix)]
    {
        let mut fds = [-1i32; 2];
        if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
            return false;
        }
        WAKE_FD.store(fds[1], Ordering::Relaxed);
        unsafe { signal(SIGWINCH, on_resize) };
        let reader = fds[0];
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            loop {
                let n = unsafe { read(reader, buf.as_mut_ptr(), buf.len()) };
                if n < 0
                    && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                {
                    continue;
                }
                if n <= 0 || !wake() {
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

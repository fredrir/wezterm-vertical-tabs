//! The inbox transport: one directory per backend session under a root Lua chose, where every
//! `send_raw` batch lands as `<seq>.msg` by rename. Frames to a same-machine mux pane then never
//! cross the GUI's mux link; the backend reads them from disk in sequence order.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use vtabs_protocol::limits::{INBOX_FILE_MAX_BYTES, INBOX_SEQ_DIGITS, INBOX_SESSION_MAX_BYTES};

use crate::uservar::nonce;

/// One `<seq>.msg` read whole: its sequence number and the framed control records inside.
pub type Message = (u32, Vec<u8>);

/// What the reader thread hands the loop: everything on disk for one session, in order.
pub struct Batch {
    pub session: String,
    pub messages: Vec<Message>,
}

/// Wakes the loop with a batch; false once the loop is gone and the reader should stop.
pub type Wake = Arc<dyn Fn(Batch) -> bool + Send + Sync>;

/// The root Lua passed at spawn and the way a session's reader reaches the loop.
pub struct Offer {
    pub root: PathBuf,
    pub wake: Wake,
}

/// A watcher makes the scan a fallback; without one the scan is the only wake there is.
pub const SCAN_WITH_WATCHER: Duration = Duration::from_secs(1);
pub const SCAN_WITHOUT_WATCHER: Duration = Duration::from_millis(50);
/// A `.tmp` older than this was never going to be renamed: its writer is gone.
const TMP_STALE: Duration = Duration::from_secs(10 * 60);

/// One session directory; dropping it removes the directory and whatever it still holds.
#[derive(Debug)]
pub struct Inbox {
    dir: PathBuf,
    session: String,
}

impl Inbox {
    /// Validates and prepares `root`, sweeps what dead siblings left there, and creates this
    /// process's own `inbox-<pid>-<nonce>` under it.
    pub fn create(root: &Path) -> Result<Self, String> {
        prepare_root(root)?;
        sweep(root);
        let session = format!("inbox-{}-{}", std::process::id(), nonce());
        debug_assert!(session.len() <= INBOX_SESSION_MAX_BYTES);
        let dir = root.join(&session);
        make_private_dir(&dir).map_err(|e| format!("inbox {}: {e}", dir.display()))?;
        Ok(Self { dir, session })
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every complete message on disk, by sequence; nothing is removed.
    pub fn scan(&self) -> Vec<Message> {
        scan_dir(&self.dir).unwrap_or_default()
    }

    pub fn remove(&self, seq: u32) {
        let _ = fs::remove_file(self.dir.join(name_of(seq)));
    }

    /// Every message still on disk, in order, each removed as it is taken.
    pub fn drain(&self) -> Vec<Message> {
        let messages = self.scan();
        for (seq, _) in &messages {
            self.remove(*seq);
        }
        messages
    }

    /// Starts this session's reader thread; `watcher` false keeps it on the 50 ms scan alone.
    pub fn watch(&self, wake: Wake, watcher: bool) {
        spawn_reader(self.dir.clone(), self.session.clone(), wake, watcher);
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn name_of(seq: u32) -> String {
    format!("{seq:0width$}.msg", width = INBOX_SEQ_DIGITS)
}

fn seq_of(name: &str) -> Option<u32> {
    let digits = name.strip_suffix(".msg")?;
    if digits.len() != INBOX_SEQ_DIGITS || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn is_tmp(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().ends_with(".tmp")
}

fn make_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}

/// The root is trusted only as a directory this user alone can write into, reached directly.
#[cfg(unix)]
fn prepare_root(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let at = || format!("inbox root {}", root.display());
    make_private_dir(root).map_err(|e| format!("{}: {e}", at()))?;
    let meta = fs::symlink_metadata(root).map_err(|e| format!("{}: {e}", at()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a symlink", at()));
    }
    if !meta.is_dir() {
        return Err(format!("{} is not a directory", at()));
    }
    // SAFETY: geteuid takes nothing and cannot fail.
    if meta.uid() != unsafe { libc::geteuid() } {
        return Err(format!("{} is not owned by this user", at()));
    }
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(format!("{} is writable by group or others", at()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn prepare_root(root: &Path) -> Result<(), String> {
    Err(format!(
        "inbox root {}: the inbox transport needs a unix host",
        root.display()
    ))
}

fn sibling_pid(name: &str) -> Option<u32> {
    let (pid, _) = name.strip_prefix("inbox-")?.split_once('-')?;
    pid.parse().ok()
}

#[cfg(unix)]
fn dead(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 delivers nothing; it only asks whether the pid exists.
    let missing = unsafe { libc::kill(pid, 0) } == -1;
    missing && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn dead(_pid: u32) -> bool {
    false
}

fn remove_stale_tmp(path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    let stale = meta.is_file()
        && meta
            .modified()
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > TMP_STALE);
    if stale {
        let _ = fs::remove_file(path);
    }
}

fn sweep_tmp(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if is_tmp(&entry.file_name()) {
            remove_stale_tmp(&entry.path());
        }
    }
}

/// Removes every sibling session whose process is gone and every write abandoned long ago.
fn sweep(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let own = std::process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let path = entry.path();
        match name.to_str().and_then(sibling_pid) {
            Some(pid) if pid != own && dead(pid) => {
                let _ = fs::remove_dir_all(&path);
            }
            Some(_) => sweep_tmp(&path),
            None if is_tmp(&name) => remove_stale_tmp(&path),
            None => {}
        }
    }
}

fn read_message(path: &Path) -> Option<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > INBOX_FILE_MAX_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(INBOX_FILE_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= INBOX_FILE_MAX_BYTES).then_some(bytes)
}

/// Every complete message in `dir` by sequence. Anything that is not `<seq>.msg` within the file
/// cap is removed, except `*.tmp`, which is a write still in progress. Err only when the
/// directory itself is gone.
fn scan_dir(dir: &Path) -> io::Result<Vec<Message>> {
    let mut messages = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        let path = entry.path();
        let Some(seq) = name.to_str().and_then(seq_of) else {
            if !is_tmp(&name) {
                let _ = fs::remove_file(&path);
            }
            continue;
        };
        match read_message(&path) {
            Some(bytes) => messages.push((seq, bytes)),
            None => {
                let _ = fs::remove_file(&path);
            }
        }
    }
    messages.sort_by_key(|(seq, _)| *seq);
    Ok(messages)
}

/// Scans on every wake and after every interval until the directory is gone or the loop is.
fn spawn_reader(dir: PathBuf, session: String, wake: Wake, watcher: bool) {
    thread::spawn(move || {
        let watcher = watcher.then(|| Watcher::open(&dir)).flatten();
        let interval = if watcher.is_some() {
            SCAN_WITH_WATCHER
        } else {
            SCAN_WITHOUT_WATCHER
        };
        while let Ok(messages) = scan_dir(&dir) {
            if !messages.is_empty()
                && !wake(Batch {
                    session: session.clone(),
                    messages,
                })
            {
                break;
            }
            match &watcher {
                Some(watcher) => watcher.wait(interval),
                None => thread::sleep(interval),
            }
        }
    });
}

#[cfg(target_os = "linux")]
struct Watcher {
    fd: libc::c_int,
}

#[cfg(target_os = "linux")]
impl Watcher {
    /// inotify on the directory for renames into it; None leaves the reader on the fast scan.
    fn open(dir: &Path) -> Option<Self> {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: a fresh descriptor and a NUL-terminated path that outlives the call.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        // SAFETY: `fd` is ours and open; `path` is a valid C string.
        if unsafe { libc::inotify_add_watch(fd, path.as_ptr(), libc::IN_MOVED_TO) } < 0 {
            // SAFETY: closing the descriptor this function opened.
            unsafe { libc::close(fd) };
            return None;
        }
        Some(Self { fd })
    }

    /// Returns on a rename into the directory or after `timeout`; queued events are drained.
    fn wait(&self, timeout: Duration) {
        let mut pollfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
        // SAFETY: one pollfd for our own open descriptor.
        if unsafe { libc::poll(&mut pollfd, 1, ms) } <= 0 {
            return;
        }
        let mut buf = [0u8; 4096];
        // SAFETY: the read writes at most `buf.len()` bytes into `buf`, which lives past it.
        while unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    }
}

#[cfg(target_os = "linux")]
impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: closing the descriptor `open` created, exactly once.
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(target_os = "macos")]
struct Watcher {
    kq: libc::c_int,
    dir: libc::c_int,
}

#[cfg(target_os = "macos")]
impl Watcher {
    /// kqueue on the directory's own descriptor: a rename into it is a write to the directory.
    fn open(dir: &Path) -> Option<Self> {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: a NUL-terminated path; O_EVTONLY opens for notification only.
        let dir = unsafe { libc::open(path.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
        if dir < 0 {
            return None;
        }
        // SAFETY: kqueue takes nothing.
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            // SAFETY: closing the descriptor opened above.
            unsafe { libc::close(dir) };
            return None;
        }
        let change = libc::kevent {
            ident: dir as libc::uintptr_t,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: one change record, no event list, no timeout.
        let registered =
            unsafe { libc::kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
        if registered < 0 {
            // SAFETY: closing both descriptors opened above.
            unsafe {
                libc::close(kq);
                libc::close(dir);
            }
            return None;
        }
        Some(Self { kq, dir })
    }

    /// Returns on a change to the directory or after `timeout`.
    fn wait(&self, timeout: Duration) {
        let timeout = libc::timespec {
            tv_sec: libc::time_t::try_from(timeout.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_nsec: libc::c_long::from(timeout.subsec_nanos()),
        };
        // SAFETY: an all-zero kevent is a valid record for the kernel to fill in.
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };
        // SAFETY: no changes, room for one event, a timeout that outlives the call.
        unsafe { libc::kevent(self.kq, std::ptr::null(), 0, &mut event, 1, &timeout) };
    }
}

#[cfg(target_os = "macos")]
impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: closing the two descriptors `open` created, exactly once.
        unsafe {
            libc::close(self.kq);
            libc::close(self.dir);
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
struct Watcher;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Watcher {
    fn open(_dir: &Path) -> Option<Self> {
        None
    }

    fn wait(&self, timeout: Duration) {
        thread::sleep(timeout);
    }
}

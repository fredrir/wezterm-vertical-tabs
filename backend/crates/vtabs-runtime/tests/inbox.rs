#![cfg(unix)]
//! The inbox directory as the reader sees it: session creation and its guards, sequence scans and
//! what they clean up, and the dead-sibling sweep. Everything here uses only the public API.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use vtabs_protocol::limits::INBOX_FILE_MAX_BYTES;
use vtabs_runtime::inbox::{Batch, Inbox};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "vtabs-inbox-{tag}-{}-{n}-{:?}",
            std::process::id(),
            Instant::now()
        ));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
}

fn msg(seq: u32) -> String {
    format!("{seq:08}.msg")
}

#[test]
fn create_prepares_a_private_session_dir_under_the_root() {
    let root = TempRoot::new("create");
    let inbox = Inbox::create(root.path()).unwrap();
    let meta = fs::metadata(inbox.dir()).unwrap();
    assert!(meta.is_dir());
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    assert!(
        inbox
            .session()
            .starts_with(&format!("inbox-{}-", std::process::id()))
    );
    assert!(inbox.dir().starts_with(root.path()));
}

#[test]
fn a_root_that_is_not_a_trustworthy_directory_is_refused() {
    let symlinked = TempRoot::new("symlink");
    let real = TempRoot::new("symlink-target");
    fs::create_dir_all(real.path()).unwrap();
    symlink(real.path(), symlinked.path()).unwrap();
    assert!(
        Inbox::create(symlinked.path())
            .unwrap_err()
            .contains("symlink")
    );

    let shared = TempRoot::new("group-writable");
    fs::create_dir_all(shared.path()).unwrap();
    fs::set_permissions(shared.path(), fs::Permissions::from_mode(0o770)).unwrap();
    assert!(
        Inbox::create(shared.path())
            .unwrap_err()
            .contains("writable by group")
    );

    let file = TempRoot::new("not-a-dir");
    write(file.path(), b"x");
    assert!(
        Inbox::create(file.path())
            .unwrap_err()
            .contains("not a directory")
    );
}

#[test]
fn scan_sorts_by_sequence_regardless_of_creation_order() {
    let root = TempRoot::new("order");
    let inbox = Inbox::create(root.path()).unwrap();
    for seq in [5u32, 1, 3, 2, 4] {
        write(
            &inbox.dir().join(msg(seq)),
            format!("record {seq}").as_bytes(),
        );
    }
    let seqs: Vec<u32> = inbox.scan().into_iter().map(|(seq, _)| seq).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
}

#[test]
fn scan_drops_junk_and_oversized_files_but_never_touches_a_tmp() {
    let root = TempRoot::new("clean");
    let inbox = Inbox::create(root.path()).unwrap();
    write(&inbox.dir().join(msg(3)), b"good");
    write(
        &inbox.dir().join(msg(4)),
        &vec![b'x'; INBOX_FILE_MAX_BYTES + 1],
    );
    write(&inbox.dir().join("garbage"), b"not a message");
    let tmp = inbox.dir().join("00000005.tmp");
    write(&tmp, b"a write still in progress");

    let messages = inbox.scan();
    assert_eq!(messages, vec![(3u32, b"good".to_vec())]);
    assert!(tmp.exists(), "a scan never deletes a partial write");
    assert!(!inbox.dir().join(msg(4)).exists(), "oversized is removed");
    assert!(!inbox.dir().join("garbage").exists(), "junk is removed");
}

#[test]
fn drain_returns_every_message_in_order_and_empties_the_directory() {
    let root = TempRoot::new("drain");
    let inbox = Inbox::create(root.path()).unwrap();
    for seq in [2u32, 1, 3] {
        write(&inbox.dir().join(msg(seq)), format!("r{seq}").as_bytes());
    }
    let seqs: Vec<u32> = inbox.drain().into_iter().map(|(seq, _)| seq).collect();
    assert_eq!(seqs, vec![1, 2, 3]);
    assert!(inbox.scan().is_empty(), "drain removed each file it took");
}

#[test]
fn create_sweeps_dead_siblings_and_keeps_live_ones() {
    let root = TempRoot::new("sweep");
    fs::create_dir_all(root.path()).unwrap();

    let mut child = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = child.id();
    child.wait().unwrap();
    let dead_sibling = root.path().join(format!("inbox-{dead_pid}-dead"));
    fs::create_dir_all(&dead_sibling).unwrap();
    write(&dead_sibling.join(msg(1)), b"orphaned");

    let live_sibling = root
        .path()
        .join(format!("inbox-{}-live", std::process::id()));
    fs::create_dir_all(&live_sibling).unwrap();

    let _inbox = Inbox::create(root.path()).unwrap();
    assert!(!dead_sibling.exists(), "a dead pid's session is swept");
    assert!(
        live_sibling.exists(),
        "a living pid's session is left alone"
    );
}

#[test]
fn the_reader_scans_without_a_watcher_and_wakes_the_loop() {
    let root = TempRoot::new("scan-reader");
    let inbox = Inbox::create(root.path()).unwrap();
    let session = inbox.session().to_owned();
    let dir = inbox.dir().to_owned();
    let (tx, rx) = mpsc::channel::<Batch>();
    inbox.watch(
        std::sync::Arc::new(move |batch: Batch| tx.send(batch).is_ok()),
        false,
    );
    let partial = dir.join("00000001.tmp");
    write(&partial, b"woke on the scan");
    fs::rename(partial, dir.join(msg(1))).unwrap();
    let batch = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("a watcher-free reader still scans");
    assert_eq!(batch.session, session);
    assert_eq!(batch.messages, vec![(1u32, b"woke on the scan".to_vec())]);
    drop(rx);
}

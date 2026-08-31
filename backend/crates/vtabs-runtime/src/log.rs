use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Logger {
    file: Option<File>,
}

/// Refuses symlinks and keeps the log owner-readable only.
fn open_private(path: &std::ffi::OsStr) -> Option<File> {
    if std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return None;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).ok()
}

impl Logger {
    pub fn from_env() -> Self {
        let file = std::env::var_os("VTABS_LOG").and_then(|path| open_private(&path));
        Self { file }
    }

    pub fn log(&mut self, msg: impl AsRef<str>) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let _ = writeln!(
            file,
            "{}.{:03} {}",
            now.as_secs(),
            now.subsec_millis(),
            msg.as_ref()
        );
    }
}

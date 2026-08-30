use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Logger {
    file: Option<File>,
}

impl Logger {
    pub fn from_env() -> Self {
        let file = std::env::var_os("VTABS_LOG")
            .and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok());
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

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::process::Runner;

pub const CAPABILITY: u32 = 1;

#[derive(Clone)]
pub struct Context {
    pub root: PathBuf,
    pub cache: PathBuf,
    pub install: PathBuf,
    pub runner: Runner,
    pub offline: bool,
    pub upstream: Option<String>,
    pub profile: String,
    pub jobs: Option<usize>,
    pub json: bool,
    pub explain: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProjectSource {
    pub remote: String,
    pub branch: String,
    pub revision: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BuildMetadata {
    pub id: String,
    pub capability: u32,
    #[serde(default)]
    pub upstream: String,
    #[serde(default)]
    pub source_digest: String,
    #[serde(default)]
    pub compile_digest: String,
    #[serde(default)]
    pub validation_digest: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub project_source: ProjectSource,
    #[serde(default)]
    pub built_at: u64,
    #[serde(default)]
    pub configuration: serde_json::Value,
    #[serde(default)]
    pub artifacts: BTreeMap<String, PathBuf>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => {
            Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
                format!("invalid JSON: {}", path.display())
            })?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read: {}", path.display())),
    }
}

pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("JSON parent missing")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temporary, value)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .with_context(|| format!("publish: {}", path.display()))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn safe_id(value: &str) -> Result<&str> {
    ensure!(
        !value.is_empty()
            && value.len() <= 160
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
        "invalid bundle ID"
    );
    Ok(value)
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub struct Lock(File);

impl Lock {
    pub fn acquire(path: &Path) -> Result<Self> {
        fs::create_dir_all(path.parent().context("lock parent missing")?)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        fs2::FileExt::lock_exclusive(&file).with_context(|| format!("lock: {}", path.display()))?;
        Ok(Self(file))
    }

    pub fn try_acquire(path: &Path) -> Result<Option<Self>> {
        fs::create_dir_all(path.parent().context("lock parent missing")?)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(Self(file))),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

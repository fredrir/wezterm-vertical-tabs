use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
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
    pub role: Role,
    /// Cross-compilation target; the host when absent.
    pub triple: Option<String>,
}

impl Context {
    pub fn build_record(&self) -> PathBuf {
        self.cache.join("build.json")
    }

    pub fn last_build(&self) -> Result<Option<BuildMetadata>> {
        read_json(&self.build_record())
    }

    pub fn lock_build(&self) -> Result<Lock> {
        Lock::acquire(&self.cache.join("build.lock"))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Desktop,
    /// Headless host reached through SSH domains: CLI and mux server only.
    Mux,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Desktop => "desktop",
            Role::Mux => "mux",
        }
    }

    pub fn binaries(self) -> &'static [&'static str] {
        match self {
            Role::Desktop => &[
                "wezterm-gui",
                "wezterm",
                "wezterm-mux-server",
                "strip-ansi-escapes",
                "wez-vtabs-store",
            ],
            Role::Mux => &["wezterm", "wezterm-mux-server"],
        }
    }
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
    pub mux_digest: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub project_source: ProjectSource,
    #[serde(default)]
    pub built_at: u64,
    #[serde(default)]
    pub configuration: crate::build_identity::BuildConfiguration,
    #[serde(default)]
    pub artifacts: BTreeMap<String, PathBuf>,
}

impl BuildMetadata {
    pub fn read(directory: &Path) -> Result<Option<Self>> {
        read_json(&directory.join("build.json"))
    }

    pub fn write(&self, directory: &Path) -> Result<()> {
        write_json(&directory.join("build.json"), self)
    }
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

pub fn hex(digest: impl AsRef<[u8]>) -> String {
    base16ct::lower::encode_string(digest.as_ref())
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
}

pub fn lease(root: &Path, name: &str) -> PathBuf {
    root.join("leases").join(format!("{name}.lock"))
}

pub struct Lock {
    _file: File,
}

impl Lock {
    fn open(path: &Path) -> Result<File> {
        fs::create_dir_all(path.parent().context("lock parent missing")?)?;
        Ok(OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?)
    }

    pub fn acquire(path: &Path) -> Result<Self> {
        let file = Self::open(path)?;
        file.lock()
            .with_context(|| format!("lock: {}", path.display()))?;
        Ok(Self { _file: file })
    }

    pub fn shared(path: &Path) -> Result<Self> {
        let file = Self::open(path)?;
        file.lock_shared()
            .with_context(|| format!("lock: {}", path.display()))?;
        Ok(Self { _file: file })
    }

    pub fn try_acquire(path: &Path) -> Result<Option<Self>> {
        let file = Self::open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

//! Machines from `~/.config/wez-vtabs/targets.toml`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use anyhow::{Context as _, Result, ensure};
use clap_complete::engine::CompletionCandidate;
use serde::{Deserialize, Serialize};

use crate::state::Role;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Default `--on` for every machine; an entry's own `on` overrides it.
    pub on: Option<String>,
    #[serde(default)]
    pub targets: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub host: String,
    pub platform: String,
    #[serde(default)]
    pub role: Role,
    /// Default `--on` when running on this machine.
    pub on: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub name: String,
    pub host: String,
    pub platform: Platform,
    pub role: Role,
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Macos,
    Linux(Distro),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Distro {
    pub name: String,
    pub version: String,
    pub image: String,
}

pub const LOCAL: &str = "local";

/// `on`: SSH host that runs the command, `None` here. `to`: receiving machine, `None` if unlisted.
#[derive(Debug, Default)]
pub struct Plan {
    pub on: Option<String>,
    pub to: Option<Target>,
}

impl Plan {
    pub fn role(&self) -> Option<Role> {
        self.to.as_ref().map(|target| target.role)
    }

    pub fn to_is_local(&self) -> bool {
        self.to.as_ref().is_none_or(|target| target.local)
    }

    pub fn is_local(&self) -> bool {
        self.on.is_none() && self.to_is_local()
    }
}

impl Platform {
    pub fn parse(value: &str) -> Result<Self> {
        Ok(if value == "macos" {
            Platform::Macos
        } else {
            Platform::Linux(Distro::parse(value)?)
        })
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Macos => f.write_str("macos"),
            Platform::Linux(distro) => write!(f, "{}-{}", distro.name, distro.version),
        }
    }
}

impl Distro {
    /// `ubuntu-26.04`, `arch`, `debian-13`; the version defaults per distribution.
    pub fn parse(value: &str) -> Result<Self> {
        let (name, version) = match value.split_once('-') {
            Some((name, version)) => (name, Some(version)),
            None => (value, None),
        };
        let name = if name == "archlinux" { "arch" } else { name };
        let version = version.unwrap_or(match name {
            "arch" => "base",
            "ubuntu" => "26.04",
            _ => "latest",
        });
        for part in [name, version] {
            ensure!(
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.'),
                "invalid platform: {value}"
            );
        }
        let image = match name {
            "arch" => format!("docker.io/library/archlinux:{version}"),
            _ => format!("docker.io/library/{name}:{version}"),
        };
        Ok(Self {
            name: name.into(),
            version: version.into(),
            image,
        })
    }
}

pub fn path() -> PathBuf {
    if let Some(path) = std::env::var_os("WEZ_VTABS_TARGETS").filter(|v| !v.is_empty()) {
        return PathBuf::from(path);
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::home().join(".config"))
        .join("wez-vtabs/targets.toml")
}

pub fn load() -> Result<Option<Config>> {
    let path = path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let config: Config =
        toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    for (name, entry) in &config.targets {
        Platform::parse(&entry.platform).with_context(|| format!("target {name}"))?;
    }
    Ok(Some(config))
}

/// Short, lowercase name of this machine; `WEZ_VTABS_HOST` overrides it.
pub fn hostname() -> String {
    let name = std::env::var("WEZ_VTABS_HOST")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| gethostname::gethostname().to_string_lossy().into_owned());
    short(&name)
}

fn short(host: &str) -> String {
    let host = host.rsplit('@').next().unwrap_or(host);
    host.split('.').next().unwrap_or(host).to_ascii_lowercase()
}

fn is_local(name: &str, host: &str) -> bool {
    let me = hostname();
    short(host) == me || name.eq_ignore_ascii_case(&me)
}

impl Config {
    fn target(&self, name: &str) -> Result<Target> {
        let entry = self
            .targets
            .get(name)
            .with_context(|| format!("unknown machine: {name} ({})", path().display()))?;
        Ok(Target {
            name: name.into(),
            host: entry.host.clone(),
            platform: Platform::parse(&entry.platform)?,
            role: entry.role,
            local: is_local(name, &entry.host),
        })
    }

    fn this(&self) -> Option<&str> {
        self.targets
            .iter()
            .find(|(name, entry)| is_local(name, &entry.host))
            .map(|(name, _)| name.as_str())
    }
}

/// Both default to this machine; `on` first falls back to this machine's or the global `on`.
pub fn plan(on: Option<&str>, to: Option<&str>) -> Result<Plan> {
    let Some(config) = load()? else {
        for name in [on, to].into_iter().flatten() {
            ensure!(name == LOCAL, "targets file missing: {}", path().display());
        }
        return Ok(Plan::default());
    };
    let this = config.this();
    let to = match to {
        None | Some(LOCAL) => this.map(|name| config.target(name)).transpose()?,
        Some(name) => Some(config.target(name)?),
    };
    let on = on.map(str::to_owned).or_else(|| {
        this.and_then(|name| config.targets[name].on.clone())
            .or_else(|| config.on.clone())
    });
    let on = match on.as_deref() {
        None | Some(LOCAL) => None,
        Some(name) => {
            let machine = config.target(name)?;
            (!machine.local).then_some(machine.host)
        }
    };
    Ok(Plan { on, to })
}

pub fn candidates() -> Vec<CompletionCandidate> {
    let local = CompletionCandidate::new(LOCAL).help(Some("this machine".into()));
    let Ok(Some(config)) = load() else {
        return vec![local];
    };
    std::iter::once(local)
        .chain(config.targets.iter().map(|(name, entry)| {
            CompletionCandidate::new(name).help(Some(
                format!("{} {} {}", entry.host, entry.platform, entry.role.name()).into(),
            ))
        }))
        .collect()
}

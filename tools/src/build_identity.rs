//! Build identities retain the on-disk format while keeping cache inputs typed.
use crate::{
    process::CommandSpec,
    source::patches,
    state::{self, Context, Role},
};
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Toolchain {
    pub rustc: String,
    pub cargo: String,
    pub project_rustc: String,
    pub project_cargo: String,
}

impl Toolchain {
    pub fn capture(ctx: &Context, worktree: &Path) -> Result<Self> {
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let versions = |cwd: &Path| -> Result<(String, String)> {
            Ok((
                ctx.runner
                    .capture(CommandSpec::new(&rustc).arg("-vV").cwd(cwd))?,
                ctx.runner
                    .capture(CommandSpec::new("cargo").arg("-V").cwd(cwd))?,
            ))
        };
        let (rustc, cargo) = versions(worktree)?;
        let (project_rustc, project_cargo) = versions(&ctx.root)?;
        Ok(Self {
            rustc,
            cargo,
            project_rustc,
            project_cargo,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CargoConfig {
    pub path: PathBuf,
    pub sha256: String,
    pub settings_toml: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LockDigests {
    pub project: Option<String>,
    pub upstream: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Compilation {
    pub version: u32,
    #[serde(flatten)]
    pub toolchain: Toolchain,
    pub target: String,
    pub profile: String,
    pub role: Role,
    pub compile_inputs: String,
    pub environment: BTreeMap<String, String>,
    pub cargo_configs: Vec<CargoConfig>,
    pub project_root: PathBuf,
    pub target_directory: PathBuf,
    pub locks: LockDigests,
    pub artifact_sha256: BTreeMap<String, String>,
    pub worktree_digest: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildConfiguration {
    #[serde(flatten)]
    pub compilation: Compilation,
    pub validation_inputs: String,
    // Network policy affects resolution, not compiled output.
    pub offline: bool,
}

impl BuildConfiguration {
    pub fn compile_digest(&self, upstream: &str) -> Result<String> {
        Ok(state::hash_bytes(&serde_json::to_vec(&json!({
            "source": self.compilation.compile_inputs,
            "upstream": upstream,
            "configuration": self.compilation,
        }))?))
    }

    pub fn validation_digest(&self, compile: &str) -> Result<String> {
        Ok(state::hash_bytes(&serde_json::to_vec(&json!({
            "compile": compile, "validation": self.validation_inputs,
        }))?))
    }
}

/// Top-level upstream directories in the path-dependency closure of `wezterm-mux-server`.
fn mux_directories(ctx: &Context, worktree: &Path) -> Result<BTreeSet<String>> {
    let output = ctx.runner.capture(
        CommandSpec::new("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--offline",
                "--format-version",
                "1",
            ])
            .cwd(worktree),
    )?;
    let metadata: serde_json::Value =
        serde_json::from_str(&output).context("invalid cargo metadata")?;
    let worktree = worktree.canonicalize()?;
    let mut packages = BTreeMap::new();
    for package in metadata["packages"]
        .as_array()
        .context("cargo metadata packages missing")?
    {
        let manifest = package["manifest_path"]
            .as_str()
            .context("package manifest missing")?;
        let directory = Path::new(manifest)
            .parent()
            .context("manifest parent missing")?
            .canonicalize()?;
        packages.insert(directory, package);
    }
    let server = packages
        .iter()
        .find(|(_, package)| package["name"] == "wezterm-mux-server")
        .map(|(directory, _)| directory.clone())
        .context("wezterm-mux-server package missing")?;
    let mut pending = vec![server];
    let mut visited = BTreeSet::new();
    let mut directories = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        let relative = directory.strip_prefix(&worktree).with_context(|| {
            format!(
                "wezterm-mux-server depends on {} outside upstream",
                directory.display()
            )
        })?;
        if let Some(top) = relative.components().next() {
            directories.insert(top.as_os_str().to_string_lossy().into_owned());
        }
        let Some(package) = packages.get(&directory) else {
            continue;
        };
        for dependency in package["dependencies"].as_array().into_iter().flatten() {
            if dependency["kind"] == "dev" {
                continue;
            }
            if let Some(path) = dependency["path"].as_str() {
                pending.push(Path::new(path).canonicalize()?);
            }
        }
    }
    Ok(directories)
}

/// Identity of the upstream sources compiled into `wezterm-mux-server`; GUI-only patches keep it.
pub fn mux_digest(ctx: &Context, worktree: &Path, revision: &str) -> Result<String> {
    let directories = mux_directories(ctx, worktree)?;
    let mux_side = |path: &str| {
        path.split_once('/')
            .is_none_or(|(top, _)| directories.contains(top))
    };
    let mut selected = BTreeMap::new();
    for patch in patches::patches(&ctx.root)? {
        let contents = fs::read(&patch)?;
        let touched = String::from_utf8_lossy(&contents)
            .lines()
            .filter_map(|line| line.strip_prefix("diff --git a/"))
            .any(|paths| {
                paths
                    .split_once(" b/")
                    .is_some_and(|(path, _)| mux_side(path))
            });
        if touched {
            let name = patch.file_name().context("patch name missing")?;
            selected.insert(
                name.to_string_lossy().into_owned(),
                state::hash_bytes(&contents),
            );
        }
    }
    let overlay = patches::overlay_files(&ctx.root)?;
    let overlay = patches::content_digests(overlay.iter().filter(|(path, _)| mux_side(path)));
    Ok(state::hash_bytes(&serde_json::to_vec(&json!({
        "upstream": revision, "directories": directories, "patches": selected, "overlay": overlay,
    }))?))
}

//! Build identities retain the on-disk format while keeping cache inputs typed.
use crate::{
    process::CommandSpec,
    state::{self, Context, Role},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
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

pub mod inventory;
pub mod patches;
pub mod project;
pub mod upstream;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::process::CommandSpec;
use crate::state::{self, Context};

#[derive(Debug, Serialize, Deserialize)]
struct Ownership {
    path: PathBuf,
    remote: String,
    capability: u32,
}

pub fn git(ctx: &Context, cwd: &Path) -> CommandSpec {
    let mut command = CommandSpec::new("git")
        .cwd(cwd)
        .env("GIT_TERMINAL_PROMPT", "0");
    if ctx.offline {
        // Prevent both submodule transports and partial-clone lazy fetching.
        command = command
            .args(["-c", "protocol.allow=never"])
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_NO_LAZY_FETCH", "1");
    }
    command
}

fn normalized_remote(remote: &str) -> &str {
    remote.trim_end_matches('/').trim_end_matches(".git")
}

fn verify_origin(ctx: &Context, repository: &Path, expected: &str) -> Result<()> {
    let origin = ctx
        .runner
        .capture(git(ctx, repository).args(["remote", "get-url", "origin"]))?;
    ensure!(
        normalized_remote(origin.trim()) == normalized_remote(expected),
        "unexpected cache remote: {} (expected {})",
        origin.trim(),
        expected
    );
    Ok(())
}

fn common_dir(ctx: &Context, repository: &Path) -> Result<PathBuf> {
    let value = ctx.runner.capture(git(ctx, repository).args([
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]))?;
    fs::canonicalize(value.trim()).context("resolve Git common directory")
}

fn verify_worktree(ctx: &Context, upstream: &Path, worktree: &Path) -> Result<()> {
    ensure!(
        fs::canonicalize(worktree)? != fs::canonicalize(upstream)?
            && common_dir(ctx, worktree)? == common_dir(ctx, upstream)?,
        "refusing to replace an unowned worktree"
    );
    Ok(())
}

fn write_ownership(path: &Path, remote: &str) -> Result<()> {
    state::write_json(
        &path.join(".git/wez-vtabs.json"),
        &Ownership {
            path: fs::canonicalize(path)?,
            remote: remote.into(),
            capability: 1,
        },
    )
}

fn verify_ownership(path: &Path, remote: &str) -> Result<bool> {
    let ownership: Option<Ownership> = state::read_json(&path.join(".git/wez-vtabs.json"))?;
    match ownership {
        Some(ownership) => {
            ensure!(
                ownership.path == fs::canonicalize(path)?
                    && ownership.remote == remote
                    && ownership.capability == 1,
                "refusing to rewrite an unowned cache: {}",
                path.display()
            );
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn validate_ref(value: &str) -> Result<&str> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && value
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || b"._/-".contains(&v))
            && !value.contains("..")
            && !value.ends_with(['/', '.'])
            && value
                .split('/')
                .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock")),
        "invalid project update branch or upstream reference: {value}"
    );
    Ok(value)
}

fn is_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn resolve_commit(ctx: &Context, repository: &Path, reference: &str) -> Result<String> {
    let revision = ctx.runner.capture(git(ctx, repository).args([
        "rev-parse",
        "--verify",
        "--end-of-options",
        &format!("{reference}^{{commit}}"),
    ]))?;
    let revision = revision.trim();
    ensure!(is_commit(revision), "Git did not resolve a full commit ID");
    Ok(revision.to_lowercase())
}

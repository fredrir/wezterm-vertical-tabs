use std::fs;
use std::path::PathBuf;

use anyhow::{Result, ensure};

use super::{git, resolve_commit, validate_ref, verify_origin, verify_ownership, write_ownership};
use crate::state::{self, Context, ProjectSource};

pub const PROJECT_URL: &str = "https://github.com/fredrir/wezterm-vertical-tabs.git";
pub const PROJECT_BRANCH: &str = "dev";

pub fn project_url() -> String {
    std::env::var("WEZ_VTABS_PROJECT_URL").unwrap_or_else(|_| PROJECT_URL.into())
}

pub fn source(ctx: &Context) -> Result<ProjectSource> {
    let recorded: Option<serde_json::Value> =
        state::read_json(&ctx.root.parent().unwrap_or(&ctx.root).join("build.json"))?;
    let recorded = recorded.as_ref().and_then(|v| v.get("project_source"));
    let remote = project_url();
    if let Some(value) = recorded {
        ensure!(value.is_object(), "invalid recorded project source");
        ensure!(
            value
                .get("remote")
                .and_then(|v| v.as_str())
                .unwrap_or(&remote)
                == remote,
            "unexpected recorded project source remote"
        );
    }
    let branch = std::env::var("WEZ_VTABS_PROJECT_BRANCH")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| recorded.and_then(|v| v.get("branch")?.as_str().map(str::to_owned)))
        .unwrap_or_else(|| PROJECT_BRANCH.into());
    validate_ref(&branch)?;
    let revision = if ctx.root.join(".git").exists() {
        Some(resolve_commit(ctx, &ctx.root, "HEAD")?)
    } else {
        recorded.and_then(|v| v.get("revision")?.as_str().map(str::to_owned))
    };
    let source = ProjectSource {
        remote,
        branch,
        revision,
    };
    ctx.runner
        .metadata("project_source", &serde_json::to_value(&source)?)?;
    Ok(source)
}

/// The installed updater's own checkout of `branch`, reset to its remote head.
pub fn refresh(ctx: &Context, branch: &str) -> Result<PathBuf> {
    validate_ref(branch)?;
    let checkout = ctx.cache.join("project");
    let remote = project_url();
    fs::create_dir_all(&ctx.cache)?;
    if !checkout.exists() {
        ensure!(
            !ctx.offline,
            "project source is not cached for offline update"
        );
        ctx.runner.run(
            git(ctx, &ctx.root)
                .args([
                    "clone",
                    "--filter=blob:none",
                    "--single-branch",
                    "--branch",
                    branch,
                    &remote,
                ])
                .arg(&checkout),
        )?;
        write_ownership(&checkout, &remote)?;
    } else {
        ensure!(
            !fs::symlink_metadata(&checkout)?.file_type().is_symlink()
                && verify_ownership(&checkout, &remote)?,
            "refusing to rewrite an unowned project cache; choose an empty WEZ_VTABS_CACHE"
        );
    }
    verify_origin(ctx, &checkout, &remote)?;
    let reference = format!("refs/remotes/origin/{branch}");
    if !ctx.offline {
        ctx.runner.run(git(ctx, &checkout).args([
            "fetch",
            "--prune",
            "origin",
            &format!("+refs/heads/{branch}:{reference}"),
        ]))?;
    }
    ctx.runner
        .run(git(ctx, &checkout).args(["reset", "--hard", &reference]))?;
    ctx.runner
        .run(git(ctx, &checkout).args(["clean", "-ffd"]))?;
    ensure!(
        ["Cargo.toml", "src/app/Cargo.toml", "tools/Cargo.toml"]
            .iter()
            .all(|name| checkout.join(name).is_file())
            && !super::patches::patches(&checkout)?.is_empty(),
        "project branch {branch} does not contain a complete implementation"
    );
    Ok(checkout)
}

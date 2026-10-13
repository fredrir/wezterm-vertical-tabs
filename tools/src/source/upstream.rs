use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::{
    git, is_commit, resolve_commit, validate_ref, verify_origin, verify_ownership, verify_worktree,
    write_ownership,
};
use crate::state::{self, Context};

pub const UPSTREAM_URL: &str = "https://github.com/wezterm/wezterm.git";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSource {
    pub checkout: PathBuf,
    pub revision: String,
}

pub fn upstream_url() -> String {
    std::env::var("WEZ_VTABS_UPSTREAM_URL").unwrap_or_else(|_| UPSTREAM_URL.into())
}

fn fetch(ctx: &Context, upstream: &Path, refspec: &str) -> Result<()> {
    ctx.runner
        .run(git(ctx, upstream).args(["fetch", "--no-tags", "origin", refspec]))
}

pub fn resolve(ctx: &Context) -> Result<ResolvedSource> {
    let _stage = ctx.runner.stage("resolve");
    let upstream = ctx.cache.join("upstream");
    let remote = upstream_url();
    let explicit = ctx.upstream.as_deref().map(validate_ref).transpose()?;
    fs::create_dir_all(&ctx.cache)?;
    let cloned = !upstream.exists();
    if cloned {
        ensure!(
            !ctx.offline,
            "offline upstream cache missing; prepare once online"
        );
        ctx.runner.run(
            git(ctx, &ctx.root)
                .args([
                    "clone",
                    "--filter=blob:none",
                    "--single-branch",
                    "--branch",
                    "main",
                    &remote,
                ])
                .arg(&upstream),
        )?;
        write_ownership(&upstream, &remote)?;
    } else {
        ensure!(
            !fs::symlink_metadata(&upstream)?.file_type().is_symlink(),
            "refusing a symlinked upstream cache"
        );
        verify_origin(ctx, &upstream, &remote)?;
        ensure!(
            verify_ownership(&upstream, &remote)?,
            "refusing to rewrite an unowned upstream cache; choose an empty WEZ_VTABS_CACHE"
        );
    }
    let revision = match explicit {
        Some(reference) if ctx.offline || is_commit(reference) => {
            match resolve_commit(ctx, &upstream, reference) {
                Ok(revision) => revision,
                Err(error) if ctx.offline => {
                    return Err(error.context("requested revision is unavailable offline"));
                }
                Err(_) => {
                    fetch(ctx, &upstream, reference)?;
                    resolve_commit(ctx, &upstream, reference)?
                }
            }
        }
        Some(reference) => {
            fetch(ctx, &upstream, reference)?;
            resolve_commit(ctx, &upstream, "FETCH_HEAD")?
        }
        None => {
            let main = "refs/remotes/origin/main";
            let iterating = ctx.profile == "iterate" || ctx.profile == "dev";
            if !ctx.offline
                && !cloned
                && (!iterating || resolve_commit(ctx, &upstream, main).is_err())
            {
                ctx.runner.run(git(ctx, &upstream).args([
                    "fetch",
                    "--prune",
                    "origin",
                    "+refs/heads/main:refs/remotes/origin/main",
                ]))?;
            }
            resolve_commit(ctx, &upstream, main)?
        }
    };
    let resolved = ResolvedSource {
        checkout: upstream,
        revision,
    };
    ctx.runner
        .metadata("upstream", &serde_json::to_value(&resolved)?)?;
    Ok(resolved)
}

/// Copy locally owned Git objects into an isolated offline reproduction cache.
pub fn seed_replay_cache(
    ctx: &Context,
    destination: &Path,
    revision: &str,
    remote: &str,
) -> Result<()> {
    let _stage = ctx.runner.stage("seed_replay");
    let _lock = ctx.lock_build()?;
    ensure!(
        is_commit(revision),
        "offline reproduction requires a full recorded upstream revision"
    );
    let upstream = ctx.cache.join("upstream");
    ensure!(
        upstream.is_dir()
            && !fs::symlink_metadata(&upstream)?.file_type().is_symlink()
            && verify_ownership(&upstream, remote)?,
        "offline reproduction needs an owned upstream cache; first prepare {revision} online using --cache {}",
        ctx.cache.display()
    );
    let mut local = ctx.clone();
    local.offline = true;
    verify_origin(&local, &upstream, remote)?;
    ensure!(
        resolve_commit(&local, &upstream, revision).ok().as_deref() == Some(revision),
        "recorded upstream {revision} is unavailable locally; prepare it online before offline reproduction"
    );
    ensure!(
        !upstream.join(".git/objects/info/alternates").exists(),
        "offline reproduction requires self-contained cached Git objects; upstream uses object alternates"
    );
    fs::create_dir_all(destination)?;
    let target_upstream = destination.join("upstream");
    ensure!(
        !target_upstream.exists(),
        "reproduction upstream destination already exists"
    );
    // A file-only protocol allowlist keeps inherited Git configuration off the network.
    ctx.runner.run(
        git(&local, &ctx.root)
            .env("GIT_ALLOW_PROTOCOL", "file")
            .args(["clone", "--local", "--no-hardlinks", "--no-checkout"])
            .arg(&upstream)
            .arg(&target_upstream),
    )?;
    ctx.runner
        .run(git(&local, &target_upstream).args(["remote", "set-url", "origin", remote]))?;
    write_ownership(&target_upstream, remote)?;
    let target_worktree = destination.join("worktree");
    ctx.runner.run(git(&local, &target_upstream).args(["worktree", "add", "--detach"])
        .arg(&target_worktree).arg(revision))
        .context("recorded revision has uncached partial-clone objects; prepare it online before offline reproduction")?;
    let source_worktree = ctx.cache.join("worktree");
    if source_worktree.join(".git").is_file() {
        verify_worktree(&local, &upstream, &source_worktree)?;
        let source_git = PathBuf::from(
            ctx.runner
                .capture(git(&local, &source_worktree).args(["rev-parse", "--absolute-git-dir"]))?,
        );
        let target_git = PathBuf::from(
            ctx.runner
                .capture(git(&local, &target_worktree).args(["rev-parse", "--absolute-git-dir"]))?,
        );
        let source_modules = source_git.join("modules");
        let target_modules = target_git.join("modules");
        if source_modules.is_dir() {
            let source_root = fs::canonicalize(&source_worktree)?;
            let mut worktrees = Vec::new();
            for entry in WalkDir::new(&source_modules).follow_links(false) {
                let entry = entry?;
                ensure!(
                    !entry.file_type().is_symlink(),
                    "offline reproduction requires a cache without module-store symlinks"
                );
                ensure!(
                    !entry.path().ends_with("objects/info/alternates"),
                    "offline reproduction requires self-contained cached submodule objects"
                );
                if entry.file_name() == "config"
                    && entry
                        .path()
                        .parent()
                        .is_some_and(|v| v.join("objects").is_dir())
                {
                    let repository = entry
                        .path()
                        .parent()
                        .context("submodule repository missing")?;
                    let current = ctx.runner.capture(
                        git(&local, &ctx.root)
                            .arg("--git-dir")
                            .arg(repository)
                            .args(["config", "--local", "--get", "core.worktree"]),
                    )?;
                    let current = repository.join(current);
                    let current = fs::canonicalize(current).context(
                        "offline reproduction needs a populated cached submodule checkout",
                    )?;
                    let relative = current.strip_prefix(&source_root).context(
                        "refusing a cached submodule worktree outside the owned source cache",
                    )?;
                    worktrees.push((
                        repository.strip_prefix(&source_modules)?.to_path_buf(),
                        relative.to_path_buf(),
                    ));
                }
            }
            crate::bundle::copy_tree(&source_modules, &target_modules)?;
            for (repository, relative) in worktrees {
                ctx.runner.run(
                    git(&local, &ctx.root)
                        .arg("--git-dir")
                        .arg(target_modules.join(repository))
                        .args(["config", "--local", "core.worktree"])
                        .arg(target_worktree.join(relative)),
                )?;
            }
        }
    }
    let prepared = serde_json::json!({"seeded_for_reproduction":true,"upstream":revision});
    state::write_json(&destination.join("prepared.json"), &prepared)?;
    ctx.runner.metadata(
        "replay_cache",
        &serde_json::json!({"revision":revision,"cache":destination,"offline":true}),
    )?;
    Ok(())
}

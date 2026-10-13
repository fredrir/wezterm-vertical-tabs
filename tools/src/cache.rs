use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use crate::process::RunReport;
use crate::state::{self, BuildMetadata, Context, Lock, read_json};

fn tree_size(path: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_file() {
            size += entry.metadata()?.len();
        }
    }
    Ok(size)
}

#[derive(Clone, Copy, Debug)]
pub struct Retention {
    pub runs: usize,
    pub bundles: usize,
    pub versions: usize,
}

impl Retention {
    /// Kept beyond active, pending, previous and running versions, active/pending bundles and the current run.
    pub const AUTOMATIC: Self = Self {
        runs: 10,
        bundles: 0,
        versions: 0,
    };

    pub fn uniform(keep: usize) -> Self {
        Self {
            runs: keep,
            bundles: keep,
            versions: keep,
        }
    }
}

/// Runs after every install; a caller holding the build lock passes it to avoid acquiring it twice.
pub fn prune(ctx: &Context, build_lock: Option<&Lock>) -> Result<Value> {
    let _lock = match build_lock {
        Some(_) => None,
        None => Some(ctx.lock_build()?),
    };
    let report = collect(ctx, true, false, Retention::AUTOMATIC)?;
    let removed = report["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry["action"] == "removed")
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(json!({
        "removed": removed.iter().map(|entry| entry["path"].clone()).collect::<Vec<_>>(),
        "freed_bytes": removed.iter().filter_map(|entry| entry["bytes"].as_u64()).sum::<u64>(),
    }))
}

pub fn cache(ctx: &Context, gc: bool, dry_run: bool, retention: Retention) -> Result<Value> {
    let _lock = if gc { Some(ctx.lock_build()?) } else { None };
    collect(ctx, gc, dry_run, retention)
}

fn collect(ctx: &Context, gc: bool, dry_run: bool, retention: Retention) -> Result<Value> {
    let mut protected_bundles = BTreeSet::new();
    let mut protected_versions = BTreeSet::new();
    for name in ["active", "pending", "previous"] {
        if let Some(value) = read_json::<Value>(&ctx.install.join(format!("{name}.json")))?
            && let Some(id) = value["id"].as_str()
        {
            protected_versions.insert(id.to_owned());
            if name != "previous" {
                protected_bundles.insert(format!("wez-vtabs-{id}"));
            }
        }
    }
    let versions = ctx.install.join("versions");
    let _install_lock = if gc && versions.is_dir() {
        Some(Lock::acquire(&ctx.install.join("install.lock"))?)
    } else {
        None
    };
    let mut entries = Vec::new();
    for (category, directory, keep) in [
        ("runs", ctx.cache.join("runs"), retention.runs),
        ("bundles", ctx.cache.join("bundles"), retention.bundles),
        ("versions", versions, retention.versions),
    ] {
        if !directory.is_dir() {
            continue;
        }
        let mut children = fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|entry| {
            std::cmp::Reverse(
                entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok(),
            )
        });
        let mut retained = 0;
        for child in children {
            let path = child.path();
            let name = child.file_name().to_string_lossy().into_owned();
            if child.file_type()?.is_symlink()
                || !child.file_type()?.is_dir()
                || name.starts_with('.')
            {
                continue;
            }
            let marker = match category {
                "runs" => Some("run.json"),
                "bundles" => Some("build.json"),
                // Incomplete versions are leftovers too; only the pointers decide.
                _ => None,
            };
            if marker.is_some_and(|marker| !path.join(marker).is_file()) {
                continue;
            }
            if category == "versions" && state::safe_id(&name).is_err() {
                continue;
            }
            let active = path == ctx.runner.directory()
                || (category == "bundles" && protected_bundles.contains(&name))
                || (category == "versions" && protected_versions.contains(&name))
                || (category == "runs"
                    && read_json::<RunReport>(&path.join("run.json"))?
                        .is_some_and(|report| report.status == "running"));
            let lease_path = if category == "versions" {
                state::lease(&ctx.install, &name)
            } else {
                state::lease(&ctx.cache, &name)
            };
            let lease = Lock::try_acquire(&lease_path)?;
            let install_lease = if category == "bundles" {
                if let Some(metadata) = BuildMetadata::read(&path)? {
                    state::safe_id(&metadata.id)?;
                    Some(Lock::try_acquire(&state::lease(
                        &ctx.install,
                        &metadata.id,
                    ))?)
                } else {
                    None
                }
            } else {
                None
            };
            let protected =
                active || lease.is_none() || install_lease.as_ref().is_some_and(Option::is_none);
            let remove = gc && !protected && retained >= keep;
            if !protected {
                retained += 1;
            }
            let bytes = tree_size(&path)?;
            entries.push(json!({"category":category,"path":path,"bytes":bytes,"protected":protected,"action":if remove {if dry_run {"would_remove"}else{"removed"}}else{"keep"}}));
            if remove && !dry_run {
                fs::remove_dir_all(&path)?;
                if category == "bundles" {
                    for suffix in [
                        ".zip",
                        ".tar.gz",
                        ".zip.manifest.json",
                        ".tar.gz.manifest.json",
                    ] {
                        let archive = path.with_file_name(format!("{name}{suffix}"));
                        if archive.is_file()
                            && !fs::symlink_metadata(&archive)?.file_type().is_symlink()
                        {
                            fs::remove_file(archive)?;
                        }
                    }
                }
                drop(lease);
                let _ = fs::remove_file(&lease_path);
            }
        }
    }
    Ok(json!({"cache":ctx.cache,"install":ctx.install,"dry_run":dry_run,"entries":entries}))
}

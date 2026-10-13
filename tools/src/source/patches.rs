use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::inventory::{IGNORED, digest_entry, digest_paths, is_ignored, walk_files};
use super::upstream::{ResolvedSource, resolve};
use super::{git, resolve_commit, verify_worktree};
use crate::process::CommandSpec;
use crate::state::{self, Context};

const PREPARATION_VERSION: u32 = 3;
const PATCHES: &str = "wezterm/patches";
const OVERLAY: &str = "wezterm/overlay";
/// Upstream paths the tool rewrites; patches and the overlay never contain them.
const TOOL_MANAGED: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "wezterm-gui/Cargo.toml",
    "wezterm-gui/src/vtabs",
];

#[derive(Debug, Serialize, Deserialize)]
struct Prepared {
    version: u32,
    upstream: String,
    patch_digest: String,
    root: PathBuf,
    #[serde(default)]
    overlay: BTreeMap<String, String>,
}

fn prepared_record(ctx: &Context) -> PathBuf {
    ctx.cache.join("prepared.json")
}

/// Records from other preparation versions read as absent.
fn read_prepared(ctx: &Context) -> Result<Option<Prepared>> {
    let raw: Option<serde_json::Value> = state::read_json(&prepared_record(ctx))?;
    Ok(raw.and_then(|value| serde_json::from_value::<Prepared>(value).ok()))
}

pub(crate) fn patches(root: &Path) -> Result<Vec<PathBuf>> {
    let mut patches = fs::read_dir(root.join(PATCHES))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    patches.retain(|path| path.is_file() && path.extension().is_some_and(|v| v == "patch"));
    patches.sort();
    ensure!(!patches.is_empty(), "patches missing");
    Ok(patches)
}

fn tool_managed(path: &str) -> bool {
    TOOL_MANAGED.iter().any(|managed| {
        path.strip_prefix(managed)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Overlay files keyed by their upstream path.
pub(crate) fn overlay_files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let directory = root.join(OVERLAY);
    let mut files = BTreeSet::new();
    if directory.exists() {
        walk_files(&directory, &mut files)?;
    }
    let mut overlay = BTreeMap::new();
    for path in files {
        let relative = path
            .strip_prefix(&directory)?
            .to_str()
            .with_context(|| format!("non-UTF-8 path: {}", path.display()))?
            .replace('\\', "/");
        ensure!(
            !tool_managed(&relative),
            "overlay file in tool-managed path: {relative}"
        );
        overlay.insert(relative, fs::read(&path)?);
    }
    Ok(overlay)
}

pub(crate) fn content_digests<'a>(
    files: impl IntoIterator<Item = (&'a String, &'a Vec<u8>)>,
) -> BTreeMap<String, String> {
    files
        .into_iter()
        .map(|(path, contents)| (path.clone(), state::hash_bytes(contents)))
        .collect()
}

/// The given paths that exist in `revision`, as a file or a directory.
fn upstream_paths<'a>(
    ctx: &Context,
    worktree: &Path,
    revision: &str,
    paths: &BTreeSet<&'a str>,
) -> Result<BTreeSet<&'a str>> {
    if paths.is_empty() {
        return Ok(BTreeSet::new());
    }
    let listed = ctx.runner.capture(
        git(ctx, worktree)
            .args(["ls-tree", "-r", "--name-only", "-z", revision, "--"])
            .args(paths),
    )?;
    let listed = listed.split('\0').collect::<Vec<_>>();
    Ok(paths
        .iter()
        .copied()
        .filter(|path| {
            listed.iter().any(|entry| {
                entry
                    .strip_prefix(path)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
            })
        })
        .collect())
}

fn write_changed(destination: &Path, contents: &[u8]) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok_and(|v| v.file_type().is_symlink()) {
        fs::remove_file(destination)?;
    }
    if fs::read(destination).ok().as_deref() != Some(contents) {
        fs::create_dir_all(destination.parent().context("destination has no parent")?)?;
        fs::write(destination, contents)?;
    }
    Ok(())
}

fn remove_empty_parents(stop: &Path, path: &Path) {
    for parent in path.ancestors().skip(1).take_while(|v| *v != stop) {
        if fs::remove_dir(parent).is_err() {
            break;
        }
    }
}

/// Copies overlay files and removes previously synced ones the project no longer has.
fn sync_overlay(
    ctx: &Context,
    worktree: &Path,
    revision: &str,
    overlay: &BTreeMap<String, Vec<u8>>,
    previous: &BTreeMap<String, String>,
) -> Result<()> {
    let stale = previous
        .keys()
        .filter(|path| !overlay.contains_key(*path))
        .collect::<Vec<_>>();
    let paths = overlay
        .keys()
        .chain(stale.iter().copied())
        .map(String::as_str)
        .collect();
    let upstream = upstream_paths(ctx, worktree, revision, &paths)?;
    if let Some(path) = overlay.keys().find(|path| upstream.contains(path.as_str())) {
        bail!("overlay file tracked upstream: {path}");
    }
    for path in stale {
        let target = worktree.join(path);
        if !upstream.contains(path.as_str()) && target.is_file() {
            fs::remove_file(&target)?;
            remove_empty_parents(worktree, &target);
        }
    }
    for (path, contents) in overlay {
        write_changed(&worktree.join(path), contents)?;
    }
    Ok(())
}

fn copy_changed(source: &Path, destination: &Path) -> Result<()> {
    let contents = fs::read(source)?;
    if fs::symlink_metadata(destination).is_ok_and(|v| v.file_type().is_symlink()) {
        fs::remove_file(destination)?;
    }
    if fs::read(destination).ok().as_deref() != Some(contents.as_slice()) {
        fs::create_dir_all(destination.parent().context("destination has no parent")?)?;
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn sync_directory(source: &Path, destination: &Path) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok_and(|v| v.file_type().is_symlink()) {
        if let (Ok(dest_can), Ok(src_can)) =
            (fs::canonicalize(destination), fs::canonicalize(source))
            && dest_can == src_can
        {
            return Ok(());
        }
        fs::remove_file(destination)?;
    }
    fs::create_dir_all(destination)?;
    let mut expected = BTreeSet::new();
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if IGNORED.iter().any(|v| name == *v) {
            continue;
        }
        expected.insert(name.clone());
        let target = destination.join(name);
        ensure!(
            !entry.file_type()?.is_symlink(),
            "adapter symlinks are unsupported"
        );
        if entry.file_type()?.is_dir() {
            if target.is_file() {
                fs::remove_file(&target)?;
            }
            sync_directory(&entry.path(), &target)?;
        } else {
            if target.is_dir() {
                fs::remove_dir_all(&target)?;
            }
            copy_changed(&entry.path(), &target)?;
        }
    }
    for entry in fs::read_dir(destination)? {
        let entry = entry?;
        if !expected.contains(&entry.file_name()) {
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

fn stage_adapter(root: &Path, worktree: &Path) -> Result<()> {
    let adapter = root.join("src/adapter");
    ensure!(
        adapter.join("src/lib.rs").is_file(),
        "src/adapter/src/lib.rs missing"
    );
    let gui = worktree.join("wezterm-gui");
    let adapter_manifest = fs::read_to_string(adapter.join("Cargo.toml"))?
        .parse::<toml_edit::DocumentMut>()
        .context("parse adapter manifest")?;
    let additions = adapter_manifest
        .get("dependencies")
        .and_then(toml_edit::Item::as_table)
        .context("adapter dependency section missing")?;
    sync_directory(&adapter, &gui.join("src/vtabs"))?;
    let manifest = gui.join("Cargo.toml");
    let original = fs::read_to_string(&manifest)?;
    let mut document = original
        .parse::<toml_edit::DocumentMut>()
        .context("parse WezTerm GUI manifest")?;
    let dependencies = document
        .get_mut("dependencies")
        .and_then(toml_edit::Item::as_table_mut)
        .context("WezTerm GUI dependency section changed")?;
    for (name, value) in additions.iter() {
        let mut dependency = value.clone();
        if let Some(relative) = dependency.get("path").and_then(toml_edit::Item::as_str) {
            let path = fs::canonicalize(adapter.join(relative))
                .with_context(|| format!("resolve adapter dependency {name}"))?;
            dependency["path"] = toml_edit::value(path.to_string_lossy().to_string());
        }
        dependencies.insert(name, dependency);
    }
    let updated = document.to_string();
    if updated != original {
        fs::write(&manifest, updated)?;
    }
    configure_dev_profile(worktree)?;
    Ok(())
}

fn configure_dev_profile(worktree: &Path) -> Result<()> {
    let manifest = worktree.join("Cargo.toml");
    let original = fs::read_to_string(&manifest)?;
    let mut document = original.parse::<toml_edit::DocumentMut>()?;
    // Profile settings belong to the workspace compiling WezTerm.
    document["profile"]["iterate"]["inherits"] = toml_edit::value("dev");
    document["profile"]["iterate"]["opt-level"] = toml_edit::value(1);
    document["profile"]["iterate"]["debug"] = toml_edit::value("line-tables-only");
    document["profile"]["iterate"]["incremental"] = toml_edit::value(true);
    document["profile"]["iterate"]["lto"] = toml_edit::value("off");
    let updated = document.to_string();
    if updated != original {
        fs::write(manifest, updated)?;
    }
    Ok(())
}

fn apply_patches(ctx: &Context, worktree: &Path, patches: &[PathBuf]) -> Result<Vec<String>> {
    let mut merged = Vec::new();
    for patch in patches {
        if ctx
            .runner
            .run(git(ctx, worktree).args(["apply", "--check"]).arg(patch))
            .is_ok()
        {
            ctx.runner.run(git(ctx, worktree).arg("apply").arg(patch))?;
            continue;
        }
        let name = patch
            .file_name()
            .context("patch has no name")?
            .to_string_lossy()
            .into_owned();
        // --3way implies --index, which must match files earlier patches changed.
        ctx.runner
            .run(git(ctx, worktree).args(["add", "--update"]))?;
        ctx.runner
            .run(git(ctx, worktree).args(["apply", "--3way"]).arg(patch))
            .with_context(|| format!("{name} does not apply"))?;
        merged.push(name);
    }
    if !merged.is_empty() {
        ctx.runner
            .run(git(ctx, worktree).args(["reset", "--quiet"]))?;
    }
    Ok(merged)
}

/// Git whose output ignores user and system configuration.
fn plain_git(ctx: &Context, cwd: &Path) -> CommandSpec {
    git(ctx, cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

pub fn prepare(ctx: &Context, resolved: &ResolvedSource) -> Result<PathBuf> {
    let _stage = ctx.runner.stage("prepare");
    let worktree = ctx.cache.join("worktree");
    let patch_files = patches(&ctx.root)?;
    let patch_digest = digest_paths(&ctx.root, &patch_files)?;
    let overlay = overlay_files(&ctx.root)?;
    let previous = read_prepared(ctx)?;
    let root = fs::canonicalize(&ctx.root)?;
    let mut reuse = false;
    let mut merged = Vec::new();
    if worktree.exists() {
        verify_worktree(ctx, &resolved.checkout, &worktree)?;
        reuse = previous.as_ref().is_some_and(|previous| {
            previous.version == PREPARATION_VERSION
                && previous.upstream == resolved.revision
                && previous.patch_digest == patch_digest
                && previous.root == root
        }) && resolve_commit(ctx, &worktree, "HEAD")? == resolved.revision;
    }
    if !reuse {
        let record = prepared_record(ctx);
        if record.exists() {
            fs::remove_file(record)?;
        }
        if worktree.exists() {
            // Resetting in place keeps the submodule object stores that cached offline builds need.
            ctx.runner.run(
                git(ctx, &worktree)
                    .args(["reset", "--hard"])
                    .arg(&resolved.revision),
            )?;
            ctx.runner
                .run(git(ctx, &worktree).args(["clean", "-ffd"]))?;
        } else {
            ctx.runner
                .run(git(ctx, &resolved.checkout).args(["worktree", "prune"]))?;
            ctx.runner.run(
                git(ctx, &resolved.checkout)
                    .args(["worktree", "add", "--detach"])
                    .arg(&worktree)
                    .arg(&resolved.revision),
            )?;
        }
        let mut submodules =
            git(ctx, &worktree).args(["submodule", "update", "--init", "--recursive"]);
        submodules = if ctx.offline {
            submodules.arg("--no-fetch")
        } else {
            submodules.args(["--depth", "1"])
        };
        ctx.runner.run(submodules).with_context(|| if ctx.offline {
            "required submodule revision is unavailable offline; prepare this revision online once"
        } else { "initialize upstream submodules" })?;
        merged = apply_patches(ctx, &worktree, &patch_files)?;
    }
    // Always compare contents: preserves mtimes and repairs interrupted staging.
    let synced = previous.map(|v| v.overlay).unwrap_or_default();
    sync_overlay(ctx, &worktree, &resolved.revision, &overlay, &synced)?;
    stage_adapter(&ctx.root, &worktree)?;
    state::write_json(
        &prepared_record(ctx),
        &Prepared {
            version: PREPARATION_VERSION,
            upstream: resolved.revision.clone(),
            patch_digest,
            root,
            overlay: content_digests(&overlay),
        },
    )?;
    if ctx.explain {
        eprintln!(
            "prepare: {}",
            if reuse {
                "reuse patches; synchronize changed overlay and adapter files".into()
            } else if merged.is_empty() {
                "created patched worktree".into()
            } else {
                format!(
                    "created patched worktree; 3-way merged {}",
                    merged.join(", ")
                )
            }
        );
    }
    Ok(worktree)
}

pub fn check(ctx: &Context) -> Result<serde_json::Value> {
    let resolved = resolve(ctx)?;
    let patch_files = patches(&ctx.root)?;
    let overlay = overlay_files(&ctx.root)?;
    let temporary = tempfile::Builder::new()
        .prefix("patch-check-")
        .tempdir_in(&ctx.cache)?;
    let checkout = temporary.path().join("worktree");
    ctx.runner.run(
        git(ctx, &resolved.checkout)
            .args(["worktree", "add", "--detach"])
            .arg(&checkout)
            .arg(&resolved.revision),
    )?;
    let checked = (|| -> Result<Vec<String>> {
        let merged = apply_patches(ctx, &checkout, &patch_files)?;
        sync_overlay(
            ctx,
            &checkout,
            &resolved.revision,
            &overlay,
            &BTreeMap::new(),
        )?;
        stage_adapter(&ctx.root, &checkout)?;
        Ok(merged)
    })();
    let removed = ctx.runner.run(
        git(ctx, &resolved.checkout)
            .args(["worktree", "remove", "--force", "--force"])
            .arg(&checkout),
    );
    let merged = checked?;
    removed?;
    Ok(
        serde_json::json!({"upstream":resolved.revision,"patches":patch_files.len(),"overlay":overlay.len(),"merged":merged,"status":"compatible"}),
    )
}

/// Writes the prepared worktree back as one patch per top-level upstream entry plus the overlay.
pub fn export(ctx: &Context) -> Result<serde_json::Value> {
    let _stage = ctx.runner.stage("export");
    let worktree = ctx.cache.join("worktree");
    let mut prepared = read_prepared(ctx)?
        .filter(|v| v.version == PREPARATION_VERSION && worktree.join(".git").is_file())
        .context("no prepared worktree")?;
    ensure!(
        prepared.root == fs::canonicalize(&ctx.root)?,
        "worktree prepared for {}",
        prepared.root.display()
    );
    verify_worktree(ctx, &ctx.cache.join("upstream"), &worktree)?;
    let revision = prepared.upstream.clone();
    ensure!(
        resolve_commit(ctx, &worktree, "HEAD")? == revision,
        "worktree moved from the prepared revision"
    );
    let existing = patches(&ctx.root)?;
    let overlay = overlay_files(&ctx.root)?;
    ensure!(
        digest_paths(&ctx.root, &existing)? == prepared.patch_digest
            && content_digests(&overlay) == prepared.overlay,
        "patches or overlay changed since prepare"
    );
    // Unstage everything so additions stay untracked and diffs cover upstream files only.
    ctx.runner
        .run(git(ctx, &worktree).args(["reset", "--quiet"]))?;
    let changed = ctx.runner.capture(plain_git(ctx, &worktree).args([
        "diff",
        "--name-only",
        "-z",
        "--no-renames",
        "--ignore-submodules=all",
        &revision,
    ]))?;
    let entries = changed
        .split('\0')
        .filter(|path| !path.is_empty() && !tool_managed(path))
        .map(|path| path.split('/').next().unwrap_or(path))
        .collect::<BTreeSet<_>>();
    let mut patch_outputs = BTreeMap::new();
    for entry in entries {
        let patch = ctx.runner.capture_bytes(
            plain_git(ctx, &worktree)
                .args([
                    "diff",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--ignore-submodules=all",
                    "--full-index",
                    "--diff-algorithm=myers",
                    "--indent-heuristic",
                    "-U3",
                    "--inter-hunk-context=0",
                    "--src-prefix=a/",
                    "--dst-prefix=b/",
                    &revision,
                    "--",
                    entry,
                ])
                .args(TOOL_MANAGED.iter().map(|path| format!(":(exclude){path}"))),
        )?;
        if !patch.is_empty() {
            patch_outputs.insert(format!("{PATCHES}/{entry}.patch"), patch);
        }
    }
    ensure!(
        !patch_outputs.is_empty(),
        "worktree has no upstream changes"
    );
    let untracked = ctx.runner.capture(git(ctx, &worktree).args([
        "ls-files",
        "--others",
        "--exclude-standard",
        "-z",
    ]))?;
    let additions = untracked
        .split('\0')
        .filter(|path| !path.is_empty() && !tool_managed(path) && !is_ignored(Path::new(path)));
    let mut overlay_outputs = BTreeMap::new();
    for path in additions.chain(prepared.overlay.keys().map(String::as_str)) {
        let source = worktree.join(path);
        if source.is_file() {
            overlay_outputs.insert(path.to_owned(), fs::read(source)?);
        }
    }
    let outputs = patch_outputs
        .iter()
        .map(|(path, contents)| (path.clone(), contents))
        .chain(
            overlay_outputs
                .iter()
                .map(|(path, contents)| (format!("{OVERLAY}/{path}"), contents)),
        )
        .collect::<BTreeMap<_, _>>();
    // Stage every file before replacing any, so a failure leaves no partial file.
    let mut staged = Vec::new();
    for (path, contents) in &outputs {
        let target = ctx.root.join(path);
        if fs::read(&target).ok().as_deref() == Some(contents.as_slice()) {
            continue;
        }
        let parent = target.parent().context("export path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut builder = tempfile::Builder::new();
        if let Ok(metadata) = fs::metadata(&target) {
            builder.permissions(metadata.permissions());
        } else {
            #[cfg(unix)]
            builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
        }
        let mut temporary = builder.tempfile_in(parent)?;
        temporary.write_all(contents)?;
        staged.push((path, temporary));
    }
    let removed = existing
        .iter()
        .filter_map(|path| Some(format!("{PATCHES}/{}", path.file_name()?.to_str()?)))
        .chain(overlay.into_keys().map(|path| format!("{OVERLAY}/{path}")))
        .filter(|path| !outputs.contains_key(path))
        .collect::<Vec<_>>();
    let mut written = Vec::new();
    for (path, temporary) in staged {
        temporary
            .persist(ctx.root.join(path))
            .with_context(|| format!("write {path}"))?;
        written.push(path.clone());
    }
    for path in &removed {
        let target = ctx.root.join(path);
        fs::remove_file(&target)?;
        remove_empty_parents(&ctx.root.join(OVERLAY), &target);
    }
    // By construction the worktree is upstream plus these files, so prepare can reuse it.
    let mut digest = Sha256::new();
    for (path, contents) in &patch_outputs {
        digest_entry(&mut digest, path, contents);
    }
    prepared.patch_digest = state::hex(digest.finalize());
    prepared.overlay = content_digests(&overlay_outputs);
    state::write_json(&prepared_record(ctx), &prepared)?;
    Ok(
        serde_json::json!({"upstream":revision,"patches":patch_outputs.len(),"overlay":overlay_outputs.len(),"written":written,"removed":removed}),
    )
}

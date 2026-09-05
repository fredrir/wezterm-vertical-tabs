use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::process::CommandSpec;
use crate::state::{self, BuildMetadata, Context, ProjectSource};

pub const UPSTREAM_URL: &str = "https://github.com/wezterm/wezterm.git";
pub const PROJECT_URL: &str = "https://github.com/fredrir/wezterm-vertical-tabs.git";
pub const PROJECT_BRANCH: &str = "native";
const PREPARATION_VERSION: u32 = 2;
const SOURCE_ITEMS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    "pyproject.toml",
    "uv.lock",
    "ruff.toml",
    "rustfmt.toml",
    "stylua.toml",
    ".editorconfig",
    ".cargo",
    ".github",
    "README.md",
    "justfile",
    "crates",
    "native",
    "plugin",
    "docs",
    "tools",
    "scripts/native.py",
    "tests",
];
const IGNORED: &[&str] = &[
    "target",
    "__pycache__",
    ".git",
    ".pytest_cache",
    ".ruff_cache",
    ".mypy_cache",
    ".venv",
    "node_modules",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSource {
    pub checkout: PathBuf,
    pub revision: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Ownership {
    path: PathBuf,
    remote: String,
    capability: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Prepared {
    version: u32,
    upstream: String,
    patch_digest: String,
    adapter_digest: String,
    root: PathBuf,
}

pub fn upstream_url() -> String {
    std::env::var("WEZ_VTABS_UPSTREAM_URL").unwrap_or_else(|_| UPSTREAM_URL.into())
}

pub fn project_url() -> String {
    std::env::var("WEZ_VTABS_PROJECT_URL").unwrap_or_else(|_| PROJECT_URL.into())
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

pub fn source_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for item in SOURCE_ITEMS {
        let path = root.join(item);
        if !path.exists() {
            continue;
        }
        for entry in WalkDir::new(&path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !IGNORED.iter().any(|ignored| entry.file_name() == *ignored))
        {
            let entry = entry.with_context(|| format!("read source {}", path.display()))?;
            ensure!(
                !entry.file_type().is_symlink(),
                "source symlinks are unsupported: {}",
                entry.path().display()
            );
            if entry.file_type().is_file() {
                files.insert(entry.into_path());
            }
        }
    }
    Ok(files.into_iter().collect())
}

fn digest_paths<'a>(root: &Path, files: impl IntoIterator<Item = &'a PathBuf>) -> Result<String> {
    let mut digest = Sha256::new();
    for path in files {
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(fs::read(path).with_context(|| format!("read {}", path.display()))?);
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn source_digest(root: &Path) -> Result<String> {
    digest_paths(root, &source_files(root)?)
}

pub fn compile_digest(root: &Path) -> Result<String> {
    let files = source_files(root)?;
    digest_paths(
        root,
        files.iter().filter(|path| {
            let relative = path
                .strip_prefix(root)
                .expect("source file belongs to root");
            if relative == Path::new("Cargo.toml")
                || relative == Path::new("Cargo.lock")
                || relative == Path::new("rust-toolchain")
                || relative == Path::new("rust-toolchain.toml")
                || relative.starts_with(".cargo")
                || relative.starts_with("native/adapter")
                || relative.starts_with("native/patches")
            {
                return true;
            }
            relative.starts_with("crates")
                && !relative.components().any(|part| {
                    matches!(
                        part.as_os_str().to_str(),
                        Some("tests" | "benches" | "examples")
                    )
                })
                && !matches!(
                    path.extension().and_then(|v| v.to_str()),
                    Some("md" | "rst")
                )
        }),
    )
}

pub fn validation_digest(root: &Path) -> Result<String> {
    let files = source_files(root)?;
    digest_paths(
        root,
        files.iter().filter(|path| {
            let relative = path
                .strip_prefix(root)
                .expect("source file belongs to root");
            relative.starts_with("crates")
                || relative.starts_with("native")
                || relative.starts_with("tests")
                || relative.starts_with("plugin")
                || relative.starts_with("tools")
                || relative.starts_with(".cargo")
                || matches!(
                    relative.to_str(),
                    Some(
                        "Cargo.toml"
                            | "Cargo.lock"
                            | "rust-toolchain"
                            | "rust-toolchain.toml"
                            | "pyproject.toml"
                            | "uv.lock"
                    )
                )
        }),
    )
}

pub fn copy_source(root: &Path, destination: &Path) -> Result<()> {
    ensure!(
        !destination.starts_with(root.join("tools")),
        "source snapshot must be outside tools/"
    );
    for source in source_files(root)? {
        let target = destination.join(source.strip_prefix(root)?);
        fs::create_dir_all(target.parent().context("source file has no parent")?)?;
        fs::copy(source, target)?;
    }
    Ok(())
}

pub fn verify_source(root: &Path, metadata: &BuildMetadata) -> Result<()> {
    ensure!(
        source_digest(root)? == metadata.source_digest,
        "project source changed during build or packaging; run again"
    );
    Ok(())
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
        &path.join(".git/wez-vtabs-native.json"),
        &Ownership {
            path: fs::canonicalize(path)?,
            remote: remote.into(),
            capability: 1,
        },
    )
}

fn verify_ownership(path: &Path, remote: &str) -> Result<bool> {
    let ownership: Option<Ownership> = state::read_json(&path.join(".git/wez-vtabs-native.json"))?;
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

fn resolve_commit(ctx: &Context, repository: &Path, reference: &str) -> Result<String> {
    let revision = ctx.runner.capture(git(ctx, repository).args([
        "rev-parse",
        "--verify",
        "--end-of-options",
        &format!("{reference}^{{commit}}"),
    ]))?;
    let revision = revision.trim();
    ensure!(
        (revision.len() == 40 || revision.len() == 64)
            && revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "Git did not resolve a full commit ID"
    );
    Ok(revision.to_lowercase())
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
        if !verify_ownership(&upstream, &remote)? {
            // The Python tool predates an upstream ownership marker. Only adopt
            // its cache when the recorded prepared worktree proves the relation.
            ensure!(
                ctx.cache.join("prepared.json").is_file()
                    && ctx.cache.join("worktree/.git").is_file(),
                "refusing to rewrite an unowned upstream cache; choose an empty WEZ_VTABS_CACHE"
            );
            verify_worktree(ctx, &upstream, &ctx.cache.join("worktree"))?;
            write_ownership(&upstream, &remote)?;
        }
    }
    let is_commit = explicit.is_some_and(|value| {
        matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
    });
    let revision = if let Some(reference) = explicit {
        if ctx.offline || is_commit {
            match resolve_commit(ctx, &upstream, reference) {
                Ok(revision) => revision,
                Err(error) if ctx.offline => {
                    return Err(error.context("requested revision is unavailable offline"));
                }
                Err(_) => {
                    ctx.runner.run(git(ctx, &upstream).args([
                        "fetch",
                        "--no-tags",
                        "origin",
                        reference,
                    ]))?;
                    resolve_commit(ctx, &upstream, reference)?
                }
            }
        } else {
            ctx.runner.run(git(ctx, &upstream).args([
                "fetch",
                "--no-tags",
                "origin",
                reference,
            ]))?;
            resolve_commit(ctx, &upstream, "FETCH_HEAD")?
        }
    } else {
        if !ctx.offline && !cloned {
            ctx.runner.run(git(ctx, &upstream).args([
                "fetch",
                "--prune",
                "origin",
                "+refs/heads/main:refs/remotes/origin/main",
            ]))?;
        }
        resolve_commit(ctx, &upstream, "refs/remotes/origin/main")?
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
    let _lock = state::Lock::acquire(&ctx.cache.join("build.lock"))?;
    ensure!(
        matches!(revision.len(), 40 | 64) && revision.bytes().all(|v| v.is_ascii_hexdigit()),
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
    // --local only accepts this verified filesystem repository. The explicit
    // file-only allowlist prevents inherited protocol configuration using a
    // network transport; partial-clone lazy fetching remains disabled.
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

pub fn project_source(ctx: &Context) -> Result<ProjectSource> {
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

pub fn refresh_project(ctx: &Context, branch: &str) -> Result<PathBuf> {
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
        [
            "Cargo.toml",
            "crates/vtabs-app/Cargo.toml",
            "tools/Cargo.toml"
        ]
        .iter()
        .all(|name| checkout.join(name).is_file())
            && !patches(&checkout)?.is_empty(),
        "project branch {branch} does not contain a complete native implementation"
    );
    Ok(checkout)
}

fn patches(root: &Path) -> Result<Vec<PathBuf>> {
    let mut patches = fs::read_dir(root.join("native/patches"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    patches.retain(|path| path.is_file() && path.extension().is_some_and(|v| v == "patch"));
    patches.sort();
    ensure!(!patches.is_empty(), "native patches missing");
    Ok(patches)
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

fn sync_directory(source: &Path, destination: &Path, skip_module: bool) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok_and(|v| v.file_type().is_symlink()) {
        fs::remove_file(destination)?;
    }
    fs::create_dir_all(destination)?;
    let mut expected = BTreeSet::new();
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if (skip_module && name == "mod.rs") || IGNORED.iter().any(|v| name == *v) {
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
            sync_directory(&entry.path(), &target, false)?;
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

pub fn stage_adapter(root: &Path, worktree: &Path) -> Result<()> {
    let adapter = root.join("native/adapter");
    ensure!(
        adapter.join("mod.rs").is_file(),
        "native/adapter/mod.rs missing"
    );
    let gui = worktree.join("wezterm-gui");
    copy_changed(&adapter.join("mod.rs"), &gui.join("src/native_vtabs.rs"))?;
    sync_directory(&adapter, &gui.join("src/native_vtabs"), true)?;
    let manifest = gui.join("Cargo.toml");
    let original = fs::read_to_string(&manifest)?;
    let mut document = original
        .parse::<toml_edit::DocumentMut>()
        .context("parse WezTerm GUI manifest")?;
    let dependencies = document
        .get_mut("dependencies")
        .and_then(toml_edit::Item::as_table_mut)
        .context("WezTerm GUI dependency section changed")?;
    for name in ["vtabs-app", "vtabs-store"] {
        let mut dependency = toml_edit::InlineTable::new();
        dependency.insert(
            "path",
            root.join("crates")
                .join(name)
                .to_string_lossy()
                .to_string()
                .into(),
        );
        dependency.insert("default-features", false.into());
        dependencies[name] = toml_edit::value(dependency);
    }
    let mut ratatui = toml_edit::InlineTable::new();
    ratatui.insert("version", "0.30.2".into());
    ratatui.insert("default-features", false.into());
    dependencies["ratatui"] = toml_edit::value(ratatui);
    if !dependencies.contains_key("unicode-width") {
        dependencies["unicode-width"] = toml_edit::value("0.2");
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

pub fn prepare(ctx: &Context, resolved: &ResolvedSource) -> Result<PathBuf> {
    let _stage = ctx.runner.stage("prepare");
    let worktree = ctx.cache.join("worktree");
    let patch_files = patches(&ctx.root)?;
    let patch_digest = digest_paths(&ctx.root, &patch_files)?;
    let adapter_files = source_files(&ctx.root)?
        .into_iter()
        .filter(|v| v.starts_with(ctx.root.join("native/adapter")))
        .collect::<Vec<_>>();
    let adapter_digest = digest_paths(&ctx.root, &adapter_files)?;
    // Legacy preparation records use a single integration digest and require
    // one rebuild. They remain valid evidence for cache ownership above.
    let raw: Option<serde_json::Value> = state::read_json(&ctx.cache.join("prepared.json"))?;
    let previous = raw.and_then(|value| serde_json::from_value::<Prepared>(value).ok());
    let root = fs::canonicalize(&ctx.root)?;
    let mut reuse = false;
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
        let record = ctx.cache.join("prepared.json");
        if record.exists() {
            fs::remove_file(record)?;
        }
        if worktree.exists() {
            // Retain the worktree's submodule object stores across patch and
            // revision changes. Removing the worktree would discard them and
            // unnecessarily make an otherwise cached offline build impossible.
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
        } else { "initialize native upstream submodules" })?;
        for patch in &patch_files {
            ctx.runner
                .run(git(ctx, &worktree).args(["apply", "--check"]).arg(patch))?;
            ctx.runner
                .run(git(ctx, &worktree).arg("apply").arg(patch))?;
        }
    }
    // Always compare contents: preserves mtimes and repairs interrupted staging.
    stage_adapter(&ctx.root, &worktree)?;
    state::write_json(
        &ctx.cache.join("prepared.json"),
        &Prepared {
            version: PREPARATION_VERSION,
            upstream: resolved.revision.clone(),
            patch_digest,
            adapter_digest,
            root,
        },
    )?;
    if ctx.explain {
        eprintln!(
            "prepare: {}",
            if reuse {
                "reuse patches; synchronize changed adapter files"
            } else {
                "created patched worktree"
            }
        );
    }
    Ok(worktree)
}

pub fn patch_check(ctx: &Context) -> Result<serde_json::Value> {
    let resolved = resolve(ctx)?;
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
    let checked = (|| -> Result<()> {
        for patch in patches(&ctx.root)? {
            ctx.runner
                .run(git(ctx, &checkout).args(["apply", "--check"]).arg(&patch))?;
            ctx.runner
                .run(git(ctx, &checkout).arg("apply").arg(&patch))?;
        }
        stage_adapter(&ctx.root, &checkout)?;
        Ok(())
    })();
    let removed = ctx.runner.run(
        git(ctx, &resolved.checkout)
            .args(["worktree", "remove", "--force", "--force"])
            .arg(&checkout),
    );
    checked?;
    removed?;
    Ok(
        serde_json::json!({"upstream":resolved.revision,"patches":patches(&ctx.root)?.len(),"status":"compatible"}),
    )
}

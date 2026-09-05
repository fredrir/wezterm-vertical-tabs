//! Update transactions, including a versioned handoff to freshly fetched tools.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bundle;
use crate::process::CommandSpec;
use crate::state::{self, Context, Lock, ProjectSource};

const DAY: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseManifest {
    schema_version: u32,
    id: String,
    target: String,
    source_digest: String,
    upstream: String,
    project_source: ProjectSource,
    archive: String,
    sha256: String,
    size: u64,
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn download(ctx: &Context, url: &str, path: &Path) -> Result<()> {
    ensure!(!ctx.offline, "offline mode forbids downloading {url}");
    ensure!(
        url.starts_with("https://"),
        "downloads require an HTTPS URL"
    );
    ctx.runner.run(
        CommandSpec::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--retry",
                "2",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--output",
            ])
            .arg(path)
            .arg(url)
            .cwd(&ctx.root)
            .timeout(Duration::from_secs(900)),
    )
}

fn release_manifest(ctx: &Context, location: &str, temporary: &Path) -> Result<ReleaseManifest> {
    let manifest_path = if location.starts_with("https://") {
        let path = temporary.join("release.json");
        download(ctx, location, &path)?;
        path
    } else {
        ensure!(!location.contains("://"), "unsupported manifest URL scheme");
        PathBuf::from(location)
    };
    let manifest: ReleaseManifest =
        state::read_json(&manifest_path)?.context("release manifest missing")?;
    ensure!(
        manifest.schema_version == 1,
        "unsupported release manifest version"
    );
    state::safe_id(&manifest.id)?;
    ensure!(
        manifest.target == env!("WEZ_VTABS_TARGET"),
        "release target mismatch: expected {}",
        env!("WEZ_VTABS_TARGET")
    );
    ensure!(
        is_hex(&manifest.sha256, 64) && is_hex(&manifest.source_digest, 64),
        "invalid release SHA-256 digest"
    );
    ensure!(
        is_hex(&manifest.upstream, 40) || is_hex(&manifest.upstream, 64),
        "release upstream must be an exact Git revision"
    );
    let revision = manifest
        .project_source
        .revision
        .as_deref()
        .context("release project revision missing")?;
    ensure!(
        is_hex(revision, 40) || is_hex(revision, 64),
        "release project source must be an exact Git revision"
    );
    let project = crate::source::project_source(ctx)?;
    ensure!(
        manifest.project_source.remote == project.remote
            && manifest.project_source.branch == project.branch,
        "release project source mismatch"
    );
    if let Some(expected) = &ctx.upstream {
        ensure!(
            &manifest.upstream == expected,
            "release upstream does not match requested revision"
        );
    }
    ensure!(manifest.size > 0, "empty release archive");
    ctx.runner
        .metadata("release", &serde_json::to_value(&manifest)?)?;
    Ok(manifest)
}

fn install_release(
    ctx: &Context,
    location: &str,
    manifest: &ReleaseManifest,
    temporary: &Path,
    stage_only: bool,
) -> Result<PathBuf> {
    let suffix = if manifest.archive.ends_with(".zip") {
        ".zip"
    } else if manifest.archive.ends_with(".tar.gz") {
        ".tar.gz"
    } else {
        anyhow::bail!("release archive must be .zip or .tar.gz")
    };
    let archive = temporary.join(format!("bundle{suffix}"));
    if manifest.archive.starts_with("https://") {
        download(ctx, &manifest.archive, &archive)?;
    } else {
        bundle::safe_relative(Path::new(&manifest.archive))?;
        ensure!(
            Path::new(&manifest.archive).components().count() == 1,
            "release archive must be a sibling filename or HTTPS URL"
        );
        if location.starts_with("https://") {
            let base = location
                .rsplit_once('/')
                .context("invalid release manifest URL")?
                .0;
            download(ctx, &format!("{base}/{}", manifest.archive), &archive)?;
        } else {
            fs::copy(
                Path::new(location)
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(&manifest.archive),
                &archive,
            )?;
        }
    }
    ensure!(
        fs::metadata(&archive)?.len() == manifest.size,
        "release archive size mismatch"
    );
    ensure!(
        bundle::hash_file(&archive)? == manifest.sha256.to_lowercase(),
        "release archive checksum mismatch"
    );
    let bundle = bundle::extract_archive(&archive, &temporary.join("extracted"))?;
    let metadata = bundle::verify(&bundle)?;
    ensure!(
        metadata.id == manifest.id
            && metadata.target == manifest.target
            && metadata.source_digest == manifest.source_digest
            && metadata.upstream == manifest.upstream
            && metadata.project_source.remote == manifest.project_source.remote
            && metadata.project_source.branch == manifest.project_source.branch
            && metadata.project_source.revision == manifest.project_source.revision,
        "release manifest does not match bundle identity"
    );
    crate::install::install(ctx, &bundle, stage_only)
}

pub fn queue_daily_update(bundle: &Path, root: &Path) -> Result<()> {
    if std::env::var("WEZ_VTABS_OFFLINE").as_deref() == Ok("1") {
        return Ok(());
    }
    let previous: Value = state::read_json(&root.join("update.json"))?.unwrap_or_else(|| json!({}));
    if state::now().saturating_sub(previous["last_attempt"].as_u64().unwrap_or(0)) < DAY {
        return Ok(());
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("update.log"))?;
    let mut command = Command::new(bundle::tool_path(bundle));
    command
        .args(["update", "--daily", "--stage-only"])
        .arg("--project-root")
        .arg(bundle.join("source"))
        .env("WEZ_VTABS_INSTALL", root)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    // Reap while the launcher remains alive. An exiting launcher does not
    // terminate the detached worker or release the worker's transaction locks.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn update(
    ctx: &Context,
    daily: bool,
    stage_only: bool,
    check_only: bool,
    output: &Path,
    manifest: Option<&str>,
) -> Result<Value> {
    let _stage = ctx.runner.stage("update");
    let mut state = json!({"last_attempt":state::now(),"status":"checking"});
    let mut delegate = None;
    let synced = std::env::var("WEZ_VTABS_UPDATE_SOURCE_SYNCED").as_deref() == Ok("1");
    {
        let _update_lock = Lock::acquire(&ctx.install.join("update.lock"))?;
        let previous: Value =
            state::read_json(&ctx.install.join("update.json"))?.unwrap_or_else(|| json!({}));
        if daily
            && !synced
            && state::now().saturating_sub(previous["last_attempt"].as_u64().unwrap_or(0)) < DAY
        {
            return Ok(json!({"status":"skipped","reason":"daily interval","previous":previous}));
        }
        state::write_json(&ctx.install.join("update.json"), &state)?;
        let operation = (|| -> Result<()> {
            if let Some(location) = manifest {
                fs::create_dir_all(&ctx.cache)?;
                let temporary = tempfile::Builder::new()
                    .prefix(".release-")
                    .tempdir_in(&ctx.cache)?;
                let release = release_manifest(ctx, location, temporary.path())?;
                let active: Value =
                    state::read_json(&ctx.install.join("active.json"))?.unwrap_or(Value::Null);
                state["available"] = json!(active["id"] != release.id);
                state["id"] = json!(release.id);
                state["release"] = serde_json::to_value(&release)?;
                if check_only {
                    state["status"] = json!("checked");
                } else {
                    let installed =
                        install_release(ctx, location, &release, temporary.path(), stage_only)?;
                    state["status"] = json!("ready");
                    state["bundle"] = json!(installed);
                    state["staged"] = json!(stage_only);
                }
                return Ok(());
            }

            let _build_lock = Lock::acquire(&ctx.cache.join("build.lock"))?;
            let installed_source = ctx
                .root
                .parent()
                .is_some_and(|parent| parent.join("build.json").is_file());
            if installed_source && !synced {
                let project = crate::source::project_source(ctx)?;
                let checkout = crate::source::refresh_project(ctx, &project.branch)?;
                let revision = ctx.runner.capture(
                    CommandSpec::new("git")
                        .args(["rev-parse", "HEAD"])
                        .cwd(&checkout),
                )?;
                ensure!(
                    is_hex(&revision, 40) || is_hex(&revision, 64),
                    "project update revision invalid"
                );
                ctx.runner.metadata(
                    "project_update",
                    &json!({"source":project,"revision":revision,"checkout":checkout}),
                )?;
                if check_only {
                    let resolved = crate::source::resolve(ctx)?;
                    let installed: state::BuildMetadata =
                        state::read_json(&ctx.root.parent().unwrap().join("build.json"))?
                            .context("installed build metadata missing")?;
                    state["status"] = json!("checked");
                    state["available"] = json!(
                        installed.project_source.revision.as_deref() != Some(&revision)
                            || installed.upstream != resolved.revision
                    );
                    state["project_revision"] = json!(revision);
                    state["upstream"] = json!(resolved.revision);
                    return Ok(());
                }
                ensure!(
                    checkout.join("tools/Cargo.toml").is_file(),
                    "updated project has no Rust tooling; choose a compatible native branch"
                );
                // A revision-specific output prevents overwriting a running
                // updater executable on Windows during a later update.
                let target = ctx.cache.join("updaters").join(&revision).join("target");
                let tool = target
                    .join(env!("WEZ_VTABS_TARGET"))
                    .join("release")
                    .join(bundle::executable_name("wez-vtabs"));
                let mut compile = CommandSpec::new("cargo")
                    .args(["build", "--release", "--locked", "--manifest-path"])
                    .arg(checkout.join("tools/Cargo.toml"))
                    .args(["--bin", "wez-vtabs"])
                    .args(["--target", env!("WEZ_VTABS_TARGET")])
                    .cwd(&checkout)
                    .env("CARGO_TARGET_DIR", &target);
                if ctx.offline {
                    compile = compile.arg("--offline");
                }
                if let Some(jobs) = ctx.jobs {
                    compile = compile.args(["--jobs", &jobs.to_string()]);
                }
                ctx.runner.run(compile)?;
                let mut command = CommandSpec::new(&tool)
                    .arg("update")
                    .arg("--output")
                    .arg(output)
                    .arg("--project-root")
                    .arg(&checkout)
                    .cwd(&checkout)
                    .env("WEZ_VTABS_UPDATE_SOURCE_SYNCED", "1")
                    .env("WEZ_VTABS_PROJECT_BRANCH", &project.branch)
                    .env("WEZ_VTABS_INSTALL", &ctx.install)
                    .env("WEZ_VTABS_CACHE", &ctx.cache);
                if stage_only {
                    command = command.arg("--stage-only");
                }
                if ctx.offline {
                    command = command.arg("--offline");
                }
                if let Some(upstream) = &ctx.upstream {
                    command = command.arg("--upstream").arg(upstream);
                }
                if let Some(jobs) = ctx.jobs {
                    command = command.args(["--jobs", &jobs.to_string()]);
                }
                if ctx.json {
                    command = command.arg("--json");
                }
                command = command.arg("--profile").arg(&ctx.profile);
                delegate = Some(command);
                state["status"] = json!("project_synced");
                state["project_revision"] = json!(revision);
            } else if check_only {
                let resolved = crate::source::resolve(ctx)?;
                let source_digest = crate::source::source_digest(&ctx.root)?;
                let active = crate::install::current_bundle(&ctx.install, false).ok();
                let installed: Option<state::BuildMetadata> = match active {
                    Some(path) => state::read_json(&path.join("build.json"))?,
                    None => None,
                };
                state["status"] = json!("checked");
                state["upstream"] = json!(resolved.revision);
                state["source_digest"] = json!(source_digest);
                state["available"] = json!(
                    installed.is_none_or(|metadata| metadata.upstream != resolved.revision
                        || metadata.source_digest != source_digest)
                );
            } else {
                state["status"] = json!("building");
                state::write_json(&ctx.install.join("update.json"), &state)?;
                let metadata = crate::build::build(ctx)?;
                let bundle = bundle::package(ctx, &metadata, output, false)?;
                let installed = crate::install::install(ctx, &bundle, stage_only)?;
                state["status"] = json!("ready");
                state["id"] = json!(metadata.id);
                state["bundle"] = json!(installed);
                state["staged"] = json!(stage_only);
            }
            Ok(())
        })();
        if let Err(error) = operation {
            state["status"] = json!("failed");
            state["error"] = json!(format!("{error:#}"));
            state["reproduction"] = json!(ctx.runner.directory());
            state::write_json(&ctx.install.join("update.json"), &state)?;
            return Err(error);
        }
        state::write_json(&ctx.install.join("update.json"), &state)?;
    }
    if let Some(command) = delegate {
        // The new tool takes its own update/build locks; no parent lock survives
        // the handoff and developer checkouts are never reset by this path.
        if let Err(error) = ctx.runner.run(command) {
            state = json!({"last_attempt":state::now(),"status":"failed","error":format!("{error:#}"),"reproduction":ctx.runner.directory()});
            state::write_json(&ctx.install.join("update.json"), &state)?;
            return Err(error);
        }
        state = state::read_json(&ctx.install.join("update.json"))?
            .context("delegated update state missing")?;
    }
    Ok(state)
}

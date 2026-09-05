use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::{Value, json};

use crate::process::{CommandSpec, RunReport, relevant_environment};
use crate::source;
use crate::state::{self, BuildMetadata, Context};

pub const BINARIES: &[&str] = &[
    "wezterm-gui",
    "wezterm",
    "wezterm-mux-server",
    "strip-ansi-escapes",
    "wez-vtabs-store",
];

fn cargo(ctx: &Context, cwd: &Path, target_dir: &Path, command: &str) -> CommandSpec {
    let mut spec = CommandSpec::new("cargo")
        .arg(command)
        .cwd(cwd)
        .env("CARGO_TARGET_DIR", target_dir)
        .args([
            "--profile",
            if ctx.profile == "debug" {
                "dev"
            } else {
                &ctx.profile
            },
        ]);
    if ctx.offline {
        spec = spec.arg("--offline").env("CARGO_NET_OFFLINE", "true");
    }
    if let Some(jobs) = ctx.jobs {
        spec = spec.args(["--jobs", &jobs.to_string()]);
    }
    if ctx
        .runner
        .report()
        .invocation
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--timings")
    {
        spec = spec.arg("--timings");
    }
    spec
}

fn preserve_cargo_timings(ctx: &Context, target_dir: &Path) -> Result<()> {
    let report = ctx.runner.report();
    if !report
        .invocation
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--timings")
    {
        return Ok(());
    }
    let source = target_dir.join("cargo-timings");
    if source.is_dir() {
        let destination = ctx.runner.directory().join("cargo-timings");
        fs::create_dir_all(&destination)?;
        for entry in fs::read_dir(&source)? {
            let entry = entry?;
            let modified = entry
                .metadata()?
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let name = entry.file_name();
            if entry.file_type()?.is_file()
                && name.to_string_lossy().starts_with("cargo-timing")
                && entry.path().extension().is_some_and(|v| v == "html")
                && modified >= report.started_at
            {
                let target = destination.join(&name);
                if !target.exists() || name == "cargo-timing.html" {
                    fs::copy(entry.path(), target)?;
                }
            }
        }
        ctx.runner
            .metadata("cargo_timings", &json!("cargo-timings"))?;
    }
    Ok(())
}

fn configuration_files(ctx: &Context, worktree: &Path) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    for root in [&ctx.root, worktree] {
        for directory in root.ancestors() {
            // Cargo prefers legacy config when both spellings exist.
            let old = directory.join(".cargo/config");
            let new = directory.join(".cargo/config.toml");
            if old.is_file() {
                files.insert(old);
            } else if new.is_file() {
                files.insert(new);
            }
        }
    }
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|value| PathBuf::from(value).join(".cargo"))
        });
    if let Some(home) = home {
        let old = home.join("config");
        let new = home.join("config.toml");
        if old.is_file() {
            files.insert(old);
        } else if new.is_file() {
            files.insert(new);
        }
    }
    files.into_iter().collect()
}

fn config_records(ctx: &Context, worktree: &Path) -> Result<Value> {
    configuration_files(ctx, worktree)
        .iter()
        .map(|path| {
            let contents = fs::read_to_string(path)?;
            let original = contents.parse::<toml_edit::DocumentMut>()?;
            let mut settings = toml_edit::DocumentMut::new();
            // Preserve reproducible compiler settings without credentials,
            // registry tokens, HTTP headers or credential-provider configuration.
            for key in ["build", "profile", "target"] {
                if let Some(value) = original.get(key) {
                    settings[key] = value.clone();
                }
            }
            Ok(
                json!({"path":path,"sha256":state::hash_bytes(contents.as_bytes()),
                "settings_toml":settings.to_string()}),
            )
        })
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

fn requested_target(ctx: &Context, worktree: &Path) -> Result<Option<String>> {
    if let Ok(target) = std::env::var("CARGO_BUILD_TARGET") {
        ensure!(!target.is_empty(), "CARGO_BUILD_TARGET must not be empty");
        return Ok(Some(target));
    }
    // Read the effective native workspace target. Passing it explicitly keeps
    // the project store helper on the same target as the application.
    for directory in worktree.ancestors().chain(ctx.root.ancestors()) {
        let old = directory.join(".cargo/config");
        let new = directory.join(".cargo/config.toml");
        let path = if old.is_file() { old } else { new };
        if path.is_file() {
            let document = fs::read_to_string(&path)?.parse::<toml_edit::DocumentMut>()?;
            if let Some(target) = document.get("build").and_then(|v| v.get("target")) {
                return target
                    .as_str()
                    .map(|v| Some(v.to_owned()))
                    .context("native builds require one Cargo build.target triple");
            }
        }
    }
    // CARGO_HOME is lower precedence than the workspace and its ancestors.
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|value| PathBuf::from(value).join(".cargo"))
        });
    if let Some(home) = home {
        let path = if home.join("config").is_file() {
            home.join("config")
        } else {
            home.join("config.toml")
        };
        if path.is_file() {
            let document = fs::read_to_string(path)?.parse::<toml_edit::DocumentMut>()?;
            if let Some(target) = document.get("build").and_then(|v| v.get("target")) {
                return target
                    .as_str()
                    .map(|v| Some(v.to_owned()))
                    .context("native builds require one Cargo build.target triple");
            }
        }
    }
    Ok(None)
}

fn restore_replay(ctx: &Context, worktree: &Path, revision: &str) -> Result<Option<RunReport>> {
    let Some(path) = std::env::var_os("WEZ_VTABS_REPRO_REPORT") else {
        return Ok(None);
    };
    let report: RunReport =
        state::read_json(Path::new(&path))?.context("reproduction report missing")?;
    ensure!(
        report.version == 1,
        "unsupported reproduction report version"
    );
    let recorded = report
        .metadata
        .get("upstream")
        .and_then(|v| v.as_str().or_else(|| v["revision"].as_str()))
        .context("reproduction report has no resolved upstream revision")?;
    ensure!(
        recorded == revision,
        "reproduction upstream revision differs from the recorded failure"
    );
    if let Some(locks) = report.metadata.get("resolved_locks") {
        for (name, root) in [("project", ctx.root.as_path()), ("upstream", worktree)] {
            if let Some(contents) = locks[name].as_str() {
                let path = root.join("Cargo.lock");
                ensure!(
                    !fs::symlink_metadata(&path).is_ok_and(|v| v.file_type().is_symlink()),
                    "refusing to restore a symlinked Cargo.lock"
                );
                fs::write(path, contents)?;
            }
        }
    }
    ctx.runner.metadata(
        "reproduction",
        &json!({"report":PathBuf::from(path),"upstream":revision,"locked":true}),
    )?;
    Ok(Some(report))
}

fn verify_replay(report: &RunReport, configuration: &Value) -> Result<()> {
    let Some(recorded) = report.metadata.get("build_configuration") else {
        return Ok(());
    };
    for key in [
        "rustc",
        "cargo",
        "project_rustc",
        "project_cargo",
        "target",
        "profile",
        "environment",
    ] {
        if let Some(expected) = recorded.get(key) {
            ensure!(
                configuration.get(key) == Some(expected),
                "reproduction {key} differs from the recorded failure; use the recorded toolchain and configuration"
            );
        }
    }
    if let Some(expected) = recorded.get("cargo_configs") {
        let digests = |value: &Value| {
            let mut values = value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v["sha256"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            values.sort();
            values
        };
        ensure!(
            digests(expected) == digests(&configuration["cargo_configs"]),
            "reproduction Cargo configuration differs from the recorded failure"
        );
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn capture_locks(ctx: &Context, worktree: &Path) -> Result<Value> {
    let locks = json!({
        "project":read_optional(&ctx.root.join("Cargo.lock"))?,
        "upstream":read_optional(&worktree.join("Cargo.lock"))?,
    });
    ctx.runner.metadata("resolved_locks", &locks)?;
    Ok(json!({
        "project":locks["project"].as_str().map(|v| state::hash_bytes(v.as_bytes())),
        "upstream":locks["upstream"].as_str().map(|v| state::hash_bytes(v.as_bytes())),
    }))
}

fn collect_artifacts(output: &str, artifacts: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    for line in output.lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(executable) = message.get("executable").and_then(Value::as_str) else {
            continue;
        };
        let Some(name) = message["target"]["name"].as_str() else {
            continue;
        };
        if BINARIES.contains(&name) {
            let path = PathBuf::from(executable);
            ensure!(
                path.is_file(),
                "Cargo reported a missing executable: {}",
                path.display()
            );
            artifacts.insert(name.into(), fs::canonicalize(path)?);
        }
    }
    Ok(())
}

fn worktree_digest(ctx: &Context, worktree: &Path) -> Result<String> {
    let diff = ctx.runner.capture(source::git(ctx, worktree).args([
        "diff",
        "--binary",
        "--no-ext-diff",
        "--no-textconv",
        "HEAD",
        "--",
        ".",
    ]))?;
    ctx.runner.metadata("upstream_diff", &json!(diff))?;
    // The injected adapter is untracked and already included in compile_digest.
    // Include other non-ignored additions because a build script may read them.
    let untracked = ctx.runner.capture(source::git(ctx, worktree).args([
        "ls-files",
        "--others",
        "--exclude-standard",
        "-z",
    ]))?;
    let mut additions = BTreeMap::new();
    for relative in untracked.split('\0').filter(|v| !v.is_empty()) {
        let path = worktree.join(relative);
        if path.is_file() {
            additions.insert(relative, state::hash_bytes(&fs::read(path)?));
        }
    }
    let submodules = ctx.runner.capture(source::git(ctx, worktree).args([
        "submodule",
        "status",
        "--recursive",
    ]))?;
    ctx.runner.metadata("submodules", &json!(submodules))?;
    Ok(state::hash_bytes(&serde_json::to_vec(
        &json!({"diff":diff,"untracked":additions,"submodules":submodules}),
    )?))
}

fn native_validation(
    ctx: &Context,
    worktree: &Path,
    target_dir: &Path,
    target: Option<&str>,
) -> Result<()> {
    let _stage = ctx.runner.stage("validate");
    for (package, library) in [
        ("wezterm-gui", false),
        ("wezterm-client", true),
        ("wezterm-input-types", true),
    ] {
        let mut command =
            cargo(ctx, worktree, target_dir, "test").args(["--locked", "-p", package]);
        if let Some(target) = target {
            command = command.args(["--target", target]);
        }
        if library {
            command = command.arg("--lib");
        }
        let result = ctx.runner.run(command.arg("native_"));
        preserve_cargo_timings(ctx, target_dir)?;
        result?;
    }
    Ok(())
}

fn snapshot(ctx: &Context, digest: &str) -> Result<()> {
    let _stage = ctx.runner.stage("snapshot");
    let first = ctx.runner.directory().join("source");
    let destination = if first.exists() && source::source_digest(&first)? != digest {
        ctx.runner.directory().join(format!("source-{digest}"))
    } else {
        first
    };
    if !destination.exists() {
        source::copy_source(&ctx.root, &destination)?;
    }
    ctx.runner.metadata(
        "source_snapshot",
        &json!(
            destination
                .file_name()
                .context("snapshot basename missing")?
        ),
    )?;
    ensure!(
        source::source_digest(&destination)? == digest,
        "project source changed while capturing reproduction snapshot; run again"
    );
    Ok(())
}

fn bundle_id(metadata: &BuildMetadata) -> Result<String> {
    let project_id = state::hash_bytes(&serde_json::to_vec(&metadata.project_source)?);
    ensure!(
        metadata.upstream.len() >= 12
            && metadata.source_digest.len() >= 12
            && metadata.compile_digest.len() >= 12,
        "incomplete build identity"
    );
    let identifier = format!(
        "{}-{}-{}-{}-{}-{}",
        &metadata.upstream[..12],
        &metadata.source_digest[..12],
        &metadata.compile_digest[..12],
        &project_id[..8],
        metadata.target,
        metadata.profile
    );
    state::safe_id(&identifier)?;
    Ok(identifier)
}

/// Restage resource changes without compilation. The previous validation
/// identity is retained so a later build can validate changed test inputs.
pub fn restage(ctx: &Context, previous: &BuildMetadata) -> Result<Option<BuildMetadata>> {
    let _stage = ctx.runner.stage("restage");
    let source_digest = source::source_digest(&ctx.root)?;
    snapshot(ctx, &source_digest)?;
    let profile = if ctx.profile == "debug" {
        "dev"
    } else {
        &ctx.profile
    };
    if previous.profile != profile
        || ctx
            .upstream
            .as_ref()
            .is_some_and(|v| v != &previous.upstream)
        || previous.configuration["compile_inputs"].as_str()
            != Some(&source::compile_digest(&ctx.root)?)
        || previous.configuration["environment"] != serde_json::to_value(relevant_environment())?
    {
        return Ok(None);
    }
    let worktree = ctx.cache.join("worktree");
    if !worktree.is_dir()
        || previous.configuration["cargo_configs"] != config_records(ctx, &worktree)?
        || previous.configuration["worktree_digest"].as_str()
            != Some(&worktree_digest(ctx, &worktree)?)
    {
        return Ok(None);
    }
    for (key, program, argument, cwd) in [
        (
            "rustc",
            std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()),
            "-vV",
            &worktree,
        ),
        ("cargo", "cargo".into(), "-V", &worktree),
        (
            "project_rustc",
            std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()),
            "-vV",
            &ctx.root,
        ),
        ("project_cargo", "cargo".into(), "-V", &ctx.root),
    ] {
        if previous.configuration[key].as_str()
            != Some(
                &ctx.runner
                    .capture(CommandSpec::new(program).arg(argument).cwd(cwd))?,
            )
        {
            return Ok(None);
        }
    }
    for name in BINARIES {
        let Some(path) = previous.artifacts.get(*name).filter(|path| path.is_file()) else {
            return Ok(None);
        };
        if previous.configuration["artifact_sha256"][*name].as_str()
            != Some(&crate::bundle::hash_file(path)?)
        {
            return Ok(None);
        }
    }
    let mut metadata = previous.clone();
    metadata.source_digest = source_digest;
    metadata.project_source = source::project_source(ctx)?;
    metadata.id = bundle_id(&metadata)?;
    source::verify_source(&ctx.root, &metadata)?;
    state::write_json(&ctx.cache.join("build.json"), &metadata)?;
    ctx.runner
        .metadata("build", &serde_json::to_value(&metadata)?)?;
    Ok(Some(metadata))
}

pub fn build(ctx: &Context) -> Result<BuildMetadata> {
    ensure!(
        !ctx.profile.is_empty()
            && ctx
                .profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "invalid Cargo profile"
    );
    let fingerprint_stage = ctx.runner.stage("fingerprint");
    let source_digest = source::source_digest(&ctx.root)?;
    snapshot(ctx, &source_digest)?;
    let compile_inputs = source::compile_digest(&ctx.root)?;
    let validation_inputs = source::validation_digest(&ctx.root)?;
    drop(fingerprint_stage);
    let project = source::project_source(ctx)?;
    ctx.runner
        .metadata("source_digest", &json!(source_digest))?;
    let resolved = source::resolve(ctx)?;
    let worktree = source::prepare(ctx, &resolved)?;
    let replay = restore_replay(ctx, &worktree, &resolved.revision)?;
    let target_dir = resolved.checkout.join("target");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let toolchain = ctx
        .runner
        .capture(CommandSpec::new(&rustc).arg("-vV").cwd(&worktree))?;
    let cargo_version = ctx
        .runner
        .capture(CommandSpec::new("cargo").arg("-V").cwd(&worktree))?;
    let project_toolchain = ctx
        .runner
        .capture(CommandSpec::new(&rustc).arg("-vV").cwd(&ctx.root))?;
    let project_cargo = ctx
        .runner
        .capture(CommandSpec::new("cargo").arg("-V").cwd(&ctx.root))?;
    let host = toolchain
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .context("rustc host target missing")?;
    let requested = requested_target(ctx, &worktree)?;
    let target = requested.as_deref().unwrap_or(host).to_string();
    ensure!(
        target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "native bundle target must be a Rust target triple"
    );
    let profile = if ctx.profile == "debug" {
        "dev"
    } else {
        &ctx.profile
    };
    let mut configuration = json!({
        "version":2,"rustc":toolchain,"cargo":cargo_version,"target":target,"profile":profile,
        "project_rustc":project_toolchain,"project_cargo":project_cargo,
        "compile_inputs":compile_inputs,"validation_inputs":validation_inputs,
        "environment":relevant_environment(),"cargo_configs":config_records(ctx,&worktree)?,
        "project_root":ctx.root,"target_directory":target_dir,"offline":ctx.offline,
        "locks":capture_locks(ctx,&worktree)?,
    });
    ctx.runner.metadata("build_configuration", &configuration)?;
    if let Some(report) = &replay {
        verify_replay(report, &configuration)?;
    }
    let previous: Option<BuildMetadata> = state::read_json(&ctx.cache.join("build.json"))?;
    let mut artifacts = BTreeMap::new();

    // Cargo is the authority for freshness, including build-script dependencies
    // and toolchain/config changes that a project-only fingerprint cannot see.
    let compile_stage = ctx.runner.stage("compile");
    let mut helper = cargo(ctx, &ctx.root, &target_dir, "build")
        .args([
            "--locked",
            "--message-format=json-render-diagnostics",
            "--manifest-path",
        ])
        .arg(ctx.root.join("Cargo.toml"))
        .args(["-p", "vtabs-store", "--features", "sqlite"]);
    if let Some(target) = &requested {
        helper = helper.args(["--target", target]);
    }
    let helper_output = ctx.runner.capture(helper);
    preserve_cargo_timings(ctx, &target_dir)?;
    collect_artifacts(&helper_output?, &mut artifacts)?;

    let mut application =
        cargo(ctx, &worktree, &target_dir, "build").arg("--message-format=json-render-diagnostics");
    if replay.is_some() {
        application = application.arg("--locked");
    }
    for package in &BINARIES[..4] {
        application = application.args(["-p", package]);
    }
    if let Some(target) = &requested {
        application = application.args(["--target", target]);
    }
    // A newly patched workspace must resolve the injected path dependencies.
    // Preserve the actual resulting lock even when compilation fails.
    let output = ctx.runner.capture(application);
    preserve_cargo_timings(ctx, &target_dir)?;
    configuration["locks"] = capture_locks(ctx, &worktree)?;
    ctx.runner.metadata("build_configuration", &configuration)?;
    collect_artifacts(&output?, &mut artifacts)?;
    for name in BINARIES {
        ensure!(
            artifacts.contains_key(*name),
            "Cargo did not produce required binary: {name}"
        );
    }
    // Bind immutable bundles and cached validation to the actual outputs. Cargo
    // build scripts may consume system inputs beyond our configuration list.
    let artifact_hashes = artifacts
        .iter()
        .map(|(name, path)| crate::bundle::hash_file(path).map(|digest| (name.clone(), digest)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    configuration["artifact_sha256"] = serde_json::to_value(artifact_hashes)?;

    #[cfg(windows)]
    {
        let binaries = artifacts["wezterm-gui"]
            .parent()
            .context("native executable parent missing")?;
        crate::bundle::copy_windows_runtime(
            &worktree,
            &[binaries.to_path_buf(), binaries.join("deps")],
        )?;
    }

    configuration["worktree_digest"] = json!(worktree_digest(ctx, &worktree)?);
    drop(compile_stage);
    // Network availability changes resolution policy, not compiled output.
    let mut identity_configuration = configuration.clone();
    identity_configuration
        .as_object_mut()
        .context("build configuration is not an object")?
        .remove("offline");
    identity_configuration
        .as_object_mut()
        .unwrap()
        .remove("validation_inputs");
    let compile_digest = state::hash_bytes(&serde_json::to_vec(&json!({
        "source":compile_inputs,"upstream":resolved.revision,"configuration":identity_configuration,
    }))?);
    let validation_digest = state::hash_bytes(&serde_json::to_vec(&json!({
        "compile":compile_digest,"validation":validation_inputs,
    }))?);
    if previous
        .as_ref()
        .is_some_and(|v| v.validation_digest == validation_digest)
    {
        if ctx.explain {
            eprintln!(
                "native tests: reuse successful validation for unchanged compilation and test inputs"
            );
        }
    } else {
        native_validation(ctx, &worktree, &target_dir, requested.as_deref())?;
    }
    let mut metadata = BuildMetadata {
        id: String::new(),
        capability: state::CAPABILITY,
        upstream: resolved.revision,
        source_digest,
        compile_digest,
        validation_digest,
        target,
        profile: profile.into(),
        project_source: project,
        built_at: state::now(),
        configuration,
        artifacts,
    };
    metadata.id = bundle_id(&metadata)?;
    source::verify_source(&ctx.root, &metadata)?;
    let latest_project = source::project_source(ctx)?;
    if serde_json::to_value(latest_project)? != serde_json::to_value(&metadata.project_source)? {
        bail!("project revision changed during build; run again");
    }
    ctx.runner
        .metadata("build", &serde_json::to_value(&metadata)?)?;
    state::write_json(&ctx.cache.join("build.json"), &metadata)?;
    Ok(metadata)
}

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::process::{CommandSpec, RunReport};
use crate::state::{BuildMetadata, Context, Lock, read_json};
use crate::{install, source};

pub fn plan(ctx: &Context, operation: &str) -> Result<Value> {
    let previous = read_json::<BuildMetadata>(&ctx.cache.join("build.json"))?;
    let inputs = json!({"source":source::source_digest(&ctx.root)?, "compile":source::compile_digest(&ctx.root)?, "validation":source::validation_digest(&ctx.root)?});
    let unchanged = previous
        .as_ref()
        .is_some_and(|old| inputs["source"].as_str() == Some(&old.source_digest));
    let compile_changed = previous
        .as_ref()
        .is_none_or(|old| old.configuration["compile_inputs"] != inputs["compile"]);
    let validation_changed = previous
        .as_ref()
        .is_none_or(|old| old.configuration["validation_inputs"] != inputs["validation"]);
    let upstream = ctx.upstream.clone().or_else(|| {
        if ctx.offline || operation == "dev" {
            previous.as_ref().map(|old| old.upstream.clone())
        } else {
            None
        }
    });
    let mut steps = vec![
        json!({"name":"resolve","action":if upstream.is_some(){"check"}else{"run"},"reason":if upstream.is_some(){"use selected upstream revision"}else{"resolve latest main once; plan does not fetch"}}),
    ];
    if operation == "check" {
        steps.clear();
        steps.push(json!({"name":"validate","action":"run","reason":"run project format, lint, schema and behavioral checks"}));
    } else {
        steps.push(json!({"name":"prepare","action":"check","reason":"compare upstream, ordered patches and adapter inputs separately"}));
        if operation != "prepare" {
            steps.push(json!({"name":"compile","action":"check","reason":if compile_changed{"Rust/native inputs changed or no previous build; Cargo checks required compilation"}else{"Rust/native inputs unchanged; Cargo checks toolchain and dependency freshness"}}));
            steps.push(json!({"name":"validate","action":if validation_changed{"run"}else{"check"},"reason":if validation_changed{"validation inputs changed or no previous build"}else{"reuse only if compiled artifact hashes also match"}}));
        }
        if operation == "package" || operation == "dev" {
            steps.push(json!({"name":"bundle","action":if unchanged {"check"}else{"run"},"reason":if unchanged {"verify existing bundle"}else{"source bundle inputs changed or no previous build"}}));
            if operation == "package" {
                steps.push(json!({"name":"archive","action":"check","reason":"verify or create compressed distribution"}));
            }
        }
    }
    Ok(
        json!({"operation":operation,"upstream":upstream,"offline":ctx.offline,"profile":ctx.profile,"inputs":inputs,"previous":previous,"steps":steps}),
    )
}

pub fn doctor(ctx: &Context, operation: &str) -> Result<Value> {
    ensure!(
        [
            "build", "prepare", "check", "test", "package", "install", "launch", "update"
        ]
        .contains(&operation),
        "unknown doctor operation"
    );
    let required: &[&str] = match operation {
        "launch" | "install" => &[],
        "prepare" => &["git"],
        "check" | "test" => &["cargo", "rustc", "uv"],
        _ => &["git", "cargo", "rustc"],
    };
    let mut checks = Vec::new();
    for program in required {
        match ctx.runner.capture(
            CommandSpec::new(program)
                .arg("--version")
                .cwd(&ctx.root)
                .timeout(Duration::from_secs(10)),
        ) {
            Ok(version) => checks.push(json!({"name":program,"ok":true,"version":version})),
            Err(error) => {
                checks.push(json!({"name":program,"ok":false,"error":format!("{error:#}")}))
            }
        }
    }
    if matches!(operation, "build" | "package" | "update") {
        let mut programs = vec![if cfg!(windows) { "cl.exe" } else { "cc" }];
        if matches!(operation, "package" | "update") && !cfg!(windows) {
            programs.push("tic");
        }
        if matches!(operation, "package" | "update") && cfg!(target_os = "macos") {
            programs.push("codesign");
        }
        for program in programs {
            let path = std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join(program))
                    .find(|path| path.is_file())
            });
            checks.push(json!({"name":program,"ok":path.is_some(),"path":path}));
        }
    }
    if operation == "check" {
        for component in ["rustfmt", "clippy"] {
            let result = ctx.runner.capture(
                CommandSpec::new("cargo")
                    .args([
                        if component == "rustfmt" {
                            "fmt"
                        } else {
                            "clippy"
                        },
                        "--version",
                    ])
                    .cwd(&ctx.root)
                    .timeout(Duration::from_secs(10)),
            );
            checks.push(match result {
                Ok(version) => json!({"name":component,"ok":true,"version":version}),
                Err(error) => json!({"name":component,"ok":false,"error":error.to_string()}),
            });
        }
    }
    let build = read_json::<BuildMetadata>(&ctx.cache.join("build.json"));
    let build = match build {
        Ok(value) => serde_json::to_value(value)?,
        Err(error) => {
            checks.push(json!({"name":"build metadata","ok":false,"error":error.to_string()}));
            Value::Null
        }
    };
    let status = match install::status(&ctx.install) {
        Ok(value) => value,
        Err(error) => {
            checks.push(json!({"name":"install state","ok":false,"error":error.to_string()}));
            Value::Null
        }
    };
    if operation == "launch" {
        let result = install::current_bundle(&ctx.install, false)
            .and_then(|path| crate::bundle::verify(&path));
        checks.push(match result {
            Ok(_) => json!({"name":"active bundle","ok":true}),
            Err(error) => json!({"name":"active bundle","ok":false,"error":error.to_string()}),
        });
    }
    Ok(
        json!({"ok":checks.iter().all(|check|check["ok"] == true),"operation":operation,"target":env!("WEZ_VTABS_TARGET"),"cache":ctx.cache,"install":ctx.install,"checks":checks,"build":build,"status":status}),
    )
}

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

pub fn cache(ctx: &Context, gc: bool, dry_run: bool, keep: usize) -> Result<Value> {
    let _lock = if gc {
        Some(Lock::acquire(&ctx.cache.join("build.lock"))?)
    } else {
        None
    };
    let mut protected = BTreeSet::new();
    for name in ["active.json", "pending.json"] {
        if let Some(value) = read_json::<Value>(&ctx.install.join(name))?
            && let Some(id) = value["id"].as_str()
        {
            protected.insert(format!("wez-vtabs-native-{id}"));
        }
    }
    let mut entries = Vec::new();
    for category in ["runs", "bundles"] {
        let directory = ctx.cache.join(category);
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
            if child.file_type()?.is_symlink() || !child.file_type()?.is_dir() {
                continue;
            }
            let marker = if category == "runs" {
                "run.json"
            } else {
                "build.json"
            };
            if !path.join(marker).is_file() {
                continue;
            }
            let name = child.file_name().to_string_lossy().into_owned();
            let active = path == ctx.runner.directory()
                || protected.contains(&name)
                || (category == "runs"
                    && read_json::<RunReport>(&path.join(marker))?
                        .is_some_and(|report| report.status == "running"));
            let lease = Lock::try_acquire(&ctx.cache.join("leases").join(format!("{name}.lock")))?;
            let install_lease = if category == "bundles" {
                if let Some(metadata) = read_json::<BuildMetadata>(&path.join("build.json"))? {
                    crate::state::safe_id(&metadata.id)?;
                    Some(Lock::try_acquire(
                        &ctx.install
                            .join("leases")
                            .join(format!("{}.lock", metadata.id)),
                    )?)
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
            entries.push(json!({"path":path,"bytes":bytes,"protected":protected,"action":if remove {if dry_run {"would_remove"}else{"removed"}}else{"keep"}}));
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
            }
        }
    }
    Ok(json!({"cache":ctx.cache,"dry_run":dry_run,"entries":entries}))
}

pub fn reproduce(ctx: &Context, path: &Path, execute: bool, explicit_root: bool) -> Result<Value> {
    let report_path = if path.is_dir() {
        path.join("run.json")
    } else {
        path.to_path_buf()
    };
    let report: RunReport = read_json(&report_path)?.context("run report missing")?;
    ensure!(report.version == 1, "unsupported run report version");
    let parent = report_path.parent().context("report parent missing")?;
    let snapshot_name = report
        .metadata
        .get("source_snapshot")
        .and_then(Value::as_str)
        .and_then(|value| value.rsplit(['/', '\\']).next())
        .map(std::ffi::OsStr::new)
        .unwrap_or_else(|| std::ffi::OsStr::new("source"));
    ensure!(
        snapshot_name.to_string_lossy().starts_with("source"),
        "invalid reproduction snapshot"
    );
    let snapshot = parent.join(snapshot_name);
    let root = if explicit_root {
        ctx.root.clone()
    } else if snapshot.join("Cargo.toml").is_file() {
        snapshot
    } else {
        ctx.root.clone()
    };
    let mut args = Vec::new();
    let mut skip = false;
    let mut forwarded = false;
    for arg in &report.invocation {
        if arg == "--" {
            forwarded = true;
            args.push(arg.clone());
            continue;
        }
        if forwarded {
            args.push(arg.clone());
            continue;
        }
        if skip {
            skip = false;
            continue;
        }
        if [
            "--project-root",
            "--cache",
            "--install-root",
            "--upstream",
            "--output",
        ]
        .contains(&arg.as_str())
        {
            skip = true;
            continue;
        }
        if [
            "--project-root=",
            "--cache=",
            "--install-root=",
            "--upstream=",
            "--output=",
        ]
        .iter()
        .any(|prefix| arg.starts_with(prefix))
        {
            continue;
        }
        args.push(arg.clone());
    }
    let parsed = <crate::cli::Cli as clap::Parser>::try_parse_from(
        std::iter::once("wez-vtabs".to_owned()).chain(args.clone()),
    )?;
    if execute {
        ensure!(
            matches!(
                parsed.command,
                crate::cli::Commands::Build
                    | crate::cli::Commands::Prepare
                    | crate::cli::Commands::Deps
                    | crate::cli::Commands::Check
                    | crate::cli::Commands::Test { .. }
                    | crate::cli::Commands::Package { bundle: None, .. }
                    | crate::cli::Commands::Generate { .. }
                    | crate::cli::Commands::Patch { .. }
            ),
            "report operation cannot be replayed; inspect recorded commands"
        );
    }
    let upstream = report
        .metadata
        .get("upstream")
        .and_then(|value| value.as_str().or_else(|| value["revision"].as_str()))
        .or_else(|| {
            report
                .metadata
                .get("build")
                .and_then(|value| value["upstream"].as_str())
        });
    if let Some(revision) = upstream {
        args.splice(0..0, ["--upstream".into(), revision.into()]);
    }
    let replay = ctx.cache.join("reproductions").join(format!(
        "{}-{}",
        crate::state::now(),
        std::process::id()
    ));
    let result = json!({"report":report_path,"status":report.status,"source":root,"upstream":upstream,"invocation":args,"configuration":report.configuration,"commands":report.commands,"execute":execute,"replay_root":replay});
    if execute {
        ensure!(
            root.join("Cargo.toml").is_file(),
            "reproduction source missing; provide --project-root"
        );
        let source = replay.join("source");
        source::copy_source(&root, &source)?;
        if parsed.offline
            && matches!(
                parsed.command,
                crate::cli::Commands::Build
                    | crate::cli::Commands::Prepare
                    | crate::cli::Commands::Package { .. }
                    | crate::cli::Commands::Patch { .. }
                    | crate::cli::Commands::Test {
                        suite: crate::cli::Suite::Native
                            | crate::cli::Suite::Ssh
                            | crate::cli::Suite::Tls,
                        ..
                    }
            )
        {
            source::seed_replay_cache(
                ctx,
                &replay.join("cache"),
                upstream.context("offline reproduction revision missing")?,
                report
                    .configuration
                    .get("WEZ_VTABS_UPSTREAM_URL")
                    .map(String::as_str)
                    .unwrap_or(source::UPSTREAM_URL),
            )?;
        }
        if let Some(project) = report.metadata.get("project_source") {
            crate::state::write_json(
                &replay.join("build.json"),
                &json!({"capability":1,"project_source":project}),
            )?;
        }
        let base = CommandSpec::new(std::env::current_exe()?)
            .arg("--project-root")
            .arg(&source)
            .arg("--cache")
            .arg(replay.join("cache"))
            .arg("--install-root")
            .arg(replay.join("install"))
            .env("WEZ_VTABS_REPRO_REPORT", report_path.canonicalize()?)
            .cwd(&source);
        let mut base = base;
        for (key, value) in &report.configuration {
            base = base.env(key, value);
        }
        if matches!(
            parsed.command,
            crate::cli::Commands::Test {
                suite: crate::cli::Suite::Native | crate::cli::Suite::Ssh | crate::cli::Suite::Tls,
                ..
            }
        ) {
            let metadata: BuildMetadata = serde_json::from_value(
                report
                    .metadata
                    .get("build")
                    .context("native reproduction build metadata missing")?
                    .clone(),
            )?;
            let build = base.clone().args([
                "--upstream",
                &metadata.upstream,
                "--profile",
                &metadata.profile,
                "build",
            ]);
            ctx.runner.run(build)?;
            let built: BuildMetadata = read_json(&replay.join("cache/build.json"))?
                .context("reproduction build missing")?;
            let binaries = built
                .artifacts
                .get("wezterm-gui")
                .and_then(|path| path.parent())
                .context("reproduction GUI artifact missing")?;
            let mut remapped = Vec::new();
            let mut skip = false;
            for arg in args {
                if skip {
                    skip = false;
                    continue;
                }
                if arg == "--native-bin-dir" || arg == "--basetemp" {
                    skip = true;
                    continue;
                }
                if arg.starts_with("--native-bin-dir=") || arg.starts_with("--basetemp=") {
                    continue;
                }
                remapped.push(arg);
            }
            if !remapped.iter().any(|arg| arg == "--") {
                remapped.push("--".into());
            }
            remapped.push(format!("--native-bin-dir={}", binaries.display()));
            remapped.push(format!(
                "--basetemp={}",
                replay.join("test-artifacts").display()
            ));
            args = remapped;
        }
        ctx.runner.run(base.args(&args))?;
    }
    Ok(result)
}

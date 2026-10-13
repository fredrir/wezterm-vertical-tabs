use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::cli::{Cli, Commands, Suite};
use crate::process::{CommandSpec, RunReport};
use crate::source::{inventory, upstream};
use crate::state::{BuildMetadata, Context, read_json};

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
    let parsed = <Cli as clap::Parser>::try_parse_from(
        std::iter::once("wez-vtabs".to_owned()).chain(args.clone()),
    )?;
    if execute {
        ensure!(
            matches!(
                parsed.command,
                Commands::Build(..)
                    | Commands::Prepare
                    | Commands::Deps
                    | Commands::Check { .. }
                    | Commands::Test { .. }
                    | Commands::Package { bundle: None, .. }
                    | Commands::Patch { .. }
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
        inventory::copy_source(&root, &source)?;
        if parsed.offline
            && matches!(
                parsed.command,
                Commands::Build(..)
                    | Commands::Prepare
                    | Commands::Package { .. }
                    | Commands::Patch { .. }
                    | Commands::Test {
                        suite: Suite::Gui | Suite::Ssh | Suite::Tls,
                        ..
                    }
            )
        {
            upstream::seed_replay_cache(
                ctx,
                &replay.join("cache"),
                upstream.context("offline reproduction revision missing")?,
                report
                    .configuration
                    .get("WEZ_VTABS_UPSTREAM_URL")
                    .map(String::as_str)
                    .unwrap_or(upstream::UPSTREAM_URL),
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
            Commands::Test {
                suite: Suite::Gui | Suite::Ssh | Suite::Tls,
                ..
            }
        ) {
            let metadata: BuildMetadata = serde_json::from_value(
                report
                    .metadata
                    .get("build")
                    .context("reproduction build metadata missing")?
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
            let built = BuildMetadata::read(&replay.join("cache"))?
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
                if arg == "--wezterm-bin-dir" || arg == "--basetemp" {
                    skip = true;
                    continue;
                }
                if arg.starts_with("--wezterm-bin-dir=") || arg.starts_with("--basetemp=") {
                    continue;
                }
                remapped.push(arg);
            }
            if !remapped.iter().any(|arg| arg == "--") {
                remapped.push("--".into());
            }
            remapped.push(format!("--wezterm-bin-dir={}", binaries.display()));
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

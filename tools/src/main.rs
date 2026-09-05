mod build;
mod bundle;
mod check;
mod cli;
mod diagnostics;
mod install;
mod process;
mod source;
mod state;
mod update;
mod watch;

use anyhow::{Context as _, Result};
use clap::Parser;
use cli::{Cli, Commands};
use serde_json::{Value, json};
use state::{Context, Lock};
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn absolute(path: PathBuf) -> Result<PathBuf> {
    let path = if path == Path::new("~") {
        home()
    } else if let Ok(relative) = path.strip_prefix("~/") {
        home().join(relative)
    } else {
        path
    };
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(if path.exists() {
        path.canonicalize()?
    } else {
        path
    })
}

fn context(cli: &Cli) -> Result<Context> {
    let managed = install::managed_context(&std::env::current_exe()?)?;
    let cwd = std::env::current_dir()?;
    let root = cli
        .project_root
        .clone()
        .or_else(|| managed.as_ref().map(|(source, _)| source.clone()))
        .or_else(|| {
            cwd.ancestors()
                .find(|path| {
                    path.join("tools/Cargo.toml").is_file() && path.join("crates").is_dir()
                })
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| cwd.clone());
    let root = absolute(root)?;
    let cache = absolute(cli.cache.clone().unwrap_or_else(|| {
        std::env::var_os(if cfg!(windows) {
            "LOCALAPPDATA"
        } else {
            "XDG_CACHE_HOME"
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
        .join("wez-vtabs-native")
    }))?;
    let install = absolute(
        cli.install_root
            .clone()
            .or_else(|| managed.map(|(_, install)| install))
            .unwrap_or_else(|| {
                std::env::var_os(if cfg!(windows) {
                    "LOCALAPPDATA"
                } else {
                    "XDG_DATA_HOME"
                })
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".local/share"))
                .join("wez-vtabs-native")
            }),
    )?;
    let runner = process::Runner::new(
        &cache,
        &root,
        std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
    )?;
    let development = matches!(&cli.command, Commands::Dev { .. })
        || matches!(&cli.command,Commands::Plan{operation} if operation == "dev");
    let profile = cli.profile.clone().unwrap_or_else(|| {
        if cli.debug {
            "dev"
        } else if development {
            "iterate"
        } else {
            "release"
        }
        .into()
    });
    let ctx = Context {
        root,
        cache,
        install,
        runner,
        offline: cli.offline,
        upstream: cli.upstream.clone(),
        profile,
        jobs: cli.jobs.map(usize::from),
        json: cli.json,
        explain: cli.explain,
    };
    ctx.runner.metadata("context",&json!({"root":ctx.root,"cache":ctx.cache,"install":ctx.install,"profile":ctx.profile,"offline":ctx.offline,"jobs":ctx.jobs,"target":env!("WEZ_VTABS_TARGET")}))?;
    Ok(ctx)
}

fn dispatch(ctx: &Context, cli: &Cli) -> Result<(Value, i32)> {
    let value = match &cli.command {
        Commands::Prepare => {
            let _lock = Lock::acquire(&ctx.cache.join("build.lock"))?;
            let resolved = source::resolve(ctx)?;
            let path = source::prepare(ctx, &resolved)?;
            json!({"upstream":resolved.revision,"worktree":path})
        }
        Commands::Build => {
            let _lock = Lock::acquire(&ctx.cache.join("build.lock"))?;
            serde_json::to_value(build::build(ctx)?)?
        }
        Commands::Deps => {
            let _lock = Lock::acquire(&ctx.cache.join("build.lock"))?;
            let resolved = source::resolve(ctx)?;
            let worktree = source::prepare(ctx, &resolved)?;
            anyhow::ensure!(
                !ctx.offline,
                "system dependency installation requires online mode"
            );
            ctx.runner.run(
                process::CommandSpec::new(worktree.join("get-deps"))
                    .cwd(&worktree)
                    .env("CI", "yes"),
            )?;
            json!({"upstream":resolved.revision,"status":"dependencies installed"})
        }
        Commands::Dev {
            watch,
            debounce_ms,
            args,
        } => {
            return watch::dev(ctx, *watch, *debounce_ms, args)
                .map(|status| (json!({"status":status}), status));
        }
        Commands::Check => {
            check::check(ctx)?;
            json!({"status":"passed"})
        }
        Commands::Test { suite, args } => {
            check::test(ctx, *suite, args)?;
            json!({"status":"passed"})
        }
        Commands::Generate { check } => {
            check::generate(ctx, *check)?;
            json!({"status":if *check {"current"}else{"generated"}})
        }
        Commands::Package {
            output,
            bundle: existing,
        } => {
            if let Some(existing) = existing {
                bundle::verify(existing)?;
                json!({"archive":bundle::archive_bundle(existing)?})
            } else {
                let _lock = Lock::acquire(&ctx.cache.join("build.lock"))?;
                let metadata = build::build(ctx)?;
                json!({"bundle":bundle::package(ctx,&metadata,&output.clone().unwrap_or_else(||ctx.root.join("dist")),true)?,"build":metadata})
            }
        }
        Commands::Install {
            bundle: existing,
            stage_only,
        } => {
            // Hold the cache lock through copying the freshly packaged runtime
            // into its immutable installed version so GC cannot remove it.
            let _lock = if existing.is_none() {
                Some(Lock::acquire(&ctx.cache.join("build.lock"))?)
            } else {
                None
            };
            let path = if let Some(path) = existing {
                path.clone()
            } else {
                let metadata = build::build(ctx)?;
                bundle::package(ctx, &metadata, &ctx.cache.join("bundles"), false)?
            };
            json!({"installed":install::install(ctx,&path,*stage_only)?,"staged":stage_only})
        }
        Commands::Update {
            daily,
            stage_only,
            check_only,
            output,
            manifest,
        } => update::update(
            ctx,
            *daily,
            *stage_only,
            *check_only,
            &output.clone().unwrap_or_else(|| ctx.cache.join("bundles")),
            manifest.as_deref(),
        )?,
        Commands::Launch { args } => {
            let status = install::launch(ctx, args)?;
            return Ok((json!({"status":status}), status));
        }
        Commands::Doctor { operation } => {
            let value = diagnostics::doctor(ctx, operation)?;
            let status = if value["ok"] == true { 0 } else { 1 };
            return Ok((value, status));
        }
        Commands::Plan { operation } => diagnostics::plan(ctx, operation)?,
        Commands::Status => install::status(&ctx.install)?,
        Commands::Versions => install::versions(&ctx.install)?,
        Commands::Rollback { id } => {
            json!({"active":install::rollback(&ctx.install,id.as_deref())?})
        }
        Commands::Cache { command } => match command {
            cli::CacheCommand::Inspect => diagnostics::cache(ctx, false, true, 5)?,
            cli::CacheCommand::Gc { dry_run, keep } => {
                diagnostics::cache(ctx, true, *dry_run, *keep)?
            }
        },
        Commands::Patch {
            command: cli::PatchCommand::Check,
        } => {
            let _lock = Lock::acquire(&ctx.cache.join("build.lock"))?;
            source::patch_check(ctx)?
        }
        Commands::Repro { report, execute } => {
            diagnostics::reproduce(ctx, report, *execute, cli.project_root.is_some())?
        }
        Commands::Manifest { bundle: path } => bundle::write_manifest(path)?,
        Commands::Verify { bundle: path } => serde_json::to_value(bundle::verify(path)?)?,
    };
    Ok((value, 0))
}

fn execute() -> Result<i32> {
    let cli = Cli::parse();
    let ctx = context(&cli)?;
    let cancellation = ctx.runner.clone();
    ctrlc::set_handler(move || cancellation.cancel()).context("install cancellation handler")?;
    let result = (|| -> Result<(Value, i32)> {
        if matches!(
            cli.command,
            Commands::Prepare
                | Commands::Deps
                | Commands::Patch { .. }
                | Commands::Check
                | Commands::Test { .. }
                | Commands::Generate { .. }
        ) {
            let project = source::project_source(&ctx)?;
            ctx.runner
                .metadata("project_source", &serde_json::to_value(project)?)?;
            source::copy_source(&ctx.root, &ctx.runner.directory().join("source"))?;
            ctx.runner.metadata("source_snapshot", &json!("source"))?;
        }
        if cli.explain
            && matches!(
                cli.command,
                Commands::Build
                    | Commands::Package { .. }
                    | Commands::Dev { .. }
                    | Commands::Prepare
            )
        {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&diagnostics::plan(&ctx, "build")?)?
            );
        }
        dispatch(&ctx, &cli)
    })();
    match result {
        Ok((value, status)) => {
            ctx.runner.finish(if status == 0 {
                None
            } else {
                Some(format!("exit: {status}"))
            })?;
            if !matches!(cli.command, Commands::Launch { .. } | Commands::Dev { .. }) || cli.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            if cli.timings {
                eprintln!("{}", serde_json::to_string_pretty(&ctx.runner.timings())?);
            }
            Ok(status)
        }
        Err(error) => {
            if ctx.root.join("Cargo.toml").is_file()
                && !ctx.runner.directory().join("source").is_dir()
            {
                match source::copy_source(&ctx.root, &ctx.runner.directory().join("source")) {
                    Ok(()) => {
                        let _ = ctx.runner.metadata("source_snapshot", &json!("source"));
                    }
                    Err(snapshot) => {
                        let _ = ctx
                            .runner
                            .metadata("snapshot_error", &json!(snapshot.to_string()));
                    }
                }
            }
            let _ = ctx.runner.finish(Some(format!("{error:#}")));
            let report = ctx.runner.directory().join("run.json");
            if cli.json {
                println!("{}", json!({"error":format!("{error:#}"),"report":report}));
            }
            eprintln!("wez-vtabs: {error:#}\nreport: {}", report.display());
            if cli.timings {
                eprintln!("{}", ctx.runner.timings());
            }
            Ok(if ctx.runner.cancelled() { 130 } else { 1 })
        }
    }
}

fn main() {
    let status = match execute() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("wez-vtabs: {error:#}");
            1
        }
    };
    std::process::exit(status);
}

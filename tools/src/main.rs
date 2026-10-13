mod build;
mod build_identity;
mod bundle;
mod cache;
mod check;
mod cli;
mod container;
mod deploy;
mod dev;
mod install;
mod plan;
mod process;
mod remote;
mod repro;
mod setup;
mod sign;
mod source;
mod state;
mod targets;
mod update;

use anyhow::{Context as _, Result};
use clap::{CommandFactory, Parser, ValueEnum};
use cli::{Cli, Commands};
use serde_json::{Value, json};
use source::{inventory, patches, project, upstream};
use state::Context;
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn data_home() -> PathBuf {
    std::env::var_os(if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "XDG_DATA_HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_else(|| home().join(".local/share"))
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
                .find(|path| path.join("tools/Cargo.toml").is_file() && path.join("src").is_dir())
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
        .join("wez-vtabs")
    }))?;
    let install = absolute(
        cli.install_root
            .clone()
            .or_else(|| managed.map(|(_, install)| install))
            .unwrap_or_else(|| data_home().join("wez-vtabs")),
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
        || matches!(&cli.command, Commands::Plan { operation } if operation == "dev");
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
        role: state::Role::default(),
        triple: None,
    };
    ctx.runner.metadata("context",&json!({"root":ctx.root,"cache":ctx.cache,"install":ctx.install,"profile":ctx.profile,"offline":ctx.offline,"jobs":ctx.jobs,"target":env!("WEZ_VTABS_TARGET")}))?;
    Ok(ctx)
}

fn request(ctx: &Context, cli: &Cli) -> Result<remote::Request> {
    let mut globals = Vec::new();
    for (enabled, flag) in [
        (cli.offline, "--offline"),
        (cli.debug, "--debug"),
        (cli.explain, "--explain"),
        (cli.timings, "--timings"),
        (cli.json, "--json"),
    ] {
        if enabled {
            globals.push(flag.to_owned());
        }
    }
    if let Some(profile) = &cli.profile {
        globals.extend(["--profile".into(), profile.clone()]);
    }
    if let Some(jobs) = cli.jobs {
        globals.extend(["--jobs".into(), jobs.to_string()]);
    }
    Ok(remote::Request {
        globals,
        upstream: remote::pinned_upstream(ctx)?,
    })
}

/// Runs `command` on the `--on` machine, or returns `None` to run it here.
fn elsewhere(
    ctx: &Context,
    cli: &Cli,
    on: &cli::On,
    mut command: Vec<String>,
) -> Result<Option<(Value, i32)>> {
    let Some(host) = targets::plan(on.on.as_deref(), None)?.on else {
        return Ok(None);
    };
    let tail = command
        .iter()
        .position(|value| value == "--")
        .unwrap_or(command.len());
    command.splice(tail..tail, ["--on".to_owned(), targets::LOCAL.to_owned()]);
    remote::delegate(ctx, &host, &request(ctx, cli)?, &command).map(Some)
}

fn dispatch(ctx: &Context, cli: &Cli) -> Result<(Value, i32)> {
    let value = match &cli.command {
        Commands::Prepare => {
            let _lock = ctx.lock_build()?;
            let resolved = upstream::resolve(ctx)?;
            let path = patches::prepare(ctx, &resolved)?;
            json!({"upstream":resolved.revision,"worktree":path})
        }
        Commands::Build(args) => {
            let plan = targets::plan(args.on.on.as_deref(), args.to.to.as_deref())?;
            if args.platform.is_none() && !plan.is_local() {
                return remote::build(ctx, &plan, &request(ctx, cli)?);
            }
            let ctx = &Context {
                role: args.role.or(plan.role()).unwrap_or_default(),
                ..ctx.clone()
            };
            if let Some(platform) = &args.platform {
                container::run(ctx, args, platform)?
            } else {
                let _lock = ctx.lock_build()?;
                serde_json::to_value(build::build(ctx)?)?
            }
        }
        Commands::Deps => setup::deps(ctx)?,
        Commands::Dev {
            watch,
            no_watch,
            debounce_ms,
            args,
        } => {
            return dev::dev(ctx, *watch && !*no_watch, *debounce_ms, args)
                .map(|status| (json!({"status":status}), status));
        }
        Commands::Check { args } => {
            check::check(ctx, args)?;
            json!({"status":"passed"})
        }
        Commands::Lint { fix, on } => {
            let mut command = vec!["lint".to_owned()];
            if *fix {
                command.push("--fix".into());
            }
            if let Some(result) = elsewhere(ctx, cli, on, command)? {
                return Ok(result);
            }
            check::lint(ctx, *fix)?;
            json!({"status":"passed"})
        }
        Commands::Test { suite, on, args } => {
            let name = suite
                .to_possible_value()
                .context("suite name")?
                .get_name()
                .to_owned();
            let mut command = vec!["test".to_owned(), name];
            if !args.is_empty() {
                command.push("--".into());
                command.extend(args.iter().cloned());
            }
            if let Some(result) = elsewhere(ctx, cli, on, command)? {
                return Ok(result);
            }
            check::test(ctx, *suite, args)?;
            json!({"status":"passed"})
        }
        Commands::Setup { check, on } => {
            let mut command = vec!["setup".to_owned()];
            if *check {
                command.push("--check".into());
            }
            if let Some(result) = elsewhere(ctx, cli, on, command)? {
                return Ok(result);
            }
            if *check {
                return setup::check(ctx);
            }
            setup::install(ctx)?
        }
        Commands::Package {
            output,
            bundle: existing,
            role,
            no_archive,
        } => {
            if let Some(existing) = existing {
                bundle::verify(existing)?;
                json!({"archive":bundle::archive_bundle(existing)?})
            } else {
                let ctx = &Context {
                    role: role.unwrap_or_default(),
                    ..ctx.clone()
                };
                let _lock = ctx.lock_build()?;
                let metadata = build::build(ctx)?;
                let output = output.clone().unwrap_or_else(|| ctx.root.join("dist"));
                json!({"bundle":bundle::package(ctx,&metadata,&output,!no_archive)?,"build":metadata})
            }
        }
        Commands::Install {
            bundle: existing,
            stage_only,
        } => {
            // Held until the packaged runtime is installed so GC cannot remove it first.
            let _lock = existing.is_none().then(|| ctx.lock_build()).transpose()?;
            let path = if let Some(path) = existing {
                path.clone()
            } else {
                let metadata = build::build(ctx)?;
                bundle::package(ctx, &metadata, &ctx.cache.join("bundles"), false)?
            };
            let installed = install::install(ctx, &path, *stage_only)?;
            json!({"installed":installed,"staged":stage_only,"pruned":cache::prune(ctx,_lock.as_ref())?})
        }
        Commands::Prebuild { triple, output } => {
            let ctx = &Context {
                triple: Some(triple.clone()),
                ..ctx.clone()
            };
            let _lock = ctx.lock_build()?;
            let metadata = build::build(ctx)?;
            json!({"prebuilt":build::prebuilt(ctx, &metadata, output)?})
        }
        Commands::Deploy {
            on,
            to,
            rollback,
            role,
            prebuilt,
            bundle: existing,
            app,
            bin,
            no_app,
        } => {
            let plan = targets::plan(on.on.as_deref(), to.to.as_deref())?;
            let build_elsewhere =
                existing.is_none() && prebuilt.is_none() && rollback.is_none() && plan.on.is_some();
            if !plan.to_is_local() || build_elsewhere {
                return remote::deploy(ctx, &plan, &request(ctx, cli)?, rollback.as_ref());
            }
            let role = role.or(plan.role()).unwrap_or_default();
            let ctx = &Context {
                role,
                ..ctx.clone()
            };
            let source = match (existing, rollback, prebuilt) {
                (Some(path), _, _) => deploy::Source::Bundle(path),
                (None, Some(id), _) => deploy::Source::Rollback(id.as_deref()),
                (None, None, Some(path)) => deploy::Source::Prebuilt(path),
                (None, None, None) => deploy::Source::Build,
            };
            let _lock = matches!(source, deploy::Source::Build | deploy::Source::Prebuilt(_))
                .then(|| ctx.lock_build())
                .transpose()?;
            let targets = deploy::Targets::resolve(app.clone(), bin.clone(), *no_app);
            let value = deploy::deploy(ctx, source, &targets, _lock.as_ref())?;
            if let Some(revision) = value["upstream"]["revision"].as_str() {
                remote::record_upstream(ctx, revision)?;
            }
            value
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
        Commands::Plan { operation } => plan::plan(ctx, operation)?,
        Commands::Status { to } => {
            let plan = targets::plan(Some(targets::LOCAL), to.to.as_deref())?;
            match plan.to {
                Some(target) if !target.local => {
                    return remote::installed(ctx, &target, &["status"]);
                }
                _ => install::status(&ctx.install)?,
            }
        }
        Commands::Cache { command } => match command {
            cli::CacheCommand::Inspect => {
                cache::cache(ctx, false, true, cache::Retention::uniform(5))?
            }
            cli::CacheCommand::Gc { dry_run, keep } => {
                cache::cache(ctx, true, *dry_run, cache::Retention::uniform(*keep))?
            }
        },
        Commands::Patch { command } => {
            let _lock = ctx.lock_build()?;
            match command {
                cli::PatchCommand::Check => patches::check(ctx)?,
                cli::PatchCommand::Export => patches::export(ctx)?,
            }
        }
        Commands::Repro { report, execute } => {
            repro::reproduce(ctx, report, *execute, cli.project_root.is_some())?
        }
        Commands::Verify { bundle: path } => serde_json::to_value(bundle::verify(path)?)?,
    };
    Ok((value, 0))
}

fn execute() -> Result<i32> {
    clap_complete::CompleteEnv::with_factory(Cli::command)
        .var("WEZ_VTABS_COMPLETE")
        .complete();
    let cli = Cli::parse();
    let ctx = context(&cli)?;
    let cancellation = ctx.runner.clone();
    ctrlc::set_handler(move || cancellation.cancel()).context("install cancellation handler")?;
    let result = (|| -> Result<(Value, i32)> {
        if cli.command.snapshots_source() {
            project::source(&ctx)?;
            inventory::copy_source(&ctx.root, &ctx.runner.directory().join("source"))?;
            ctx.runner.metadata("source_snapshot", &json!("source"))?;
        }
        if cli.explain
            && let Some(operation) = cli.command.explained_operation()
        {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&plan::plan(&ctx, operation)?)?
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
            if !value.is_null()
                && (!matches!(cli.command, Commands::Launch { .. } | Commands::Dev { .. })
                    || cli.json)
            {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            if cli.timings {
                eprintln!("{}", serde_json::to_string_pretty(&ctx.runner.timings())?);
            }
            Ok(status)
        }
        Err(error) => {
            if let Some(unreachable) = error.downcast_ref::<remote::Unreachable>() {
                let _ = ctx.runner.finish(Some(unreachable.to_string()));
                eprintln!("Error: {unreachable}");
                return Ok(1);
            }
            if ctx.root.join("Cargo.toml").is_file()
                && !ctx.runner.directory().join("source").is_dir()
            {
                match inventory::copy_source(&ctx.root, &ctx.runner.directory().join("source")) {
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

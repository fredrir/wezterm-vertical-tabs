use crate::{
    cli::Suite,
    process::CommandSpec,
    state::{Context, Lock},
};
use anyhow::{Result, ensure};
use std::path::PathBuf;

fn cargo(ctx: &Context) -> CommandSpec {
    CommandSpec::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .cwd(&ctx.root)
        .env("CARGO_TARGET_DIR", ctx.root.join("target/pytest"))
}

fn locked(ctx: &Context, command: CommandSpec) -> CommandSpec {
    let command = command.arg("--locked");
    let command = if ctx.offline {
        command.arg("--offline")
    } else {
        command
    };
    if let Some(jobs) = ctx.jobs {
        command.args(["--jobs", &jobs.to_string()])
    } else {
        command
    }
}

fn uv(ctx: &Context) -> CommandSpec {
    let command = CommandSpec::new("uv").cwd(&ctx.root);
    if ctx.offline {
        command.arg("--offline")
    } else {
        command
    }
}

fn helpers(ctx: &Context) -> Result<PathBuf> {
    let target = ctx.root.join("target/pytest");
    let _lock = Lock::acquire(&target.join("build.lock"))?;
    ctx.runner.run(locked(
        ctx,
        cargo(ctx).args([
            "build",
            "-p",
            "vtabs-core",
            "--bin",
            "gen-schema",
            "-p",
            "vtabs-store",
            "--bin",
            "wez-vtabs-store",
            "--features",
            "vtabs-store/sqlite",
        ]),
    ))?;
    Ok(target.join("debug"))
}

fn schema(ctx: &Context, directory: &std::path::Path, check: bool) -> Result<()> {
    let binary = directory.join(if cfg!(windows) {
        "gen-schema.exe"
    } else {
        "gen-schema"
    });
    for (format, path) in [
        ("lua", "plugin/schema.lua"),
        ("types", "plugin/types.lua"),
        ("markdown", "docs/options.md"),
    ] {
        ctx.runner.run(
            CommandSpec::new(&binary)
                .args([if check { "--check" } else { "--write" }, format, path])
                .cwd(&ctx.root),
        )?;
    }
    Ok(())
}

pub fn generate(ctx: &Context, check: bool) -> Result<()> {
    schema(ctx, &helpers(ctx)?, check)
}

pub fn check(ctx: &Context) -> Result<()> {
    let _stage = ctx.runner.stage("check");
    // Only independent format/lint processes run together; Cargo builds stay coordinated.
    let results = std::thread::scope(|scope| {
        let lint = scope.spawn(|| -> Result<()> {
            ctx.runner.run(uv(ctx).args(["lock", "--check"]))?;
            ctx.runner
                .run(uv(ctx).args(["run", "--locked", "ruff", "check", "scripts", "tests"]))?;
            ctx.runner.run(uv(ctx).args([
                "run", "--locked", "ruff", "format", "--check", "scripts", "tests",
            ]))
        });
        let fmt = scope.spawn(|| ctx.runner.run(cargo(ctx).args(["fmt", "--all", "--check"])));
        (lint.join(), fmt.join())
    });
    results
        .0
        .map_err(|_| anyhow::anyhow!("lint worker failed"))??;
    results
        .1
        .map_err(|_| anyhow::anyhow!("format worker failed"))??;
    ctx.runner.run(locked(
        ctx,
        cargo(ctx).args(["test", "--workspace", "--all-features"]),
    ))?;
    ctx.runner.run(
        locked(
            ctx,
            cargo(ctx).args(["clippy", "--workspace", "--all-targets", "--all-features"]),
        )
        .args(["--", "-D", "warnings"]),
    )?;
    let directory = helpers(ctx)?;
    schema(ctx, &directory, true)?;
    pytest(ctx, Suite::All, &[], Some(directory))
}

fn pytest(ctx: &Context, suite: Suite, args: &[String], binaries: Option<PathBuf>) -> Result<()> {
    let mut command = uv(ctx).args([
        "run",
        "--locked",
        "pytest",
        "-n",
        &ctx.jobs.unwrap_or(2).to_string(),
    ]);
    command = match suite {
        Suite::All => command.arg("tests"),
        Suite::Tools => command.arg("tests/tools"),
        Suite::Rust => command.arg("tests/rust"),
        Suite::Lua => command.arg("tests/integration/test_lua.py"),
        Suite::Native => command.args(["tests/integration", "--run-native"]),
        Suite::Ssh => command.args([
            "tests/integration/test_container_ssh.py",
            "--run-container",
            "--run-native",
        ]),
        Suite::Tls => command.args([
            "tests/native/test_tls.py",
            "tests/integration/test_native.py",
            "--run-native",
            "-k",
            "tls",
        ]),
    };
    if let Some(directory) = binaries {
        command = command.arg("--rust-bin-dir").arg(directory);
    }
    command = command.arg("--tools-bin").arg(std::env::current_exe()?);
    ctx.runner.run(command.args(args))
}

pub fn test(ctx: &Context, suite: Suite, args: &[String]) -> Result<()> {
    let _stage = ctx.runner.stage("test");
    let mut args = args.to_vec();
    if matches!(suite, Suite::Rust) {
        ctx.runner.run(locked(
            ctx,
            cargo(ctx).args(["test", "--workspace", "--all-features"]),
        ))?;
    }
    if matches!(suite, Suite::Native | Suite::Ssh | Suite::Tls) {
        if !args.iter().any(|arg| arg.starts_with("--native-bin-dir"))
            && let Some(metadata) = crate::state::read_json::<crate::state::BuildMetadata>(
                &ctx.cache.join("build.json"),
            )?
            && let Some(directory) = metadata
                .artifacts
                .get("wezterm-gui")
                .and_then(|path| path.parent())
        {
            args.push(format!("--native-bin-dir={}", directory.display()));
        }
        ensure!(
            args.iter().any(|arg| arg.starts_with("--native-bin-dir")),
            "native suite requires -- --native-bin-dir=PATH"
        );
        let requested = args
            .iter()
            .find_map(|arg| arg.strip_prefix("--native-bin-dir="))
            .map(PathBuf::from)
            .or_else(|| {
                args.windows(2)
                    .find(|args| args[0] == "--native-bin-dir")
                    .map(|args| PathBuf::from(&args[1]))
            });
        if let Some(metadata) =
            crate::state::read_json::<crate::state::BuildMetadata>(&ctx.cache.join("build.json"))?
            && requested.as_ref().and_then(|path| path.canonicalize().ok())
                == metadata
                    .artifacts
                    .get("wezterm-gui")
                    .and_then(|path| path.parent())
                    .and_then(|path| path.canonicalize().ok())
        {
            ctx.runner.metadata(
                "upstream",
                &serde_json::json!({"revision":metadata.upstream}),
            )?;
            ctx.runner
                .metadata("build_configuration", &metadata.configuration)?;
            ctx.runner.metadata(
                "resolved_locks",
                &serde_json::json!({
                    "project":std::fs::read_to_string(ctx.root.join("Cargo.lock")).ok(),
                    "upstream":std::fs::read_to_string(ctx.cache.join("worktree/Cargo.lock")).ok()
                }),
            )?;
            ctx.runner
                .metadata("build", &serde_json::to_value(metadata)?)?;
        }
    }
    pytest(ctx, suite, &args, None)
}

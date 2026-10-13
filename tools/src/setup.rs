//! System and Python dependencies for building and testing.

use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::process::CommandSpec;
use crate::source::{patches, upstream};
use crate::state::Context;

pub fn install(ctx: &Context) -> Result<Value> {
    ensure!(!ctx.offline, "setup requires online mode");
    let resolved = {
        let _lock = ctx.lock_build()?;
        let resolved = upstream::resolve(ctx)?;
        patches::prepare(ctx, &resolved)?;
        resolved
    };
    let worktree = ctx.cache.join("worktree");
    eprintln!("+ {} --testing", worktree.join("get-deps").display());
    // Foreground and on the terminal so sudo can prompt.
    let status = std::process::Command::new(worktree.join("get-deps"))
        .arg("--testing")
        .current_dir(&worktree)
        .env("CI", "yes")
        .status()
        .context("start: get-deps")?;
    ensure!(status.success(), "get-deps failed: {status}");
    ctx.runner.run(
        CommandSpec::new("uv")
            .args(["sync", "--locked"])
            .cwd(&ctx.root),
    )?;
    Ok(json!({"upstream":resolved.revision,"status":"ready"}))
}

pub fn deps(ctx: &Context) -> Result<Value> {
    ensure!(
        !ctx.offline,
        "system dependency installation requires online mode"
    );
    let _lock = ctx.lock_build()?;
    let resolved = upstream::resolve(ctx)?;
    let worktree = patches::prepare(ctx, &resolved)?;
    ctx.runner.run(
        CommandSpec::new(worktree.join("get-deps"))
            .cwd(&worktree)
            .env("CI", "yes"),
    )?;
    Ok(json!({"upstream":resolved.revision,"status":"dependencies installed"}))
}

fn version(ctx: &Context, name: &str, command: CommandSpec) -> Value {
    match ctx
        .runner
        .capture(command.cwd(&ctx.root).timeout(Duration::from_secs(10)))
    {
        Ok(version) => json!({"name":name,"ok":true,"version":version}),
        Err(error) => json!({"name":name,"ok":false,"error":format!("{error:#}")}),
    }
}

/// Diagnoses build and test prerequisites without changing anything.
pub fn check(ctx: &Context) -> Result<(Value, i32)> {
    let mut checks = Vec::new();
    for program in ["git", "cargo", "rustc", "uv"] {
        checks.push(version(
            ctx,
            program,
            CommandSpec::new(program).arg("--version"),
        ));
    }
    for (component, subcommand) in [("rustfmt", "fmt"), ("clippy", "clippy")] {
        checks.push(version(
            ctx,
            component,
            CommandSpec::new("cargo").args([subcommand, "--version"]),
        ));
    }
    let compiler = if cfg!(windows) { "cl.exe" } else { "cc" };
    let path = which::which(compiler).ok();
    checks.push(json!({"name":compiler,"ok":path.is_some(),"path":path}));
    if let Err(error) = ctx.last_build() {
        checks.push(json!({"name":"build metadata","ok":false,"error":error.to_string()}));
    }
    if let Err(error) = crate::install::status(&ctx.install) {
        checks.push(json!({"name":"install state","ok":false,"error":error.to_string()}));
    }
    let ok = checks.iter().all(|check| check["ok"] == true);
    Ok((json!({"ok":ok,"checks":checks}), if ok { 0 } else { 1 }))
}

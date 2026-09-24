//! Targets on other machines: source sync, delegated commands and bundle transfer over SSH.

use std::fmt;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::process::CommandSpec;
use crate::state::{self, BuildMetadata, Context};
use crate::targets::{LOCAL, Plan, Platform, Target};
use crate::{build, bundle, container, deploy, source};

const PATH: &str =
    r#"export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH""#;
const INCOMING: &str = ".cache/wez-vtabs/incoming/bundle";
const INSTALLED_TOOL: &str = r#""$HOME/.local/bin/wez-vtabs""#;
const MISSING: &str = "this machine is not in targets.toml; name it with --to";

#[derive(Debug)]
pub struct Unreachable(pub String);

impl fmt::Display for Unreachable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unreachable ({})", self.0)
    }
}

impl std::error::Error for Unreachable {}

/// Flags every delegated invocation repeats; `upstream` pins all machines to one WezTerm revision.
#[derive(Default)]
pub struct Request {
    pub globals: Vec<String>,
    pub upstream: Option<String>,
}

impl Request {
    fn arguments<'a>(
        &self,
        pinned: bool,
        command: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let mut arguments = self.globals.clone();
        if pinned && let Some(upstream) = &self.upstream {
            arguments.extend(["--upstream".into(), upstream.clone()]);
        }
        arguments.extend(command.into_iter().map(str::to_owned));
        arguments
    }
}

pub struct Host(String);

impl Host {
    pub fn connect(name: &str) -> Result<Self> {
        let reachable = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                name,
                "true",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !reachable {
            return Err(Unreachable(name.into()).into());
        }
        Ok(Self(name.into()))
    }

    /// Inherits the terminal so prompts, colors and Ctrl-C reach the remote command.
    fn run(&self, ctx: &Context, script: &str) -> Result<i32> {
        let mut command = Command::new("ssh");
        if std::io::stdin().is_terminal() {
            command.arg("-t");
        }
        eprintln!("+ ssh {} {script}", self.0);
        ctx.runner
            .metadata("remote", &json!({"host": self.0, "script": script}))?;
        let status = command
            .args([
                "-o",
                "LogLevel=error",
                &self.0,
                &format!("{PATH}; {script}"),
            ])
            .status()
            .context("start: ssh")?;
        Ok(status.code().unwrap_or(1))
    }

    fn capture(&self, ctx: &Context, script: &str) -> Result<String> {
        ctx.runner.capture(
            CommandSpec::new("ssh")
                .args(["-o", "BatchMode=yes", &self.0, &format!("{PATH}; {script}")])
                .cwd(&ctx.root),
        )
    }
}

fn same_machine(a: &str, b: &str) -> bool {
    let name = |host: &str| {
        let host = host.rsplit('@').next().unwrap_or(host);
        host.split('.').next().unwrap_or(host).to_ascii_lowercase()
    };
    name(a) == name(b)
}

pub fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:@+,".contains(&b))
    {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Paths relative to the remote home, separate per originating machine.
struct Layout(String);

impl Layout {
    fn new() -> Self {
        let origin: String = crate::targets::hostname()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        Self(format!(".cache/wez-vtabs/remote/{origin}"))
    }
    fn source(&self) -> String {
        format!("{}/source", self.0)
    }
    fn cache(&self) -> String {
        format!("{}/cache", self.0)
    }
    fn out(&self, target: &str) -> String {
        format!("{}/out/{target}", self.0)
    }
}

/// Mirrors the working tree, including uncommitted changes; remote `target/` survives.
fn sync(ctx: &Context, host: &Host) -> Result<Layout> {
    let _stage = ctx.runner.stage("sync-source");
    let layout = Layout::new();
    let record = json!({"project_source": source::project_source(ctx)?}).to_string();
    host.capture(
        ctx,
        &format!(
            "mkdir -p {source} && printf '%s' {record} > {base}/build.json",
            source = layout.source(),
            record = quote(&record),
            base = layout.0,
        ),
    )?;
    let mut rsync = CommandSpec::new("rsync")
        .args(["-a", "--delete"])
        .cwd(&ctx.root);
    for ignored in source::IGNORED {
        rsync = rsync.arg(format!("--exclude={ignored}"));
    }
    for item in source::SOURCE_ITEMS {
        if ctx.root.join(item).exists() {
            rsync = rsync.arg(item);
        }
    }
    ctx.runner
        .run(rsync.arg(format!("{}:{}/", host.0, layout.source())))?;
    Ok(layout)
}

fn xtask(ctx: &Context, host: &Host, layout: &Layout, arguments: &[String]) -> Result<i32> {
    let arguments: Vec<String> = arguments.iter().map(|value| quote(value)).collect();
    host.run(
        ctx,
        &format!(
            r#"cd {source} && exec cargo xtask --project-root "$HOME/{source}" --cache "$HOME/{cache}" {arguments}"#,
            source = layout.source(),
            cache = layout.cache(),
            arguments = arguments.join(" "),
        ),
    )
}

fn pull(ctx: &Context, host: &Host, remote: &str, local: &Path) -> Result<()> {
    std::fs::create_dir_all(local)?;
    ctx.runner.run(
        CommandSpec::new("rsync")
            .args(["-a", "--delete"])
            .arg(format!("{}:{remote}/", host.0))
            .arg(format!("{}/", local.display()))
            .cwd(&ctx.root),
    )
}

fn push(ctx: &Context, local: &Path, host: &Host, remote: &str) -> Result<()> {
    host.capture(ctx, &format!("mkdir -p {remote}"))?;
    ctx.runner.run(
        CommandSpec::new("rsync")
            .args(["-a", "--delete"])
            .arg(format!("{}/", local.display()))
            .arg(format!("{}:{remote}/", host.0))
            .cwd(&ctx.root),
    )
}

pub fn pinned_upstream(ctx: &Context) -> Result<Option<String>> {
    if ctx.upstream.is_some() {
        return Ok(ctx.upstream.clone());
    }
    let recorded: Option<Value> = state::read_json(&ctx.cache.join("upstream.json"))?;
    if let Some(revision) = recorded.as_ref().and_then(|v| v["revision"].as_str()) {
        return Ok(Some(revision.into()));
    }
    let previous: Option<BuildMetadata> = state::read_json(&ctx.cache.join("build.json"))?;
    Ok(previous.map(|previous| previous.upstream))
}

pub fn record_upstream(ctx: &Context, revision: &str) -> Result<()> {
    state::write_json(
        &ctx.cache.join("upstream.json"),
        &json!({"revision": revision}),
    )
}

fn record_remote_upstream(ctx: &Context, host: &Host, layout: &Layout) -> Result<()> {
    let build = host.capture(ctx, &format!("cat {}/build.json", layout.cache()))?;
    let metadata: BuildMetadata = serde_json::from_str(&build).context("remote build record")?;
    record_upstream(ctx, &metadata.upstream)
}

fn for_target(ctx: &Context, target: &Target) -> Result<Context> {
    Ok(Context {
        role: target.role,
        upstream: pinned_upstream(ctx)?,
        ..ctx.clone()
    })
}

/// Bundle for a machine other than this one, built here; prebuilt when macOS is cross-compiled.
pub fn bundle_for(ctx: &Context, target: &Target, output: &Path) -> Result<PathBuf> {
    let ctx = &for_target(ctx, target)?;
    if target.platform == Platform::Macos && cfg!(target_os = "macos") {
        let metadata = build::build(ctx)?;
        return bundle::package(ctx, &metadata, output, false);
    }
    let options = container::Options::default();
    container::build(ctx, &target.platform, &options, output)
}

/// The macOS SDK is licensed with Xcode on this Mac; builders get a copy their linker accepts.
fn ensure_sdk(ctx: &Context, host: &Host) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let _stage = ctx.runner.stage("sync-sdk");
    let xcrun = |flag: &str| {
        ctx.runner
            .capture(CommandSpec::new("xcrun").arg(flag).cwd(&ctx.root))
    };
    let path = PathBuf::from(xcrun("--show-sdk-path")?).canonicalize()?;
    let version = format!("{} {}", xcrun("--show-sdk-version")?, path.display());
    let directory = ".cache/wez-vtabs/sdk";
    if host
        .capture(ctx, &format!("cat {directory}/version 2>/dev/null || true"))?
        .trim()
        == version
    {
        return Ok(());
    }
    let name = path.file_name().context("SDK name missing")?;
    let rename = if name == "MacOSX.sdk" {
        String::new()
    } else {
        format!(
            "mv {directory}.new/{} {directory}.new/MacOSX.sdk; ",
            quote(&name.to_string_lossy())
        )
    };
    let mut tar = Command::new("tar")
        .args(["--no-fflags", "--no-xattrs", "--no-mac-metadata", "-C"])
        .arg(path.parent().context("SDK parent missing")?)
        .arg("-cf")
        .arg("-")
        .arg(name)
        .stdout(Stdio::piped())
        .spawn()
        .context("start: tar")?;
    // LLVM rejects the arm64e.x1 slices newer SDK stubs list; nothing links against them.
    let script = format!(
        r#"set -e; rm -rf {directory}.new; mkdir -p {directory}.new; tar -xf - -C {directory}.new; {rename}find {directory}.new/MacOSX.sdk -type f -name '*.tbd' -exec grep -l 'arm64e\.x1-' {{}} + | xargs -r perl -0777 -pi -e 's/\s*,\s*arm64e\.x1-[a-z]+//g; s/arm64e\.x1-[a-z]+\s*,\s*//g'; printf '%s' {version} > {directory}.new/version; rm -rf {directory}; mv {directory}.new {directory}"#,
        version = quote(&version),
    );
    eprintln!("+ tar {} | ssh {} (sdk)", path.display(), host.0);
    let status = Command::new("ssh")
        .args(["-o", "BatchMode=yes", &host.0, &script])
        .stdin(tar.stdout.take().context("tar output missing")?)
        .status()
        .context("start: ssh")?;
    ensure!(tar.wait()?.success(), "tar of the macOS SDK failed");
    ensure!(status.success(), "macOS SDK transfer to {} failed", host.0);
    Ok(())
}

/// `deploy --prebuilt` with the prebuilt's own tool, on the terminal so codesign can prompt.
fn install_prebuilt(ctx: &Context, request: &Request, prebuilt: &Path) -> Result<i32> {
    let tool = prebuilt.join("bin/wez-vtabs");
    let arguments = request.arguments(false, ["deploy", "--offline", "--prebuilt"]);
    eprintln!(
        "+ {} {} {}",
        tool.display(),
        arguments.join(" "),
        prebuilt.display()
    );
    ctx.runner.metadata("prebuilt", &json!(prebuilt))?;
    let status = Command::new(&tool)
        .arg("--project-root")
        .arg(prebuilt.join("source"))
        .args(arguments)
        .arg(prebuilt)
        .status()
        .with_context(|| format!("start: {}", tool.display()))?;
    Ok(status.code().unwrap_or(1))
}

fn install(ctx: &Context, host: &Host, target: &Target, prebuilt: bool) -> Result<i32> {
    let (tool, source) = match (&target.platform, prebuilt) {
        (_, true) => ("bin/wez-vtabs", "--prebuilt"),
        (Platform::Macos, false) => ("WezTerm.app/Contents/MacOS/wez-vtabs", "--bundle"),
        (Platform::Linux(_), false) => ("bin/wez-vtabs", "--bundle"),
    };
    // Code signing over SSH needs the login keychain unlocked in this session.
    let unlock = if target.platform == Platform::Macos {
        "security show-keychain-info login.keychain-db >/dev/null 2>&1 || security unlock-keychain login.keychain-db; "
    } else {
        ""
    };
    host.run(
        ctx,
        &format!(
            r#"{unlock}exec "$HOME/{INCOMING}/{tool}" --project-root "$HOME/{INCOMING}/source" deploy {source} "$HOME/{INCOMING}" --offline"#
        ),
    )
}

/// Builds on the `--on` machine, then installs and places it on the `--to` machine.
pub fn deploy(
    ctx: &Context,
    plan: &Plan,
    request: &Request,
    rollback: Option<&Option<String>>,
) -> Result<(Value, i32)> {
    let target = plan.to.as_ref().context(MISSING)?;
    let role = target.role.name();
    if let Some(id) = rollback {
        let host = Host::connect(&target.host)?;
        let id = id.as_deref().map(quote).unwrap_or_default();
        let status = host.run(
            ctx,
            &format!("exec {INSTALLED_TOOL} deploy --rollback {id}"),
        )?;
        return Ok((Value::Null, status));
    }
    let bundle = match &plan.on {
        Some(builder) if same_machine(builder, &target.host) => {
            let host = Host::connect(builder)?;
            let layout = sync(ctx, &host)?;
            let status = xtask(
                ctx,
                &host,
                &layout,
                &request.arguments(true, ["deploy", "--on", LOCAL, "--role", role]),
            )?;
            if status == 0 {
                record_remote_upstream(ctx, &host, &layout)?;
            }
            return Ok((Value::Null, status));
        }
        Some(builder) => {
            let host = Host::connect(builder)?;
            if !target.local {
                Host::connect(&target.host)?;
            }
            if target.platform == Platform::Macos {
                ensure_sdk(ctx, &host)?;
            }
            let layout = sync(ctx, &host)?;
            let out = layout.out(&target.name);
            host.capture(ctx, &format!("rm -rf {out}"))?;
            let status = xtask(
                ctx,
                &host,
                &layout,
                &request.arguments(
                    true,
                    [
                        "build",
                        "--on",
                        LOCAL,
                        "--platform",
                        &target.platform.to_string(),
                        "--role",
                        role,
                        "--output",
                        &format!("../out/{}", target.name),
                    ],
                ),
            )?;
            if status != 0 {
                return Ok((Value::Null, status));
            }
            let names = host.capture(ctx, &format!("ls {out}"))?;
            let name = names.trim();
            ensure!(
                !name.is_empty() && !name.contains('\n'),
                "expected one bundle in {out}"
            );
            let staging = ctx.cache.join("staging").join(&target.name);
            pull(ctx, &host, &format!("{out}/{name}"), &staging)?;
            staging
        }
        None => {
            Host::connect(&target.host)?;
            bundle_for(ctx, target, &ctx.cache.join("out").join(&target.name))?
        }
    };
    let metadata: BuildMetadata =
        state::read_json(&bundle.join("build.json"))?.context("bundle metadata missing")?;
    record_upstream(ctx, &metadata.upstream)?;
    let prebuilt = bundle.join("prebuilt.json").is_file();
    if target.local && prebuilt {
        return Ok((Value::Null, install_prebuilt(ctx, request, &bundle)?));
    }
    if target.local {
        let value = deploy::deploy(
            ctx,
            deploy::Source::Bundle(&bundle),
            &deploy::Targets::resolve(None, None, false),
            None,
        )?;
        return Ok((value, 0));
    }
    let host = Host::connect(&target.host)?;
    push(ctx, &bundle, &host, INCOMING)?;
    Ok((Value::Null, install(ctx, &host, target, prebuilt)?))
}

/// Compiles and validates for the `--to` machine on the `--on` machine; nothing is installed.
pub fn build(ctx: &Context, plan: &Plan, request: &Request) -> Result<(Value, i32)> {
    let target = plan.to.as_ref().context(MISSING)?;
    let role = target.role.name();
    match &plan.on {
        Some(builder) => {
            let host = Host::connect(builder)?;
            if target.platform == Platform::Macos {
                ensure_sdk(ctx, &host)?;
            }
            let layout = sync(ctx, &host)?;
            let platform = target.platform.to_string();
            let mut command = vec!["build", "--on", LOCAL, "--role", role];
            if !same_machine(builder, &target.host) {
                command.extend(["--platform", &platform]);
            }
            let status = xtask(ctx, &host, &layout, &request.arguments(true, command))?;
            Ok((Value::Null, status))
        }
        None => {
            let output = ctx.root.join("dist");
            if ctx.explain && !(target.platform == Platform::Macos && cfg!(target_os = "macos")) {
                let ctx = &for_target(ctx, target)?;
                let options = container::Options::default();
                return Ok((
                    container::plan(ctx, &target.platform, &options, &output)?,
                    0,
                ));
            }
            let bundle = bundle_for(ctx, target, &output)?;
            Ok((json!({"target": target.name, "bundle": bundle}), 0))
        }
    }
}

/// Runs a development command on `host` against the synced working tree.
pub fn delegate(
    ctx: &Context,
    host: &str,
    request: &Request,
    command: &[String],
) -> Result<(Value, i32)> {
    let host = Host::connect(host)?;
    let layout = sync(ctx, &host)?;
    let arguments = request.arguments(false, command.iter().map(String::as_str));
    Ok((Value::Null, xtask(ctx, &host, &layout, &arguments)?))
}

/// Runs the deployed tool on the target; no source is needed.
pub fn installed(ctx: &Context, target: &Target, command: &[&str]) -> Result<(Value, i32)> {
    let host = Host::connect(&target.host)?;
    let command: Vec<String> = command.iter().map(|value| quote(value)).collect();
    let status = host.run(ctx, &format!("exec {INSTALLED_TOOL} {}", command.join(" ")))?;
    Ok((Value::Null, status))
}

//! Build, install and place the active version where the desktop launches it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::process::CommandSpec;
use crate::state::{BuildMetadata, Context, Lock, Role};
use crate::{build, bundle, install};

const DESKTOP_MARKER: &str = "X-WezVtabs-Install=";

fn linked(role: Role) -> &'static [&'static str] {
    match role {
        Role::Desktop => &[
            "wezterm",
            "wezterm-gui",
            "wezterm-mux-server",
            "wez-vtabs",
            "wez-vtabs-store",
            "strip-ansi-escapes",
        ],
        Role::Mux => &["wezterm", "wezterm-mux-server", "wez-vtabs"],
    }
}

pub enum Source<'a> {
    Build,
    Bundle(&'a Path),
    /// The previous version, or an installed ID.
    Rollback(Option<&'a str>),
    /// Binaries cross-compiled elsewhere; packaged and signed here.
    Prebuilt(&'a Path),
}

pub struct Targets {
    pub app: Option<PathBuf>,
    pub bin: Option<PathBuf>,
}

impl Targets {
    pub fn resolve(app: Option<PathBuf>, bin: Option<PathBuf>, disabled: bool) -> Self {
        let bin = bin.or_else(|| Some(crate::home().join(".local/bin")));
        if disabled || !(cfg!(target_os = "macos") || cfg!(target_os = "linux")) {
            Self {
                app: None,
                bin: None,
            }
        } else if cfg!(target_os = "macos") {
            Self {
                app: app.or_else(|| Some(PathBuf::from("/Applications/WezTerm.app"))),
                bin,
            }
        } else {
            Self {
                app: app.or_else(|| {
                    Some(
                        crate::data_home()
                            .join("applications")
                            .join("org.wezfurlong.wezterm.desktop"),
                    )
                }),
                bin,
            }
        }
    }
}

pub fn deploy(
    ctx: &Context,
    source: Source,
    targets: &Targets,
    build_lock: Option<&Lock>,
) -> Result<Value> {
    let pinned = pin_upstream(ctx)?;
    let ctx = pinned.as_ref().unwrap_or(ctx);
    let (installed, pruned) = match source {
        Source::Rollback(id) => (install::rollback(&ctx.install, id)?, Value::Null),
        Source::Bundle(path) => (
            install::install(ctx, path, false)?,
            crate::diagnostics::prune(ctx, build_lock)?,
        ),
        Source::Build => {
            let metadata = build::build(ctx)?;
            let bundle = bundle::package(ctx, &metadata, &ctx.cache.join("bundles"), false)?;
            (
                install::install(ctx, &bundle, false)?,
                crate::diagnostics::prune(ctx, build_lock)?,
            )
        }
        Source::Prebuilt(path) => {
            let bundle = bundle::package_prebuilt(ctx, path, &ctx.cache.join("bundles"))?;
            (
                install::install(ctx, &bundle, false)?,
                crate::diagnostics::prune(ctx, build_lock)?,
            )
        }
    };
    let metadata = crate::state::read_json::<BuildMetadata>(&installed.join("build.json"))?
        .context("installed version metadata missing")?;
    let role = metadata.role;
    // Headless versions have no application; never replace one with them.
    let targets = &Targets {
        app: targets.app.clone().filter(|_| role == Role::Desktop),
        bin: targets.bin.clone(),
    };
    if targets.bin.is_some() {
        ensure_binaries(&installed, role)?;
    }
    let app = match &targets.app {
        Some(app) if cfg!(target_os = "macos") => Some(place_app(ctx, &installed, app)?),
        Some(entry) => Some(place_desktop(ctx, &installed, entry)?),
        None => None,
    };
    let links = match targets.bin.as_deref() {
        Some(bin) => place_links(ctx, &link_source(&installed, targets)?, bin, role)?,
        None => Vec::new(),
    };
    Ok(json!({
        "installed": installed,
        "app": app,
        "links": links,
        "upstream": {"revision": metadata.upstream, "pinned": pinned.is_some()},
        "pruned": pruned,
        "next": "Quit and reopen WezTerm to run this version",
    }))
}

/// Plugin changes should not pull a newer WezTerm and its full rebuild along; only an
/// explicit `--upstream` moves past the revision the last deploy used on any machine.
fn pin_upstream(ctx: &Context) -> Result<Option<Context>> {
    if ctx.upstream.is_some() {
        return Ok(None);
    }
    Ok(
        crate::remote::pinned_upstream(ctx)?.map(|upstream| Context {
            upstream: Some(upstream),
            ..ctx.clone()
        }),
    )
}

/// Swap the application atomically; a running instance keeps its renamed files open.
fn place_app(ctx: &Context, installed: &Path, app: &Path) -> Result<Value> {
    let _stage = ctx.runner.stage("deploy-app");
    let source = installed.join("WezTerm.app");
    ensure!(
        source.join("Contents/MacOS").is_dir(),
        "installed version has no application bundle"
    );
    let app = absolute(app)?;
    let staging = tempfile::Builder::new()
        .prefix(".wez-vtabs-deploy-")
        .tempdir_in(app.parent().unwrap())?;
    let staged = staging.path().join("WezTerm.app");
    bundle::copy_tree(&source, &staged)?;
    if source.join("Contents/_CodeSignature").is_dir() {
        ctx.runner.run(
            CommandSpec::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&staged)
                .cwd(&ctx.root),
        )?;
    }
    let owned = app.join("Contents/Resources/bundle.json").is_file();
    let retired = retire(ctx, &app, owned)?;
    if let Err(error) = fs::rename(&staged, &app) {
        if let Some(retired) = &retired {
            move_path(&retired.path, &app)?;
        }
        return Err(error).with_context(|| format!("placing {}", app.display()));
    }
    for name in ["wezterm-gui", "wez-vtabs"] {
        let relative = Path::new("Contents/MacOS").join(bundle::executable_name(name));
        ensure!(
            bundle::hash_file(&app.join(&relative))? == bundle::hash_file(&source.join(&relative))?,
            "placed application differs from the installed version: {name}"
        );
    }
    fs::File::open(&app)?.set_modified(SystemTime::now())?;
    Ok(json!({"path": app, "replaced": finish(retired)?}))
}

/// A user desktop entry with the system entry's name takes XDG precedence in launchers.
fn place_desktop(ctx: &Context, installed: &Path, entry: &Path) -> Result<Value> {
    let _stage = ctx.runner.stage("deploy-desktop");
    let entry = absolute(entry)?;
    let dispatcher = install::dispatcher(&ctx.install)?;
    let marker = format!("{DESKTOP_MARKER}{}\n", ctx.install.display());
    let owned = fs::read_to_string(&entry).is_ok_and(|text| text.contains(DESKTOP_MARKER));
    let retired = retire(ctx, &entry, owned)?;
    install::atomic_text(
        &entry,
        &install::desktop_entry("WezTerm", &dispatcher, installed, &marker),
        true,
    )?;
    let replaced = finish(retired)?;
    // Launchers that index a cache pick the entry up sooner; absence is not an error.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(entry.parent().unwrap())
        .status();
    Ok(json!({"path": entry, "replaced": replaced}))
}

fn link_source(installed: &Path, targets: &Targets) -> Result<PathBuf> {
    match &targets.app {
        Some(app) if cfg!(target_os = "macos") => Ok(absolute(app)?.join("Contents/MacOS")),
        _ => Ok(bundle::binary_dir(installed)),
    }
}

/// A link earlier on PATH shadows a packaged CLI on both macOS and Linux.
fn place_links(ctx: &Context, binaries: &Path, bin: &Path, role: Role) -> Result<Vec<Value>> {
    let _stage = ctx.runner.stage("deploy-links");
    fs::create_dir_all(bin)?;
    let bin = bin.canonicalize()?;
    let mut links = Vec::new();
    for name in linked(role) {
        let link = bin.join(bundle::executable_name(name));
        let target = binaries.join(bundle::executable_name(name));
        let owned = fs::read_link(&link).is_ok_and(|current| {
            current.starts_with(&ctx.install) || current.starts_with(binaries)
        });
        let retired = retire(ctx, &link, owned)?;
        symlink(&target, &link)?;
        links.push(json!({"path": link, "target": target, "replaced": finish(retired)?}));
    }
    // A role change leaves own links to binaries this version does not have.
    for name in linked(Role::Desktop)
        .iter()
        .filter(|name| !linked(role).contains(name))
    {
        let link = bin.join(bundle::executable_name(name));
        if fs::read_link(&link).is_ok_and(|current| current.starts_with(&ctx.install)) {
            fs::remove_file(&link)?;
            links.push(json!({"path": link, "removed": true}));
        }
    }
    Ok(links)
}

fn ensure_binaries(installed: &Path, role: Role) -> Result<()> {
    let binaries = bundle::binary_dir(installed);
    for name in linked(role) {
        ensure!(
            binaries.join(bundle::executable_name(name)).is_file(),
            "installed version has no {name} binary"
        );
    }
    Ok(())
}

struct Retired {
    owned: bool,
    path: PathBuf,
}

/// Own artifacts are discarded after the swap; anything else is kept under the install root.
fn retire(ctx: &Context, path: &Path, owned: bool) -> Result<Option<Retired>> {
    if fs::symlink_metadata(path).is_err() {
        return Ok(None);
    }
    let name = path.file_name().context("target name missing")?;
    let destination = if owned {
        path.with_file_name(format!(
            ".{}.retired-{}",
            name.to_string_lossy(),
            crate::state::now()
        ))
    } else {
        let preserved = ctx.install.join("replaced").join(name);
        fs::create_dir_all(preserved.parent().unwrap())?;
        remove_path(&preserved)?;
        preserved
    };
    move_path(path, &destination)?;
    Ok(Some(Retired {
        owned,
        path: destination,
    }))
}

fn finish(retired: Option<Retired>) -> Result<Value> {
    Ok(match retired {
        Some(Retired { owned: true, path }) => {
            remove_path(&path)?;
            json!({"kind": "previous"})
        }
        Some(Retired { owned: false, path }) => json!({"kind": "foreign", "preserved": path}),
        None => Value::Null,
    })
}

fn absolute(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("target parent missing")?;
    fs::create_dir_all(parent)?;
    Ok(parent
        .canonicalize()?
        .join(path.file_name().context("target name missing")?))
}

fn symlink(target: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
            .with_context(|| format!("linking {} to {}", link.display(), target.display()))
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!(
            "symbolic links are not deployed on this platform: {} -> {}",
            link.display(),
            target.display()
        )
    }
}

fn remove_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn move_path(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            let metadata = fs::symlink_metadata(from)?;
            if metadata.is_dir() {
                bundle::copy_tree(from, to)?;
            } else if metadata.file_type().is_symlink() {
                symlink(&fs::read_link(from)?, to)?;
            } else {
                fs::copy(from, to)?;
            }
            remove_path(from)
        }
        Err(error) => {
            Err(error).with_context(|| format!("moving {} to {}", from.display(), to.display()))
        }
    }
}

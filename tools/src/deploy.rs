//! Build, install and place the active version where the desktop launches it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::process::CommandSpec;
use crate::state::{Context, Lock};
use crate::{build, bundle, install};

const DESKTOP_MARKER: &str = "X-WezVtabs-Install=";
const LINKS: [&str; 3] = ["wezterm", "wezterm-gui", "wezterm-mux-server"];

/// macOS replaces an application bundle; Linux shadows the system desktop entry and CLI.
pub struct Targets {
    pub app: Option<PathBuf>,
    pub bin: Option<PathBuf>,
}

impl Targets {
    pub fn resolve(app: Option<PathBuf>, bin: Option<PathBuf>, disabled: bool) -> Self {
        if disabled {
            Self {
                app: None,
                bin: None,
            }
        } else if cfg!(target_os = "macos") {
            Self {
                app: app.or_else(|| Some(PathBuf::from("/Applications/WezTerm.app"))),
                bin: None,
            }
        } else if cfg!(target_os = "linux") {
            Self {
                app: app.or_else(|| {
                    Some(
                        crate::data_home()
                            .join("applications")
                            .join("org.wezfurlong.wezterm.desktop"),
                    )
                }),
                bin: bin.or_else(|| Some(crate::home().join(".local/bin"))),
            }
        } else {
            Self {
                app: None,
                bin: None,
            }
        }
    }
}

pub fn deploy(
    ctx: &Context,
    existing: Option<&Path>,
    targets: &Targets,
    build_lock: Option<&Lock>,
) -> Result<Value> {
    let pinned = pin_upstream(ctx)?;
    let ctx = pinned.as_ref().unwrap_or(ctx);
    let bundle = match existing {
        Some(path) => path.to_path_buf(),
        None => {
            let metadata = build::build(ctx)?;
            bundle::package(ctx, &metadata, &ctx.cache.join("bundles"), false)?
        }
    };
    let installed = install::install(ctx, &bundle, false)?;
    let pruned = crate::diagnostics::prune(ctx, build_lock)?;
    let app = match &targets.app {
        Some(app) if cfg!(target_os = "macos") => Some(place_app(ctx, &installed, app)?),
        Some(entry) => Some(place_desktop(
            ctx,
            &installed,
            entry,
            targets.bin.as_deref(),
        )?),
        None => None,
    };
    Ok(json!({
        "installed": installed,
        "app": app,
        "upstream": {"revision": ctx.upstream, "pinned": pinned.is_some()},
        "pruned": pruned,
        "next": "Quit and reopen WezTerm to run this version",
    }))
}

/// Plugin changes should not pull a newer WezTerm and its full rebuild along; only an
/// explicit `--upstream` moves past the revision the last build used.
fn pin_upstream(ctx: &Context) -> Result<Option<Context>> {
    if ctx.upstream.is_some() {
        return Ok(None);
    }
    let previous: Option<crate::state::BuildMetadata> =
        crate::state::read_json(&ctx.cache.join("build.json"))?;
    Ok(previous.map(|previous| Context {
        upstream: Some(previous.upstream),
        ..ctx.clone()
    }))
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

/// A user desktop entry with the system entry's name takes XDG precedence in launchers;
/// `~/.local/bin` links put the matching CLI ahead of a packaged one on PATH.
fn place_desktop(
    ctx: &Context,
    installed: &Path,
    entry: &Path,
    bin: Option<&Path>,
) -> Result<Value> {
    let _stage = ctx.runner.stage("deploy-desktop");
    let binaries = bundle::binary_dir(installed);
    for name in LINKS {
        ensure!(
            binaries.join(bundle::executable_name(name)).is_file(),
            "installed version has no {name} binary"
        );
    }
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
    let mut links = Vec::new();
    if let Some(bin) = bin {
        fs::create_dir_all(bin)?;
        let bin = bin.canonicalize()?;
        for name in LINKS {
            let link = bin.join(bundle::executable_name(name));
            let target = binaries.join(bundle::executable_name(name));
            let owned = fs::read_link(&link).is_ok_and(|current| current.starts_with(&ctx.install));
            let retired = retire(ctx, &link, owned)?;
            symlink(&target, &link)?;
            links.push(json!({"path": link, "target": target, "replaced": finish(retired)?}));
        }
    }
    // Launchers that index a cache pick the entry up sooner; absence is not an error.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(entry.parent().unwrap())
        .status();
    Ok(json!({"path": entry, "replaced": replaced, "links": links}))
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

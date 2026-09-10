//! Build, install and place the active version where the desktop launches it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::process::CommandSpec;
use crate::state::Context;
use crate::{build, bundle, install};

pub fn default_app() -> Option<PathBuf> {
    cfg!(target_os = "macos").then(|| PathBuf::from("/Applications/WezTerm.app"))
}

pub fn deploy(ctx: &Context, existing: Option<&Path>, app: Option<&Path>) -> Result<Value> {
    let bundle = match existing {
        Some(path) => path.to_path_buf(),
        None => {
            let metadata = build::build(ctx)?;
            bundle::package(ctx, &metadata, &ctx.cache.join("bundles"), false)?
        }
    };
    let installed = install::install(ctx, &bundle, false)?;
    let app = match app {
        Some(app) => Some(place_app(ctx, &installed, app)?),
        None => None,
    };
    Ok(json!({
        "installed": installed,
        "app": app,
        "next": "Quit and reopen WezTerm to run this version",
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
    let parent = app.parent().context("application parent missing")?;
    fs::create_dir_all(parent)?;
    let app = parent
        .canonicalize()?
        .join(app.file_name().context("application name missing")?);
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
    let retired = retire(ctx, &app)?;
    if let Err(error) = fs::rename(&staged, &app) {
        if let Some(retired) = &retired {
            move_tree(&retired.path, &app)?;
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
    let replaced = match retired {
        Some(Retired {
            kind: Kind::Previous,
            path,
        }) => {
            fs::remove_dir_all(path)?;
            json!({"kind": "previous"})
        }
        Some(Retired {
            kind: Kind::Foreign,
            path,
        }) => json!({"kind": "foreign", "preserved": path}),
        Some(Retired {
            kind: Kind::Link, ..
        }) => json!({"kind": "symlink"}),
        None => Value::Null,
    };
    Ok(json!({"path": app, "replaced": replaced}))
}

enum Kind {
    Previous,
    Foreign,
    Link,
}

struct Retired {
    kind: Kind,
    path: PathBuf,
}

/// Own copies are discarded after the swap; anything else is kept under the install root.
fn retire(ctx: &Context, app: &Path) -> Result<Option<Retired>> {
    let Ok(metadata) = fs::symlink_metadata(app) else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() {
        fs::remove_file(app)?;
        return Ok(Some(Retired {
            kind: Kind::Link,
            path: app.to_path_buf(),
        }));
    }
    let name = app.file_name().context("application name missing")?;
    if app.join("Contents/Resources/native-bundle.json").is_file() {
        let path = app.with_file_name(format!(
            ".{}.retired-{}",
            name.to_string_lossy(),
            crate::state::now()
        ));
        fs::rename(app, &path)?;
        return Ok(Some(Retired {
            kind: Kind::Previous,
            path,
        }));
    }
    let preserved = ctx.install.join("replaced").join(name);
    fs::create_dir_all(preserved.parent().unwrap())?;
    if preserved.exists() {
        fs::remove_dir_all(&preserved)?;
    }
    move_tree(app, &preserved)?;
    Ok(Some(Retired {
        kind: Kind::Foreign,
        path: preserved,
    }))
}

fn move_tree(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            bundle::copy_tree(from, to)?;
            fs::remove_dir_all(from)?;
            Ok(())
        }
        Err(error) => {
            Err(error).with_context(|| format!("moving {} to {}", from.display(), to.display()))
        }
    }
}

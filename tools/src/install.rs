//! Installed versions are immutable; only small pointer files are replaced.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bundle;
use crate::state::{self, Context, Lock};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Pointer {
    id: String,
}

fn pointer(root: &Path, name: &str) -> Result<Option<Pointer>> {
    let pointer: Option<Pointer> = state::read_json(&root.join(format!("{name}.json")))?;
    if let Some(pointer) = &pointer {
        state::safe_id(&pointer.id)?;
    }
    Ok(pointer)
}

pub fn gui_path(bundle: &Path) -> PathBuf {
    bundle::binary_dir(bundle).join(bundle::executable_name("wezterm-gui"))
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn activate(root: &Path, next: &Pointer) -> Result<()> {
    if let Some(active) = pointer(root, "active")?
        && active.id != next.id
    {
        state::write_json(&root.join("previous.json"), &active)?;
    }
    state::write_json(&root.join("active.json"), next)?;
    remove_if_exists(&root.join("pending.json"))?;
    Ok(())
}

pub fn install(ctx: &Context, bundle: &Path, stage_only: bool) -> Result<PathBuf> {
    let _stage = ctx.runner.stage("install");
    let metadata = bundle::verify(bundle)?;
    let versions = ctx.install.join("versions");
    fs::create_dir_all(&versions)?;
    let destination = versions.join(&metadata.id);
    let _lock = Lock::acquire(&ctx.install.join("install.lock"))?;
    if destination.exists() {
        bundle::verify(&destination)?;
        ensure!(
            fs::read(destination.join("checksums.json"))?
                == fs::read(bundle.join("checksums.json"))?,
            "installed bundle ID collision: immutable version contents differ"
        );
    } else {
        let staging = tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(&versions)?;
        bundle::copy_tree(bundle, staging.path())?;
        bundle::verify(staging.path())?;
        fs::rename(staging.path(), &destination)?;
    }
    // Prepare all launch infrastructure before committing the active pointer.
    install_entry(&ctx.install, &destination)?;
    let next = Pointer { id: metadata.id };
    if stage_only {
        state::write_json(&ctx.install.join("pending.json"), &next)?;
    } else {
        activate(&ctx.install, &next)?;
    }
    state::write_json(
        &ctx.install.join("runtime.json"),
        &json!({"updater_protocol":1,"tool":"wez-vtabs","launcher_protocol":1}),
    )?;
    Ok(destination)
}

pub fn current_bundle(root: &Path, promote: bool) -> Result<PathBuf> {
    let _lock = Lock::acquire(&root.join("install.lock"))?;
    if promote && let Some(pending) = pointer(root, "pending")? {
        let path = root.join("versions").join(&pending.id);
        let metadata = bundle::verify(&path).context("pending bundle incomplete or invalid")?;
        ensure!(
            metadata.id == pending.id,
            "pending bundle identity mismatch"
        );
        activate(root, &pending)?;
    }
    let active =
        pointer(root, "active")?.context("native bundle not installed; run just install")?;
    let bundle = root.join("versions").join(&active.id);
    // Hashing the complete source distribution on every launch would delay
    // startup. Installation and promotion verify it; launch checks essentials.
    let metadata: state::BuildMetadata =
        state::read_json(&bundle.join("build.json"))?.context("active bundle missing")?;
    ensure!(
        metadata.id == active.id && metadata.capability == 1,
        "active bundle identity mismatch"
    );
    ensure!(
        gui_path(&bundle).is_file() && bundle::tool_path(&bundle).is_file(),
        "active bundle incomplete"
    );
    Ok(bundle)
}

pub fn rollback(root: &Path, id: Option<&str>) -> Result<PathBuf> {
    let _lock = Lock::acquire(&root.join("install.lock"))?;
    let id = match id {
        Some(id) => state::safe_id(id)?.to_owned(),
        None => {
            pointer(root, "previous")?
                .context("no previous version; select an installed version")?
                .id
        }
    };
    let path = root.join("versions").join(&id);
    ensure!(
        bundle::verify(&path)?.id == id,
        "rollback bundle identity mismatch"
    );
    activate(root, &Pointer { id })?;
    Ok(path)
}

pub fn versions(root: &Path) -> Result<Value> {
    let active = pointer(root, "active")?.map(|value| value.id);
    let pending = pointer(root, "pending")?.map(|value| value.id);
    let previous = pointer(root, "previous")?.map(|value| value.id);
    let mut versions = Vec::new();
    let directory = root.join("versions");
    if directory.is_dir() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type()?.is_dir() || id.starts_with('.') {
                continue;
            }
            state::safe_id(&id)?;
            let metadata: Option<state::BuildMetadata> =
                state::read_json(&entry.path().join("build.json"))?;
            versions.push(json!({"id":id,"path":entry.path(),"active":active.as_deref()==Some(&id),"pending":pending.as_deref()==Some(&id),"previous":previous.as_deref()==Some(&id),"metadata":metadata}));
        }
    }
    versions.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok(json!(versions))
}

pub fn status(root: &Path) -> Result<Value> {
    Ok(json!({
        "install":root,"active":pointer(root,"active")?,"pending":pointer(root,"pending")?,
        "previous":pointer(root,"previous")?,"update":state::read_json::<Value>(&root.join("update.json"))?,
        "versions":versions(root)?
    }))
}

fn atomic_text(path: &Path, text: &str, executable: bool) -> Result<()> {
    use std::io::Write;
    fs::create_dir_all(path.parent().context("launcher parent missing")?)?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(text.as_bytes())?;
    if executable {
        bundle::set_mode(temporary.path(), 0o755)?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn desktop_quote(value: &str) -> String {
    let mut value = value.replace('%', "%%");
    for character in ['\\', '"', '`', '$'] {
        value = value.replace(character, &format!("\\{character}"));
    }
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

fn install_entry(root: &Path, bundle: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let dispatcher = root.join(bundle::executable_name("wez-vtabs-launcher"));
    if !dispatcher.exists() {
        let temporary = tempfile::NamedTempFile::new_in(&root)?;
        fs::copy(bundle::tool_path(bundle), temporary.path())?;
        temporary.as_file().sync_all()?;
        temporary.persist_noclobber(&dispatcher)?;
    }
    // The stable dispatcher is intentionally installed once. Every launch
    // delegates to the current immutable version, including on Windows where
    // replacing an executing .exe would fail.
    if cfg!(windows) {
        atomic_text(
            &root.join("wez-vtabs.cmd"),
            "@echo off\r\n\"%~dp0wez-vtabs-launcher.exe\" launch -- %*\r\n",
            false,
        )?;
    } else {
        let script = format!(
            "#!/bin/sh\nexec {} launch -- \"$@\"\n",
            shell_quote(&dispatcher.to_string_lossy())
        );
        atomic_text(&root.join("wez-vtabs"), &script, true)?;
        if cfg!(target_os = "macos") {
            let contents = root.join("WezTerm Native.app/Contents");
            fs::create_dir_all(contents.join("Resources"))?;
            atomic_text(&contents.join("MacOS/launch"), &script, true)?;
            let icon = bundle.join("WezTerm.app/Contents/Resources/terminal.icns");
            if icon.is_file() {
                fs::copy(icon, contents.join("Resources/terminal.icns"))?;
            }
            let mut info = plist::Dictionary::new();
            for (key, value) in [
                ("CFBundleExecutable", "launch"),
                ("CFBundleIdentifier", "dev.fredrir.wez-vtabs.launcher"),
                ("CFBundleName", "WezTerm Native"),
                ("CFBundleDisplayName", "WezTerm Native"),
                ("CFBundlePackageType", "APPL"),
                ("CFBundleVersion", "1"),
                ("CFBundleIconFile", "terminal.icns"),
            ] {
                info.insert(key.into(), plist::Value::String(value.into()));
            }
            info.insert(
                "NSHighResolutionCapable".into(),
                plist::Value::Boolean(true),
            );
            let mut bytes = Vec::new();
            plist::Value::Dictionary(info).to_writer_xml(&mut bytes)?;
            atomic_text(
                &contents.join("Info.plist"),
                std::str::from_utf8(&bytes)?,
                false,
            )?;
        } else {
            let icon = bundle
                .join("share/icons/terminal.png")
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            atomic_text(
                &root.join("wez-vtabs.desktop"),
                &format!(
                    "[Desktop Entry]\nName=WezTerm Native\nType=Application\nTerminal=false\nCategories=System;TerminalEmulator;\nStartupWMClass=org.wezfurlong.wezterm\nExec={} launch\nIcon={icon}\n",
                    desktop_quote(&dispatcher.to_string_lossy())
                ),
                true,
            )?;
        }
    }
    Ok(())
}

pub fn managed_context(executable: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    let Some(directory) = executable.parent() else {
        return Ok(None);
    };
    if directory.join("active.json").is_file() || directory.join("pending.json").is_file() {
        // First staged installs do not have an active version until launched.
        let active = pointer(directory, "active")?
            .or(pointer(directory, "pending")?)
            .context("installed version missing")?;
        return Ok(Some((
            directory.join("versions").join(active.id).join("source"),
            directory.to_path_buf(),
        )));
    }
    let marker_directory = if cfg!(target_os = "macos") {
        directory.join("../Resources")
    } else {
        directory.to_path_buf()
    };
    let Some(marker) = state::read_json::<Value>(&marker_directory.join("native-bundle.json"))?
    else {
        return Ok(None);
    };
    ensure!(
        marker["capability"] == 1 && marker["updater_protocol"] == 1,
        "native bundle contract mismatch"
    );
    let bundle = marker_directory
        .join(marker["root"].as_str().context("bundle root missing")?)
        .canonicalize()?;
    let install = if bundle
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|name| name == "versions")
    {
        bundle
            .parent()
            .and_then(Path::parent)
            .context("install root missing")?
            .to_path_buf()
    } else {
        // Uninstalled runtime tools still know their source; retain the configured
        // install location rather than enabling implicit updates for dev bundles.
        return Ok(None);
    };
    Ok(Some((bundle.join("source"), install)))
}

pub fn launch(ctx: &Context, arguments: &[String]) -> Result<i32> {
    let offline = ctx.offline || std::env::var("WEZ_VTABS_OFFLINE").as_deref() == Ok("1");
    let bundle = current_bundle(&ctx.install, true)?;
    let tool = bundle::tool_path(&bundle).canonicalize()?;
    if std::env::current_exe()?.canonicalize()? != tool {
        let mut command = Command::new(tool);
        command.arg("--project-root").arg(bundle.join("source"));
        if offline {
            command.arg("--offline").env("WEZ_VTABS_OFFLINE", "1");
        }
        let status = command
            .args(["launch", "--"])
            .args(arguments)
            .env("WEZ_VTABS_INSTALL", &ctx.install)
            .env("WEZ_VTABS_CACHE", &ctx.cache)
            .status()?;
        return Ok(status.code().unwrap_or(1));
    }
    if !offline {
        crate::update::queue_daily_update(&bundle, &ctx.install)?;
    }
    let leases = ctx.install.join("leases");
    fs::create_dir_all(&leases)?;
    let marker = leases.join(format!(
        "{}.lock",
        bundle
            .file_name()
            .context("version missing")?
            .to_string_lossy()
    ));
    let lease = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(marker)?;
    fs2::FileExt::lock_shared(&lease)?;
    let args = if arguments.is_empty() {
        vec!["start".to_owned()]
    } else {
        arguments.to_vec()
    };
    let mut command = Command::new(gui_path(&bundle));
    command.args(args).env("WEZ_VTABS_BUNDLE", &bundle);
    if offline {
        command.env("WEZ_VTABS_OFFLINE", "1");
    }
    let status = command.status();
    drop(lease);
    Ok(status?.code().unwrap_or(1))
}

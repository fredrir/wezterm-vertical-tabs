use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

static SCHEDULED: AtomicBool = AtomicBool::new(false);

fn script() -> anyhow::Result<Option<PathBuf>> {
    let exe = std::env::current_exe()?;
    let Some(directory) = exe.parent() else {
        return Ok(None);
    };
    let directory = if cfg!(target_os = "macos") {
        directory.join("../Resources")
    } else {
        directory.to_path_buf()
    };
    let marker = directory.join("native-bundle.json");
    if !marker.is_file() {
        return Ok(None);
    }
    let marker: serde_json::Value = serde_json::from_slice(&std::fs::read(marker)?)?;
    anyhow::ensure!(marker["capability"] == 1, "native bundle contract mismatch");
    let relative = marker["root"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("native bundle root missing"))?;
    let root = directory.join(relative).canonicalize()?;
    // Only installed immutable bundles perform updates. Development archives do not.
    if root
        .parent()
        .and_then(|path| path.file_name())
        .is_none_or(|name| name != "versions")
    {
        return Ok(None);
    }
    let install = root
        .parent()
        .and_then(|path| path.parent())
        .ok_or_else(|| anyhow::anyhow!("native install root missing"))?;
    if !install.join("active.json").is_file() {
        return Ok(None);
    }
    Ok(Some(root.join("source/scripts/native.py")))
}

pub fn schedule() {
    if SCHEDULED.swap(true, Ordering::Relaxed) {
        return;
    }
    promise::spawn::spawn(async {
        let result = smol::unblock(|| -> anyhow::Result<()> {
            let Some(script) = script()? else {
                return Ok(());
            };
            let install = script
                .ancestors()
                .nth(5)
                .ok_or_else(|| anyhow::anyhow!("native install root missing"))?;
            let state = install.join("update.json");
            if state.is_file() {
                let state: serde_json::Value = serde_json::from_slice(&std::fs::read(state)?)?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();
                if now.saturating_sub(state["last_attempt"].as_u64().unwrap_or(0)) < 24 * 60 * 60 {
                    return Ok(());
                }
            }
            let log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(install.join("update.log"))?;
            let runtime: serde_json::Value = serde_json::from_slice(
                &std::fs::read(install.join("runtime.json")).map_err(|error| {
                    anyhow::anyhow!("native Python runtime missing; run just install: {error}")
                })?,
            )?;
            let python = runtime["python"].as_str().ok_or_else(|| {
                anyhow::anyhow!("native Python runtime invalid; run just install")
            })?;
            let mut command = std::process::Command::new(python);
            command
                .arg(&script)
                .args(["update", "--daily", "--stage-only"])
                .env("WEZ_VTABS_INSTALL", install);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            // The worker owns its build lifetime and an OS lock. A GUI exit cannot
            // cancel an update midway through installing the next immutable version.
            smol::process::Command::from(command)
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log)
                .reap_on_drop(true)
                .spawn()?;
            Ok(())
        })
        .await;
        if let Err(error) = result {
            log::warn!("native update: {error:#}");
        }
    })
    .detach();
}

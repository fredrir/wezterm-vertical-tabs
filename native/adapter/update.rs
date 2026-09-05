use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

static SCHEDULED: AtomicBool = AtomicBool::new(false);

fn updater() -> anyhow::Result<Option<(PathBuf, PathBuf, PathBuf)>> {
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
    anyhow::ensure!(
        marker["updater_protocol"] == 1,
        "native updater protocol mismatch"
    );
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
    let relative_tool = marker["tool"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("native updater tool missing"))?;
    anyhow::ensure!(
        std::path::Path::new(relative_tool)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))),
        "invalid native updater tool path"
    );
    let tool = root.join(relative_tool).canonicalize()?;
    anyhow::ensure!(
        tool.starts_with(&root) && tool.is_file(),
        "native updater tool invalid"
    );
    Ok(Some((tool, install.to_path_buf(), root.join("source"))))
}

pub fn schedule() {
    if std::env::var("WEZ_VTABS_OFFLINE").as_deref() == Ok("1") {
        return;
    }
    if SCHEDULED.swap(true, Ordering::Relaxed) {
        return;
    }
    promise::spawn::spawn(async {
        let result = smol::unblock(|| -> anyhow::Result<()> {
            let Some((tool, install, source)) = updater()? else {
                return Ok(());
            };
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
            let mut command = std::process::Command::new(tool);
            command
                .args(["update", "--daily", "--stage-only"])
                .arg("--project-root")
                .arg(source)
                .env("WEZ_VTABS_INSTALL", &install);
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

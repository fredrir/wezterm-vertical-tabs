use crate::termwindow::TermWindowNotif;
use smol::io::{AsyncReadExt, AsyncWriteExt};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use vtabs_store::{Request, Response, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PROTOCOL_VERSION};
use window::WindowOps;

fn paths() -> anyhow::Result<(PathBuf, PathBuf)> {
    let helper = if let Some(path) = std::env::var_os("WEZ_VTABS_STORE") {
        PathBuf::from(path)
    } else {
        let exe = std::env::current_exe()?;
        exe.parent()
            .ok_or_else(|| anyhow::anyhow!("GUI executable directory missing"))?
            .join(if cfg!(windows) {
                "wez-vtabs-store.exe"
            } else {
                "wez-vtabs-store"
            })
    };
    let database = if let Some(path) = std::env::var_os("WEZ_VTABS_DB") {
        PathBuf::from(path)
    } else {
        dirs_next::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("local data directory missing"))?
            .join("wez-vtabs")
            .join("state.sqlite")
    };
    if let Some(parent) = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok((helper, database))
}

pub(super) async fn invoke(request: Request) -> anyhow::Result<Response> {
    let request_id = request.request_id;
    let bytes = serde_json::to_vec(&request)?;
    anyhow::ensure!(
        bytes.len() <= MAX_REQUEST_BYTES,
        "storage request too large"
    );
    let operation = async move {
        let mut child = smol::unblock(|| -> anyhow::Result<_> {
            let (helper, database) = paths()?;
            let mut command = std::process::Command::new(helper);
            command.arg("--db").arg(database);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            Ok(smol::process::Command::from(command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .reap_on_drop(true)
                .spawn()?)
        })
        .await?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("storage stdin missing"))?;
        input.write_all(&bytes).await?;
        input.close().await?;
        drop(input);
        let output = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("storage stdout missing"))?;
        let mut output = output.take((MAX_RESPONSE_BYTES + 1) as u64);
        let mut response = Vec::new();
        output.read_to_end(&mut response).await?;
        anyhow::ensure!(
            response.len() <= MAX_RESPONSE_BYTES,
            "storage response too large"
        );
        let status = child.status().await?;
        let response: Response = serde_json::from_slice(&response)?;
        anyhow::ensure!(
            response.version == PROTOCOL_VERSION && response.request_id == request_id,
            "storage response identity mismatch"
        );
        anyhow::ensure!(
            status.success() || response.error.is_some(),
            "storage helper exited {status}"
        );
        Ok(response)
    };
    smol::future::race(operation, async {
        smol::Timer::after(Duration::from_secs(3)).await;
        anyhow::bail!("storage helper timed out")
    })
    .await
}

pub fn request(window: window::Window, request: Request, mux_window_id: usize) {
    promise::spawn::spawn(async move {
        let request_id = request.request_id;
        let profiles = request
            .operations
            .iter()
            .filter_map(|operation| {
                let key = match operation {
                    vtabs_store::Operation::Put { key, .. }
                    | vtabs_store::Operation::Delete { key, .. } => key,
                    vtabs_store::Operation::Read { .. } => return None,
                };
                Some(match &key.scope {
                    vtabs_store::Scope::Profile { profile }
                    | vtabs_store::Scope::Session { profile, .. } => profile.clone(),
                })
            })
            .collect::<std::collections::BTreeSet<_>>();
        let (result, committed) = match invoke(request).await {
            Ok(response) => {
                let committed = response.error.is_none();
                (serde_json::json!({"ok": response}), committed)
            }
            Err(error) => (
                serde_json::json!({"error": error.to_string(), "request_id": request_id}),
                false,
            ),
        };
        let source = window.clone();
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.native_message_for(mux_window_id, serde_json::json!({"store": result}));
            if committed && !profiles.is_empty() {
                for gui in crate::frontend::front_end().gui_windows() {
                    if gui.window == source {
                        continue;
                    }
                    for profile in &profiles {
                        let profile = profile.clone();
                        gui.window
                            .notify(TermWindowNotif::Apply(Box::new(move |other| {
                                other.native_message(
                                    serde_json::json!({"refresh_profile": profile}),
                                );
                            })));
                    }
                }
            }
        })));
    })
    .detach();
}

//! Project requests: close policy, location, adoption, replacement and spawning beside a pane.
use anyhow::anyhow;
use codec::*;
use mux::client::ClientId;
use mux::domain::{Domain, DomainState};
use mux::pane::{CachePolicy, PaneId};
use mux::Mux;
use portable_pty::CommandBuilder;
use promise::spawn::spawn_into_main_thread;
use std::sync::Arc;

/// Commands relayed from another host keep only explicit variables, as SSH domains do.
pub(crate) fn rebase_relayed(spawn: &mut SpawnV2) {
    if !spawn.workspace.starts_with(mux::BACKING_WORKSPACE_PREFIX) {
        return;
    }
    let Some(relayed) = spawn.command.take() else {
        return;
    };
    let mut command = if relayed.is_default_prog() {
        CommandBuilder::new_default_prog()
    } else {
        CommandBuilder::from_argv(relayed.get_argv().clone())
    };
    for (key, value) in relayed.iter_extra_env_as_str() {
        command.env(key, value);
    }
    if let Some(cwd) = relayed.get_cwd() {
        command.cwd(cwd);
    }
    command.set_controlling_tty(relayed.get_controlling_tty());
    spawn.command = Some(command);
}

pub(crate) fn schedule<SND>(pdu: Pdu, send_response: SND, client_id: Option<Arc<ClientId>>)
where
    SND: Fn(anyhow::Result<Pdu>) + Send + 'static,
{
    spawn_into_main_thread(async move {
        // Within this turn, as `SendKeyDown` does, so a later key cannot overtake it.
        if let Pdu::SendKeyDownIfBusy(request) = pdu {
            return key_down_if_busy(request, send_response);
        }
        promise::spawn::spawn(async move { send_response(respond(pdu, client_id).await) }).detach();
    })
    .detach();
}

/// The owner sends the key only if closing the pane would prompt; a relay asks further along.
fn key_down_if_busy<SND>(request: SendKeyDownIfBusy, send_response: SND)
where
    SND: Fn(anyhow::Result<Pdu>) + Send + 'static,
{
    let SendKeyDownIfBusy { pane_id, event } = request;
    let answer = |sent| Pdu::SendKeyDownIfBusyResponse(SendKeyDownIfBusyResponse { sent });
    let Some(pane) = Mux::get().get_pane(pane_id) else {
        return send_response(Err(anyhow!("no such pane {pane_id}")));
    };
    if let Some(relayed) = pane.downcast_ref::<wezterm_client::pane::ClientPane>() {
        let sent = relayed.key_down_if_busy(event.key, event.modifiers);
        promise::spawn::spawn(async move { send_response(sent.await.map(answer)) }).detach();
        return;
    }
    let busy = !pane.can_close_without_prompting(mux::pane::CloseReason::Pane);
    let sent = if busy {
        pane.key_down(event.key, event.modifiers)
    } else {
        Ok(())
    };
    send_response(sent.map(|()| answer(busy)));
}

async fn respond(pdu: Pdu, client_id: Option<Arc<ClientId>>) -> anyhow::Result<Pdu> {
    let (name, adopt) = match pdu {
        Pdu::GetMuxSession(_) => {
            return Ok(Pdu::GetMuxSessionResponse(GetMuxSessionResponse {
                session: mux_session().to_owned(),
            }))
        }
        Pdu::GetPaneCloseInfo(GetPaneCloseInfo { pane_id }) => {
            return Ok(Pdu::GetPaneCloseInfoResponse(close_info(pane_id).await))
        }
        Pdu::GetPaneLocation(GetPaneLocation { pane_id }) => {
            return pane_location(pane_id)
                .await
                .map(Pdu::GetPaneLocationResponse)
        }
        Pdu::ReplacePane(request) => return replace_pane(request, client_id).await,
        Pdu::SpawnBesidePane(request) => return spawn_beside_pane(request, client_id).await,
        Pdu::FindAdoptablePanes(FindAdoptablePanes { domains }) => {
            return Ok(Pdu::FindAdoptablePanesResponse(
                FindAdoptablePanesResponse {
                    panes: wezterm_client::domain::adoption::find(&domains).await,
                },
            ))
        }
        Pdu::ListAdoptablePanes(ListAdoptablePanes { domain }) => (domain, None),
        Pdu::AdoptPane(AdoptPane {
            domain,
            remote_pane_id,
            window_id,
        }) => (domain, Some((remote_pane_id, window_id))),
        pdu => anyhow::bail!("unexpected {}", pdu.pdu_name()),
    };
    let domain = Mux::get()
        .get_domain_by_name(&name)
        .ok_or_else(|| anyhow!("no such domain {name}"))?;
    let client = domain
        .downcast_ref::<wezterm_client::domain::ClientDomain>()
        .ok_or_else(|| anyhow!("{name} is not a client domain"))?;
    if client.state() == DomainState::Detached {
        client.attach(None).await?;
    }
    Ok(match adopt {
        None => Pdu::ListAdoptablePanesResponse(ListAdoptablePanesResponse {
            panes: client.adoptable_panes().await?,
        }),
        Some((remote_pane_id, window_id)) => {
            let (tab, window_id, pane) = client.adopt_pane(remote_pane_id, window_id).await?;
            Pdu::AdoptPaneResponse(AdoptPaneResponse {
                pane_id: pane.pane_id(),
                tab_id: tab.tab_id(),
                window_id,
            })
        }
    })
}

fn mux_session() -> &'static str {
    use std::hash::{Hash, Hasher};
    static SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SESSION.get_or_init(|| {
        let mut host = std::collections::hash_map::DefaultHasher::new();
        sysinfo::System::host_name().hash(&mut host);
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("mux session clock precedes Unix epoch")
            .as_nanos();
        format!("{:016x}-{:x}-{epoch:x}", host.finish(), std::process::id())
    })
}

/// The owner's local policy, via relays; a gone pane needs no prompt, a silent owner keeps it.
async fn close_info(pane_id: PaneId) -> GetPaneCloseInfoResponse {
    let Some(pane) = Mux::get().get_pane(pane_id) else {
        return GetPaneCloseInfoResponse::default();
    };
    if let Some(relayed) = pane.downcast_ref::<wezterm_client::pane::ClientPane>() {
        return relayed
            .close_info()
            .await
            .unwrap_or(GetPaneCloseInfoResponse {
                prompt: true,
                process: String::new(),
            });
    }
    let prompt = !pane.can_close_without_prompting(mux::pane::CloseReason::Pane);
    let process = pane
        .get_foreground_process_name(CachePolicy::AllowStale)
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    GetPaneCloseInfoResponse { prompt, process }
}

/// A pane this server merely relays belongs to the machine further along the chain.
async fn pane_location(pane_id: Option<PaneId>) -> anyhow::Result<GetPaneLocationResponse> {
    let mux = Mux::get();
    let pane = pane_id
        .map(|id| mux.get_pane(id).ok_or_else(|| anyhow!("no such pane {id}")))
        .transpose()?;
    if let Some(relayed) = pane
        .as_ref()
        .and_then(|pane| pane.downcast_ref::<wezterm_client::pane::ClientPane>())
    {
        return relayed.relay_location().await;
    }
    let cwd = pane
        .and_then(|pane| pane.get_current_working_dir(CachePolicy::AllowStale))
        .map(|url| url.path().to_owned())
        .unwrap_or_default();
    Ok(GetPaneLocationResponse {
        home: mux::location::home(),
        repo_root: mux::location::repo_root(&cwd),
        cwd,
        os: sysinfo::System::distribution_id(),
        hostname: sysinfo::System::host_name().unwrap_or_default(),
    })
}

async fn replace_pane(
    request: ReplacePane,
    client_id: Option<Arc<ClientId>>,
) -> anyhow::Result<Pdu> {
    let mux = Mux::get();
    let _identity = mux.with_identity(client_id);
    let (_domain, window_id, tab_id) = mux
        .resolve_pane_id(request.pane_id)
        .ok_or_else(|| anyhow!("pane_id {} invalid", request.pane_id))?;
    let (pane, size) = mux
        .replace_pane(
            request.pane_id,
            request.command,
            request.command_dir,
            request.domain,
            request.wait_for_ready,
        )
        .await?;
    Ok(Pdu::SpawnResponse(SpawnResponse {
        pane_id: pane.pane_id(),
        tab_id,
        window_id,
        size,
    }))
}

/// The pane's own domain spawns, so a relayed pane's program runs on its real host.
async fn spawn_beside_pane(
    request: SpawnBesidePane,
    client_id: Option<Arc<ClientId>>,
) -> anyhow::Result<Pdu> {
    let mux = Mux::get();
    let _identity = mux.with_identity(client_id);
    let (tab, pane, window_id) = mux
        .spawn_tab_or_window(
            None,
            config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
            request.command,
            request.command_dir,
            request.size,
            Some(request.pane_id),
            request.workspace,
            None,
        )
        .await?;
    Ok(Pdu::SpawnResponse(SpawnResponse {
        pane_id: pane.pane_id(),
        tab_id: tab.tab_id(),
        window_id,
        size: tab.get_size(),
    }))
}

//! Requests served for this project's clients: close policy, location, adoption and replacement.
use anyhow::anyhow;
use codec::*;
use mux::client::ClientId;
use mux::domain::{Domain, DomainState};
use mux::pane::{CachePolicy, PaneId};
use mux::Mux;
use promise::spawn::spawn_into_main_thread;
use std::sync::Arc;

pub(crate) fn schedule<SND>(pdu: Pdu, send_response: SND, client_id: Option<Arc<ClientId>>)
where
    SND: Fn(anyhow::Result<Pdu>) + Send + 'static,
{
    spawn_into_main_thread(async move {
        promise::spawn::spawn(async move { send_response(respond(pdu, client_id).await) }).detach();
    })
    .detach();
}

async fn respond(pdu: Pdu, client_id: Option<Arc<ClientId>>) -> anyhow::Result<Pdu> {
    let (name, adopt) = match pdu {
        Pdu::GetPaneCloseInfo(GetPaneCloseInfo { pane_id }) => {
            return Ok(Pdu::GetPaneCloseInfoResponse(close_info(pane_id).await))
        }
        Pdu::GetPaneLocation(GetPaneLocation { pane_id }) => {
            return pane_location(pane_id)
                .await
                .map(Pdu::GetPaneLocationResponse)
        }
        Pdu::ReplacePane(request) => return replace_pane(request, client_id).await,
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

/// The pane's owner applies the ordinary local policy; a relay asks further along. A pane
/// that is gone needs no prompt, and an owner that cannot answer keeps the prompt.
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

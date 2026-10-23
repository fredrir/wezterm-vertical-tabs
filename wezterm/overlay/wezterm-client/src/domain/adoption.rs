//! Unadopted remote panes, found and adopted by the mux that owns a window's layout.
use crate::domain::{ClientDomain, ClientInner};
use anyhow::anyhow;
use codec::{AdoptPane, AdoptablePane, FindAdoptablePanes, GetPaneLocation};
use mux::domain::{Domain, DomainState};
use mux::pane::PaneId;
use mux::tab::TabId;
use mux::window::WindowId;
use mux::Mux;
use std::collections::HashSet;
use std::sync::Arc;

/// Asks the mux `window` mirrors, or this process when it owns the window's layout.
pub async fn find_for(
    window: WindowId,
    domains: Vec<String>,
) -> anyhow::Result<Vec<AdoptablePane>> {
    match mirror(window) {
        Some(inner) => Ok(inner
            .client
            .find_adoptable_panes(FindAdoptablePanes { domains })
            .await?
            .panes),
        None => Ok(find(&domains).await),
    }
}

/// Shows the pane in a new tab of `window` and returns that tab.
pub async fn adopt_into(
    window: WindowId,
    domain: String,
    remote_pane_id: PaneId,
) -> anyhow::Result<TabId> {
    let Some(inner) = mirror(window) else {
        let domain = Mux::get()
            .get_domain_by_name(&domain)
            .ok_or_else(|| anyhow!("no such domain {domain}"))?;
        let client = domain
            .downcast_ref::<ClientDomain>()
            .ok_or_else(|| anyhow!("{} is not a client domain", domain.domain_name()))?;
        let (tab, _, _) = client.adopt_pane(remote_pane_id, Some(window)).await?;
        return Ok(tab.tab_id());
    };
    let window_id = inner
        .local_to_remote_window(window)
        .ok_or_else(|| anyhow!("window {window} is not mirrored"))?;
    let adopted = inner
        .client
        .adopt_pane(AdoptPane {
            domain,
            remote_pane_id,
            window_id: Some(window_id),
        })
        .await?;
    ClientDomain::resync_inner(Arc::clone(&inner)).await?;
    inner
        .remote_to_local_tab_id(adopted.tab_id)
        .ok_or_else(|| anyhow!("adopted tab {} is not mirrored", adopted.tab_id))
}

/// Remote panes this process could show, once per server: `domains` first, then connected ones.
pub async fn find(domains: &[String]) -> Vec<AdoptablePane> {
    let mux = Mux::get();
    let named: Vec<_> = domains
        .iter()
        .filter_map(|name| mux.get_domain_by_name(name))
        .collect();
    for domain in &named {
        if layout(domain).is_some() && domain.state() == DomainState::Detached {
            if let Err(err) = domain.attach(None).await {
                log::warn!("{}: {err:#}", domain.domain_name());
            }
        }
    }
    let (mut ids, mut servers) = (HashSet::new(), HashSet::new());
    let reachable: Vec<_> = named
        .into_iter()
        .chain(mux.iter_domains())
        .filter(|domain| ids.insert(domain.domain_id()))
        .filter(|domain| {
            layout(domain)
                .and_then(ClientDomain::inner)
                .is_some_and(|inner| inner.server().is_none_or(|server| servers.insert(server)))
        })
        .collect();
    let found = futures::future::join_all(reachable.iter().map(located)).await;
    found
        .into_iter()
        .zip(&reachable)
        .flat_map(|(panes, domain)| {
            panes.unwrap_or_else(|err| {
                log::warn!("{}: {err:#}", domain.domain_name());
                vec![]
            })
        })
        .collect()
}

fn layout(domain: &Arc<dyn Domain>) -> Option<&ClientDomain> {
    domain
        .downcast_ref::<ClientDomain>()
        .filter(|client| client.local_pane_layout())
}

async fn located(domain: &Arc<dyn Domain>) -> anyhow::Result<Vec<AdoptablePane>> {
    let client = layout(domain).ok_or_else(|| anyhow!("not a local_pane_layout domain"))?;
    let inner = client
        .inner()
        .ok_or_else(|| anyhow!("domain is not attached"))?;
    let entries = client.adoptable_panes().await?;
    let locations = futures::future::join_all(entries.iter().map(|entry| {
        inner.client.get_pane_location(GetPaneLocation {
            pane_id: Some(entry.pane_id),
        })
    }))
    .await;
    Ok(entries
        .into_iter()
        .zip(locations)
        .map(|(entry, location)| AdoptablePane {
            domain: client.domain_name().to_owned(),
            entry,
            location: location.ok(),
        })
        .collect())
}

/// The client domain whose mux owns `window`'s layout; `None` when this process does.
fn mirror(window: WindowId) -> Option<Arc<ClientInner>> {
    let mux = Mux::get();
    let tab = mux.get_active_tab_for_window(window)?;
    let domain = mux.get_domain(tab.get_active_pane()?.domain_id())?;
    let client = domain.downcast_ref::<ClientDomain>()?;
    if client.local_pane_layout() {
        return None;
    }
    client.inner()
}

//! Unadopted remote panes for the tab search, asked of the mux that owns the window's layout.
use crate::termwindow::TermWindowNotif;
use serde_json::{Value, json};
use vtabs_app::{core, ui};
use window::{Window, WindowOps};

/// Answers with an `adoptable` message naming `search`, so a later search drops it.
pub fn find(window: Window, mux_window: usize, search: u64) {
    promise::spawn::spawn(async move {
        let domains = super::lua::adoptable_domains().await;
        let panes = match wezterm_client::domain::adoption::find_for(mux_window, domains).await {
            Ok(panes) if panes.is_empty() => return,
            Ok(panes) => panes,
            Err(error) => return log::warn!("tabs adoptable: {error:#}"),
        };
        let panes = match serde_json::to_value(panes) {
            Ok(panes) => panes,
            Err(error) => return log::warn!("tabs adoptable: {error:#}"),
        };
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.vtabs_message_for(
                mux_window,
                json!({ "adoptable": { "search": search, "panes": panes } }),
            );
        })));
    })
    .detach();
}

pub fn entries(panes: Value, home: Option<&str>) -> anyhow::Result<Vec<ui::AdoptablePane>> {
    let panes: Vec<codec::AdoptablePane> = serde_json::from_value(panes)?;
    Ok(panes.into_iter().map(|found| entry(found, home)).collect())
}

/// The owner's location labels the pane like a tab of this window.
fn entry(found: codec::AdoptablePane, home: Option<&str>) -> ui::AdoptablePane {
    let location = found.location.unwrap_or_default();
    let place = location
        .hostname
        .split('.')
        .next()
        .filter(|host| !host.is_empty())
        .unwrap_or(&found.domain)
        .to_owned();
    let cwd = if location.cwd.is_empty() {
        found
            .entry
            .working_dir
            .map(|dir| dir.url.path().to_owned())
            .unwrap_or_default()
    } else {
        location.cwd
    };
    let tab = core::Tab {
        title: found.entry.title,
        cwd,
        home: (!location.home.is_empty()).then_some(location.home),
        repo_root: location.repo_root,
        os: location.os,
        remote: true,
        ..Default::default()
    };
    ui::AdoptablePane::new(found.domain, found.entry.pane_id as u64, &tab, home, place)
}

/// Shows the adopted pane's new tab in `mux_window`.
pub async fn adopt(
    window: Option<Window>,
    mux_window: usize,
    domain: String,
    pane: u64,
) -> anyhow::Result<()> {
    let tab =
        wezterm_client::domain::adoption::adopt_into(mux_window, domain, pane as mux::pane::PaneId)
            .await?;
    if let Some(window) = window {
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.vtabs_message_for(mux_window, json!({ "show_tab": tab }));
        })));
    }
    Ok(())
}

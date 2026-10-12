mod keybinds;
pub(crate) mod launcher;
pub(crate) mod settings_page;
pub(crate) mod sidebar;

use crate::icons;
use vtabs_core::{Tab, TabTitle};

pub(crate) fn spans_machines(tab: &Tab) -> bool {
    let mut glyphs = tab
        .panes
        .iter()
        .map(|pane| icons::host(pane.remote, &pane.os));
    glyphs
        .next()
        .is_some_and(|first| glyphs.any(|glyph| glyph != first))
}

pub(crate) fn tab_machine(tab: &Tab) -> Option<(bool, &str)> {
    if spans_machines(tab) {
        return None;
    }
    Some(
        tab.panes
            .iter()
            .find(|pane| pane.active)
            .map_or((tab.remote, &tab.os), |pane| (pane.remote, &pane.os)),
    )
}

pub(crate) fn tab_name(tab: &Tab, home: Option<&str>, source: TabTitle) -> Option<String> {
    if let Some(title) = tab.custom_title() {
        return Some(title.to_owned());
    }
    let location = tab.location(home);
    let process = tab
        .process
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty());
    match source {
        TabTitle::Directory => location,
        TabTitle::Process => process.map(str::to_owned).or(location),
        TabTitle::PaneTitle => Some(tab.title.clone())
            .filter(|title| !title.is_empty())
            .or(location),
        TabTitle::ProcessAndDirectory => match (process, location) {
            (Some(process), Some(location)) => Some(format!("{process} {location}")),
            (process, location) => process.map(str::to_owned).or(location),
        },
    }
}

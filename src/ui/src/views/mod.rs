//! Screens built from components: the sidebar, the settings page and the launchers.
pub(crate) mod launcher;
pub(crate) mod settings_page;
pub(crate) mod sidebar;

use crate::icons;
use vtabs_core::Tab;

pub(crate) fn spans_machines(tab: &Tab) -> bool {
    let mut glyphs = tab
        .panes
        .iter()
        .map(|pane| icons::host(pane.remote, &pane.os));
    glyphs
        .next()
        .is_some_and(|first| glyphs.any(|glyph| glyph != first))
}

/// Splits spanning several machines read as an unknown remote rather than following focus.
pub(crate) fn tab_machine(tab: &Tab) -> (bool, &str) {
    if spans_machines(tab) {
        return (true, "");
    }
    tab.panes
        .iter()
        .find(|pane| pane.active)
        .map_or((tab.remote, &tab.os), |pane| (pane.remote, &pane.os))
}

pub(crate) fn tab_name(tab: &Tab, home: Option<&str>) -> Option<String> {
    tab.custom_title()
        .map(str::to_owned)
        .or_else(|| tab.location(home))
}

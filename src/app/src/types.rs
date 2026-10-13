use crate::core::{HostCommand, HostTab, PaneId, TabId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub dpi: f32,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            cols: 32,
            rows: 40,
            pixel_width: 256,
            pixel_height: 640,
            cell_width: 8.,
            cell_height: 16.,
            dpi: 96.,
        }
    }
}

/// Revision belongs to the host, and covers topology, active ID, geometry and configuration.
#[derive(Clone, Debug)]
pub struct HostSnapshot {
    pub revision: u64,
    pub tabs: Vec<HostTab>,
    pub active_tab: Option<TabId>,
    pub metrics: Metrics,
    pub focused: bool,
    pub configuration_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    OpenCommands,
    RunCommand(usize),
    QuickTerminal,
    Refresh,
    Host(HostCommand),
    SetClipboard(String),
    RequestClipboard,
    /// Close directly when the tab is idle; otherwise call `SidebarUi::confirm_close_tab`.
    ConfirmClose(TabId),
    /// The same for one split, answered with `SidebarUi::confirm_close_pane`.
    ConfirmClosePane(TabId, PaneId),
    /// A tab another window owns, or one this window lost to a detached domain.
    ShowTab {
        window: u64,
        tab: TabId,
    },
    /// Lists foreign tabs and unadopted panes, then opens the tab search.
    SearchTabs,
    /// A remote pane no mux shows, adopted into this window.
    AdoptPane {
        domain: String,
        pane: u64,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Update {
    pub commands: Vec<Command>,
}
impl Update {
    pub(crate) fn merge(&mut self, other: Self) {
        self.commands.extend(other.commands);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Projection<'a> {
    pub visible: &'a [TabId],
    pub active: Option<TabId>,
}

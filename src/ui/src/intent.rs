use vtabs_core::{Intent, PaneId, TabId};

#[derive(Clone, Debug)]
pub enum HostAction {
    OpenJobs,
    Job(vtabs_core::jobs::JobTarget, vtabs_core::jobs::JobOperation),
    Custom(String),
    MoveTabToNewWindow(TabId),
    CloseTab(TabId),
    FocusPane(TabId, PaneId),
    ClosePane(TabId, PaneId),
    KillPane(TabId, PaneId),
    DetachPane {
        tab: TabId,
        pane: PaneId,
        index: Option<usize>,
    },
    JoinPane {
        pane: PaneId,
        tab: TabId,
    },
    JoinTab {
        source: TabId,
        tab: TabId,
    },
    ShowTab {
        window: u64,
        tab: TabId,
    },
}

#[derive(Clone, Debug)]
pub enum UiIntent {
    Refresh,
    Domain(Intent),
    SetClipboard(String),
    RequestClipboard,
    Host(HostAction),
}

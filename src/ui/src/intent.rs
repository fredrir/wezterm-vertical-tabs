use vtabs_core::{Intent, PaneId, TabId};

#[derive(Clone, Debug)]
pub enum HostAction {
    OpenJobs,
    Job(vtabs_core::jobs::JobTarget, vtabs_core::jobs::JobOperation),
    /// Custom menu actions remain semantic; the adapter resolves a registered Lua action.
    Custom(String),
    MoveTabToNewWindow(TabId),
    /// The host closes an idle tab directly and asks `confirm_close_tab` for a busy one.
    CloseTab(TabId),
    FocusPane(TabId, PaneId),
    /// The host closes an idle split directly and asks `confirm_close_pane` for a busy one.
    ClosePane(TabId, PaneId),
    KillPane(TabId, PaneId),
    /// A pane dragged out of its tab becomes a tab of its own, optionally at an index.
    DetachPane {
        tab: TabId,
        pane: PaneId,
        index: Option<usize>,
    },
    /// A pane dropped inside another tab becomes one of its splits.
    JoinPane {
        pane: PaneId,
        tab: TabId,
    },
    /// A tab dropped inside another tab hands over every pane it has.
    JoinTab {
        source: TabId,
        tab: TabId,
    },
    /// The host asks the owning window to show a tab, reattaching its domain if needed.
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

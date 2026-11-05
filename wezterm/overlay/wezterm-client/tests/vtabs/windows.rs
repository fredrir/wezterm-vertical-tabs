use super::*;
use mux::tab::{PaneEntry, PaneNode};

pub(super) static MUX_TEST_LOCK: Mutex<()> = Mutex::new(());

fn snapshot() -> ListPanesResponse {
    ListPanesResponse {
        tabs: vec![PaneNode::Leaf(PaneEntry {
            window_id: 101,
            tab_id: 102,
            pane_id: 103,
            title: "dogit".into(),
            size: TerminalSize {
                rows: 24,
                cols: 80,
                ..TerminalSize::default()
            },
            working_dir: None,
            is_active_pane: true,
            is_zoomed_pane: false,
            workspace: "quick-terminal".into(),
            cursor_pos: Default::default(),
            physical_top: 0,
            top_row: 0,
            left_col: 0,
            tty_name: None,
        })],
        tab_titles: vec!["dogit".into()],
        window_titles: [(101, "Quick dogit".into())].into(),
    }
}

#[test]
fn pane_list_recovers_missing_windows() {
    let _serial = MUX_TEST_LOCK.lock().unwrap();
    // No RPC or scheduled pruning is needed to apply an already received snapshot.
    promise::spawn::set_schedulers(Box::new(drop), Box::new(drop));
    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            Mux::shutdown();
        }
    }
    let _cleanup = Cleanup;
    let inner = Arc::new(ClientInner::new(
        alloc_domain_id(),
        Client::for_test(),
        None,
        false,
        false,
        None,
    ));
    let window = mux.new_empty_window(Some("quick-terminal".into()), None);
    let original = *window;
    drop(window);
    inner.record_remote_to_local_window_mapping(101, original);

    ClientDomain::process_pane_list(Arc::clone(&inner), snapshot(), None, HashMap::new()).unwrap();
    assert_eq!(inner.remote_to_local_window(101), Some(original));
    let tab = inner.remote_to_local_tab_id(102).unwrap();
    let pane = inner.remote_to_local_pane_id(103).unwrap();

    // Ordinary refreshes retain both identities and do not duplicate the tab.
    ClientDomain::process_pane_list(Arc::clone(&inner), snapshot(), None, HashMap::new()).unwrap();
    assert_eq!(inner.remote_to_local_tab_id(102), Some(tab));
    assert_eq!(inner.remote_to_local_pane_id(103), Some(pane));
    assert_eq!(mux.get_window(original).unwrap().iter_tabs().count(), 1);

    // A snapshot can arrive after its mapped window and attach target were removed.
    mux.kill_window(original);
    assert!(mux.get_window(original).is_none());
    ClientDomain::process_pane_list(
        Arc::clone(&inner),
        snapshot(),
        Some(original),
        HashMap::new(),
    )
    .unwrap();
    let replacement = inner.remote_to_local_window(101).unwrap();
    assert_ne!(replacement, original);
    let window = mux.get_window(replacement).unwrap();
    assert_eq!(window.get_workspace(), "quick-terminal");
    assert_eq!(window.get_title(), "Quick dogit");
    assert_eq!(window.iter_tabs().count(), 1);
    drop(window);

    // Title-only entries for a removed window must not abort the rest of the sync.
    inner.record_remote_to_local_window_mapping(104, original);
    let mut panes = snapshot();
    panes.window_titles.insert(104, "Closed window".into());
    ClientDomain::process_pane_list(Arc::clone(&inner), panes, None, HashMap::new()).unwrap();
    assert_eq!(inner.remote_to_local_window(104), None);
    assert_eq!(inner.remote_to_local_window(101), Some(replacement));
}

use super::*;
use crate::termwindow::ui_host::TabInfo;

fn snapshot(revision: u64, config_epoch: usize, process: &str) -> Snapshot {
    Snapshot {
        revision,
        config_epoch,
        window_id: 1,
        active: Some(1),
        focused: true,
        tabs: vec![TabInfo {
            id: 1,
            title: format!("Tab: {process}"),
            process: process.into(),
            cwd: "/project".into(),
            domain: "local".into(),
            host: String::new(),
            user: String::new(),
            unread: false,
            bell: false,
            remote: false,
            user_vars: HashMap::new(),
        }],
    }
}

#[test]
fn hook_metadata_is_optional_and_survives_reconfiguration_and_retry() {
    config::designate_this_as_the_main_thread();
    let mux = std::sync::Arc::new(mux::Mux::new(None));
    mux::Mux::set_mux(&mux);
    let mut adapter = Adapter::new(1);
    adapter.config_generation = 0;
    adapter
        .app
        .config(serde_json::json!({"templates":[{
            "id":"process-$process", "name":"$process",
            "rules":[{"fields":[["process",["*"]]]}]
        }]}))
        .unwrap();

    for (revision, process) in [(1, "shell"), (2, "editor")] {
        adapter.snapshot(snapshot(revision, 0, process));
        let tab = &adapter.app.model().tabs[&1];
        assert_eq!(tab.process, process);
        assert_eq!(tab.space_id, format!("process-{process}"));
        assert!(adapter.hook_metadata.is_empty());
        assert!(adapter.hook_queued.is_empty());
    }

    // Enabling hooks must send current metadata even when the host facts have
    // not changed since a snapshot that ran with callbacks disabled.
    adapter.hooks_enabled = true;
    adapter.snapshot(snapshot(3, 0, "editor"));
    assert_eq!(adapter.hook_metadata[&1].process, "editor");
    assert_eq!(adapter.hook_queued[&1].process, "editor");
    let stale = adapter.app.hook_token(Some(1));
    adapter.hook_queued.clear();
    adapter.hook_pending = true;
    adapter.snapshot(snapshot(4, 0, "builder"));
    assert_eq!(adapter.hook_metadata[&1].process, "builder");
    assert_eq!(adapter.hook_queued[&1].process, "builder");
    adapter.hook_queued.clear();
    adapter.message(serde_json::json!({"hooks":[{
        "token":stale, "result":app::HookResult::Title("stale".into())
    }]}));
    assert_eq!(adapter.hook_queued[&1].process, "builder");
    assert_eq!(adapter.app.model().tabs[&1].display_title(), "Tab: builder");

    // A real configuration epoch change reads the default callback-free Lua
    // configuration, releasing both retained metadata and queued retry work.
    adapter.snapshot(snapshot(5, 1, "builder"));
    assert!(!adapter.hooks_enabled);
    assert!(adapter.hook_metadata.is_empty());
    assert!(adapter.hook_queued.is_empty());
    adapter.hooks_enabled = true;
    adapter.snapshot(snapshot(6, 1, "builder"));
    assert_eq!(adapter.hook_metadata[&1].process, "builder");
    assert_eq!(adapter.hook_queued[&1].process, "builder");
    mux::Mux::shutdown();
}

use super::*;
use facts::{Facts, TabInfo};

fn snapshot(revision: u64, config_epoch: usize, process: &str) -> Facts {
    Facts {
        revision,
        config_epoch,
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
        adapter.apply_facts(snapshot(revision, 0, process));
        let tab = &adapter.app.model().tabs[&1];
        assert_eq!(tab.process, process);
        assert_eq!(tab.space_id, format!("process-{process}"));
        assert!(adapter.hooks.take_batch(&adapter.app).is_none());
    }

    // Enabling hooks sends current metadata even when host facts are unchanged.
    adapter.hooks.configure(true);
    adapter.apply_facts(snapshot(3, 0, "editor"));
    let batch = adapter.hooks.take_batch(&adapter.app).unwrap();
    assert_eq!(batch.tabs.len(), 1);
    assert_eq!(batch.tabs[0].1.process, "editor");
    let stale = batch.tabs[0].0;
    adapter.apply_facts(snapshot(4, 0, "builder"));
    assert!(adapter.hooks.take_batch(&adapter.app).is_none());
    adapter.message(Message::Hooks(vec![lua::HookCompletion {
        token: stale,
        result: app::HookResult::Title("stale".into()),
    }]));
    let retry = adapter.hooks.take_batch(&adapter.app).unwrap();
    assert_eq!(retry.tabs.len(), 1);
    assert_eq!(retry.tabs[0].1.process, "builder");
    assert_eq!(adapter.app.model().tabs[&1].display_title(), "Tab: builder");

    // Reconfiguration drops old work; an in-flight callback keeps its claim until it completes.
    adapter.apply_facts(snapshot(5, 1, "builder"));
    assert!(!adapter.hooks.enabled());
    assert!(adapter.hooks.take_batch(&adapter.app).is_none());
    adapter.hooks.configure(true);
    adapter.apply_facts(snapshot(6, 1, "builder"));
    assert!(adapter.hooks.take_batch(&adapter.app).is_none());
    adapter.message(Message::Hooks(Vec::new()));
    let batch = adapter.hooks.take_batch(&adapter.app).unwrap();
    assert_eq!(batch.tabs.len(), 1);
    assert_eq!(batch.tabs[0].1.process, "builder");
    adapter.message(Message::Hooks(Vec::new()));
    assert!(adapter.hooks.take_batch(&adapter.app).is_none());
    mux::Mux::shutdown();
}

#[test]
fn hook_batches_are_bounded_and_drop_removed_tabs_before_dispatch() {
    let mut app = WindowApp::default();
    let tabs = (1..=lua::MAX_HOOK_TABS + 2)
        .map(|id| core::HostTab {
            id: id as u64,
            ..Default::default()
        })
        .collect::<Vec<_>>();
    app.update(app::HostSnapshot {
        revision: 1,
        tabs: tabs.clone(),
        active_tab: Some(1),
        metrics: app::Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
    let mut hooks = hooks::Hooks::default();
    hooks.configure(true);
    hooks.observe(&tabs);
    let first = hooks.take_batch(&app).unwrap();
    assert_eq!(first.tabs.len(), lua::MAX_HOOK_TABS);
    assert_eq!(first.tabs[0].1.id, 1);
    assert_eq!(first.tabs.last().unwrap().1.id, lua::MAX_HOOK_TABS as u64);
    assert!(hooks.take_batch(&app).is_none());
    let remaining = &tabs[..tabs.len() - 1];
    app.update(app::HostSnapshot {
        revision: 2,
        tabs: remaining.to_vec(),
        active_tab: Some(1),
        metrics: app::Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
    hooks.observe(remaining);
    hooks.complete(true, false);
    let second = hooks.take_batch(&app).unwrap();
    assert_eq!(second.tabs.len(), 1);
    assert_eq!(second.tabs[0].1.id, (lua::MAX_HOOK_TABS + 1) as u64);
    hooks.complete(true, false);
    assert!(hooks.take_batch(&app).is_none());
}

#[test]
fn host_fact_batches_keep_the_full_tab_wire_contract() {
    let facts = core::HostTab {
        id: 7,
        title: "Host title".into(),
        cwd: "/work".into(),
        remote: true,
        ..Default::default()
    };
    let mut app = WindowApp::default();
    app.update(app::HostSnapshot {
        revision: 1,
        tabs: vec![facts.clone()],
        active_tab: Some(7),
        metrics: app::Metrics::default(),
        focused: true,
        configuration_epoch: 0,
    })
    .unwrap();
    app.dispatch(core::Intent::PinTab {
        id: 7,
        pinned: true,
    })
    .unwrap();
    app.dispatch(core::Intent::RenameTab {
        id: 7,
        title: "App title".into(),
    })
    .unwrap();
    let mut hooks = hooks::Hooks::default();
    hooks.configure(true);
    hooks.observe(&[facts]);
    let batch = hooks.take_batch(&app).unwrap();
    assert_eq!(batch.tabs.len(), 1);
    assert_eq!(
        serde_json::to_value(&batch.tabs[0].1).unwrap(),
        serde_json::json!({
            "id": 7, "title": "Host title", "icon": "", "cwd": "/work", "repo_root": null,
            "home": null, "domain": "", "host": "", "user": "", "process": "", "remote": true,
            "os": "", "panes": [], "unread": false, "bell": false, "pinned": false,
            "folder_id": null, "space_id": "", "manual_assignment": false, "launch": null,
            "title_override": null, "title_hook": null,
        })
    );
}

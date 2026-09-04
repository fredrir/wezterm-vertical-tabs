use serde_json::json;
use vtabs_protocol::payload::{Scheme, SpaceRouteHookAnswer, SpacesMsg};

use super::{Plan, fingerprint, glob_matches, plan};

fn msg(value: serde_json::Value) -> SpacesMsg {
    serde_json::from_value(value).unwrap()
}

fn scheme() -> Scheme {
    serde_json::from_value(json!({
        "ansi": ["#000000", "#110000", "#220000", "#330000", "#440000", "#550000", "#660000"],
        "brights": ["#777777", "#880000", "#990000", "#aa0000", "#bb0000", "#cc0000", "#dd0000"]
    }))
    .unwrap()
}

fn resolved(
    msg: &SpacesMsg,
    answers: Option<&[SpaceRouteHookAnswer]>,
) -> vtabs_protocol::payload::SpaceResolution {
    match plan(msg, &scheme(), answers) {
        Plan::Resolved(result) => *result,
        Plan::NeedsHooks { .. } => panic!("unexpected hook request"),
    }
}

#[test]
fn rules_are_anchored_lists_are_any_of_and_cwd_without_star_is_a_prefix() {
    assert!(glob_matches("*a", "aba"));
    assert!(glob_matches("a*a", "aba"));
    assert!(!glob_matches("a*b", "abx"));
    let msg = msg(json!({
        "window_id": 7, "enabled": true,
        "definitions": [
            {"id":"work", "match":{"domain":["tls:*","ssh:*"], "cwd":"~/work"}},
            {"id":"claude", "match":{"proc":"cla*"}},
            {"id":"home"}
        ],
        "tabs": [
            {"id":1,"index":1,"title":"a","domain":"ssh:pi","cwd":"~/work/x","proc":"zsh"},
            {"id":2,"index":2,"title":"b","domain":"local","cwd":"~/workshop","proc":"xclaude"},
            {"id":3,"index":3,"title":"c","domain":"local","cwd":"~","proc":"claude"}
        ], "active_tab": 1
    }));
    let result = resolved(&msg, None);
    assert_eq!(result.assignments[0].space.as_deref(), Some("work"));
    assert_eq!(
        result.assignments[1].space.as_deref(),
        Some("work"),
        "falls into active/default after both anchored rules miss"
    );
    assert_eq!(result.assignments[2].space.as_deref(), Some("claude"));
}

#[test]
fn templates_are_utf8_safe_capped_and_skip_missing_facts() {
    let msg = msg(json!({
        "window_id":1,"enabled":true,
        "definitions":[
            {"id":"home"},
            {"id":"$host", "name":"Remote $host", "match":{"remote":true}}
        ],
        "tabs":[
            {"id":1,"index":1,"title":"one","remote":true,"host":"éééééééééééééééééééééééééééééé"},
            {"id":2,"index":2,"title":"two","remote":true}
        ], "active_tab":1
    }));
    let result = resolved(&msg, None);
    let id = result.assignments[0].space.as_ref().unwrap();
    assert!(id.len() <= 48);
    assert!(id.is_char_boundary(id.len()));
    assert_eq!(result.assignments[1].space.as_deref(), Some("home"));
}

#[test]
fn hook_is_batched_only_for_changed_automatic_tabs_and_wins_before_rules() {
    let msg = msg(json!({
        "window_id":9,"enabled":true,"hook":true,
        "definitions":[{"id":"home"},{"id":"rules","match":{"proc":"cargo"}}],
        "tabs":[
            {"id":1,"index":1,"title":"one","proc":"zsh","space":"home","fingerprint":"stale"},
            {"id":2,"index":2,"title":"two","proc":"cargo","space":"home","manual":true},
            {"id":3,"index":3,"title":"three","proc":"cargo"}
        ], "active_tab":1
    }));
    let requested = match plan(&msg, &scheme(), None) {
        Plan::NeedsHooks { tabs } => tabs,
        Plan::Resolved(_) => panic!("hook batch expected"),
    };
    assert_eq!(
        requested.iter().map(|tab| tab.tab_id).collect::<Vec<_>>(),
        [1, 3]
    );
    let answers = [
        SpaceRouteHookAnswer {
            tab_id: 1,
            space: Some("hooked".into()),
        },
        SpaceRouteHookAnswer {
            tab_id: 3,
            space: None,
        },
    ];
    let result = resolved(&msg, Some(&answers));
    assert_eq!(result.assignments[0].space.as_deref(), Some("hooked"));
    assert_eq!(result.assignments[1].space.as_deref(), Some("home"));
    assert_eq!(result.assignments[2].space.as_deref(), Some("rules"));

    let mut next = msg;
    for (tab, assignment) in next.tabs.iter_mut().zip(&result.assignments) {
        tab.space.clone_from(&assignment.space);
        tab.fingerprint.clone_from(&assignment.fingerprint);
    }
    assert!(matches!(plan(&next, &scheme(), None), Plan::Resolved(_)));
}

#[test]
fn hook_fingerprint_covers_every_fact_the_callback_can_observe() {
    let base = msg(json!({
        "window_id":9,"enabled":true,"hook":true,
        "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host",
            "user":"user","domain":"local","remote":false,"space":"home","manual":false}]
    }));
    let stamp = fingerprint(&base, &base.tabs[0], Some("home"), false);
    let changed = |value: serde_json::Value| {
        let changed = msg(value);
        fingerprint(
            &changed,
            &changed.tabs[0],
            changed.tabs[0].space.as_deref(),
            changed.tabs[0].manual,
        )
    };
    for value in [
        json!({"window_id":10,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":2,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"two","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"fish","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"/tmp","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"other","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"other","domain":"local","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"ssh","remote":false,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":true,"space":"home","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"work","manual":false}]}),
        json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":true}]}),
    ] {
        assert_ne!(changed(value), stamp);
    }
}

#[test]
fn sticky_assignment_holds_on_no_match_but_moves_into_a_new_match() {
    let mut msg = msg(json!({
        "window_id":1,"enabled":true,
        "definitions":[{"id":"home"},{"id":"claude","match":{"proc":"claude"}}],
        "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","space":"home","fingerprint":"old"}],
        "active_tab":1,"active_space":"home"
    }));
    let first = resolved(&msg, None);
    assert_eq!(first.assignments[0].space.as_deref(), Some("home"));
    msg.tabs[0].tab.proc = Some("claude".into());
    msg.tabs[0]
        .fingerprint
        .clone_from(&first.assignments[0].fingerprint);
    let second = resolved(&msg, None);
    assert_eq!(second.assignments[0].space.as_deref(), Some("claude"));
}

#[test]
fn active_follow_is_a_trigger_and_empty_manual_switch_holds() {
    let base = json!({
        "window_id":1,"enabled":true,
        "definitions":[{"id":"home"},{"id":"scratch"}],
        "tabs":[{"id":1,"index":1,"title":"one","space":"home","fingerprint":"same"}],
        "active_tab":1,"follow":{"tab_id":1,"space":"home"}
    });
    let mut empty = base.clone();
    empty["active_space"] = json!("scratch");
    let held = resolved(&msg(empty), None);
    assert_eq!(held.active.as_deref(), Some("scratch"));

    let mut external = base;
    external["active_space"] = json!("scratch");
    external["follow"] = json!({"tab_id":2,"space":"scratch"});
    let followed = resolved(&msg(external), None);
    assert_eq!(followed.active.as_deref(), Some("home"));
}

#[test]
fn summary_keeps_static_order_then_dynamic_first_sight_and_settings_are_global() {
    let msg = msg(json!({
        "window_id":1,"enabled":true,
        "definitions":[{"id":"home"},{"id":"$host","icon":"R","match":{"remote":true}}],
        "dynamics":[{"id":"old","name":"Old","seq":1},{"id":"empty","name":"Empty","seq":2}],
        "tabs":[
            {"id":1,"index":1,"title":"one","space":"old","fingerprint":"same","unseen":true},
            {"id":2,"index":2,"title":"settings","settings":true},
            {"id":3,"index":3,"title":"three","remote":true,"host":"pi"}
        ], "active_tab":1,"active_space":"old"
    }));
    let result = resolved(&msg, None);
    assert_eq!(
        result
            .summary
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["home", "old", "pi"]
    );
    assert_eq!(result.summary[1].count, 1);
    assert!(result.summary[1].unseen);
    assert_eq!(result.summary[2].icon.as_deref(), Some("R"));
    assert!(result.visible_tab_ids.contains(&2));
    assert!(result.dynamics.iter().all(|space| space.id != "empty"));
}

#[test]
fn disabling_spaces_clears_the_authoritative_dynamic_ledger() {
    let msg = msg(json!({
        "window_id":1,"enabled":false,
        "dynamics":[{"id":"stale","name":"Stale","seq":1}],
        "tabs":[{"id":1,"index":1,"title":"one","space":"stale"}],
        "active_tab":1,"active_space":"stale"
    }));
    let result = resolved(&msg, None);
    assert!(result.dynamics.is_empty());
    assert_eq!(result.active, None);
    assert_eq!(result.visible_tab_ids, vec![1]);
}

#[test]
fn admission_refuses_only_the_dynamic_id_past_the_per_window_cap() {
    let mut definitions = (0..31)
        .map(|at| json!({"id":format!("s{at}")}))
        .collect::<Vec<_>>();
    definitions.push(json!({"id":"$host","match":{"remote":true}}));
    let msg = msg(json!({
        "window_id":44,"enabled":true,"definitions":definitions,
        "tabs":[
            {"id":1,"index":1,"title":"one","remote":true,"host":"pi"},
            {"id":2,"index":2,"title":"two","remote":true,"host":"nas"}
        ],"active_tab":1
    }));
    let result = resolved(&msg, None);
    assert_eq!(result.assignments[0].space.as_deref(), Some("pi"));
    assert_eq!(result.assignments[1].space.as_deref(), Some("s0"));
    assert_eq!(result.summary.len(), 32);
    assert_eq!(
        result
            .warnings
            .iter()
            .filter(|warning| warning.code == "spaces_max")
            .count(),
        1
    );
}

#[test]
fn validation_drops_only_bad_parts_and_theme_layer_is_typed() {
    let msg = msg(json!({
        "window_id":1,"enabled":true,
        "definitions":[
            7,
            {"id":"home","name":9,"theme":{"accent":[1,2,3],"bg":false,"elevation":2,"scrim":-0.1,"mystery":true},"match":{"proc":["zsh",4],"wat":"x"}},
            {"id":"home"},
            {"id":"work","theme":"wrong"},
            {"id":"bad-colour","theme":{"accent":"#zzzzzz"}}
        ],
        "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh"}],"active_tab":1
    }));
    let result = resolved(&msg, None);
    assert_eq!(result.active.as_deref(), Some("home"));
    assert_eq!(result.theme_overrides.accent.as_deref(), Some("#010203"));
    assert_eq!(result.theme_overrides.bg, None);
    assert_eq!(result.theme_overrides.elevation, None);
    assert_eq!(result.theme_overrides.scrim, None);
    let codes = result
        .warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"missing_id"));
    assert!(codes.contains(&"invalid_definition_field"));
    assert!(codes.contains(&"invalid_theme_field"));
    assert!(codes.contains(&"unknown_theme_field"));
    assert!(codes.contains(&"invalid_match_field"));
    assert!(codes.contains(&"unknown_match_field"));
    assert!(codes.contains(&"duplicate_id"));
    assert!(codes.contains(&"invalid_theme"));
    assert!(result.warnings.iter().any(|warning| {
        warning.code == "invalid_theme_field"
            && warning.space_id.as_deref() == Some("bad-colour")
            && warning.field.as_deref() == Some("accent")
    }));
}

#[test]
fn automatic_theme_accent_uses_normal_then_bright_palette_slots_stably() {
    let msg = msg(json!({
        "window_id":1,"enabled":true,
        "definitions":[{"id":"home"},{"id":"$host","theme":"auto","match":{"remote":true}}],
        "tabs":[{"id":1,"index":1,"title":"one","remote":true,"host":"pi"}],"active_tab":1
    }));
    let first = resolved(&msg, None).theme_overrides.accent;
    let second = resolved(&msg, None).theme_overrides.accent;
    assert!(first.is_some());
    assert_eq!(first, second);
}

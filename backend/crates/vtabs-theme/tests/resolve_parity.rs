//! resolve() must reproduce theme.lua byte-for-byte; the fixture is exported by
//! `lua scripts/export-theme-fixture.lua` and committed.

use serde_json::Value;
use vtabs_theme::{Palette, Rgb, Theme, UserTheme, resolve};

fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_string)
}

fn palette(v: &Value) -> Palette {
    Palette {
        background: string(v, "background"),
        foreground: string(v, "foreground"),
        cursor_bg: string(v, "cursor_bg"),
        active_tab_bg: v
            .pointer("/tab_bar/active_tab/bg_color")
            .and_then(Value::as_str)
            .map(str::to_string),
        ansi: v
            .get("ansi")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn user(v: &Value) -> UserTheme {
    UserTheme {
        accent: string(v, "accent"),
        elevation: v.get("elevation").and_then(Value::as_f64),
        hover_bg: string(v, "hover_bg"),
        scrim: v.get("scrim").and_then(Value::as_f64),
        popover_sel_fg: string(v, "popover_sel_fg"),
        ..UserTheme::default()
    }
}

fn rgb(v: &Value, key: &str) -> Rgb {
    let a = v[key]
        .as_array()
        .unwrap_or_else(|| panic!("fixture rgb {key}"));
    [
        a[0].as_u64().unwrap() as u8,
        a[1].as_u64().unwrap() as u8,
        a[2].as_u64().unwrap() as u8,
    ]
}

fn check_case(name: &str, want: &Value, got: &Theme) {
    let rgb_fields: &[(&str, Rgb)] = &[
        ("bg", got.bg),
        ("fg", got.fg),
        ("dim", got.dim),
        ("accent", got.accent),
        ("title_idle", got.title_idle),
        ("meta_fg", got.meta_fg),
        ("active_bg", got.active_bg),
        ("active_fg", got.active_fg),
        ("hover_bg", got.hover_bg),
        ("hover_fg", got.hover_fg),
        ("focus_bg", got.focus_bg),
        ("pinned_fg", got.pinned_fg),
        ("separator", got.separator),
        ("border", got.border),
        ("border_idle", got.border_idle),
        ("ghost_border_hover", got.ghost_border_hover),
        ("new_tab_fg", got.new_tab_fg),
        ("close_fg", got.close_fg),
        ("close_hover_fg", got.close_hover_fg),
        ("unseen_fg", got.unseen_fg),
        ("private_accent", got.private_accent),
        ("drag_bg", got.drag_bg),
        ("drag_fg", got.drag_fg),
        ("scroll_fg", got.scroll_fg),
        ("scroll_idle_fg", got.scroll_idle_fg),
        ("title_active", got.title_active),
        ("active_title_fg", got.active_title_fg),
        ("content_bg", got.content_bg),
        ("surface_raised", got.surface_raised),
        ("disabled_fg", got.disabled_fg),
        ("popover_sel_bg", got.popover_sel_bg),
        ("popover_sel_fg", got.popover_sel_fg),
        ("popover_sel_hint", got.popover_sel_hint),
    ];
    for (field, value) in rgb_fields {
        assert_eq!(*value, rgb(want, field), "{name}: {field}");
    }
    for (field, value) in [
        ("scrim", got.scrim),
        ("title_active_contrast", got.title_active_contrast),
    ] {
        let want = want[field]
            .as_f64()
            .unwrap_or_else(|| panic!("fixture {field}"));
        assert!(
            (value - want).abs() < 1e-9,
            "{name}: {field} rust {value} lua {want}"
        );
    }
}

#[test]
fn resolve_matches_the_lua_fixture() {
    let raw = include_str!("fixtures/resolve.json");
    let cases: Value = serde_json::from_str(raw).expect("fixture parses");
    let cases = cases.as_array().expect("fixture is a list");
    assert!(cases.len() >= 36, "fixture unexpectedly small");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let got = resolve(
            &user(&case["user"]),
            &palette(&case["palette"]),
            case["private"].as_bool().unwrap_or(false),
        );
        check_case(name, &case["resolved"], &got);
    }
}

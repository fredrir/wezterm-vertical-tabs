//! sanitize/strip_geometry/icons must reproduce their Lua originals; fixtures from
//! `lua scripts/export-parity-fixtures.lua`.

use serde_json::Value;
use vtabs_core::geom::{Dims, StripOpts, strip_geometry};
use vtabs_core::{icons, sanitize};

#[test]
fn sanitize_matches_the_lua_fixture() {
    let cases: Value = serde_json::from_str(include_str!("fixtures/sanitize.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let bytes: Vec<u8> = case["bytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect();
        assert_eq!(
            sanitize(&bytes),
            case["out"].as_str().unwrap(),
            "sanitize {bytes:?}"
        );
    }
}

#[test]
fn strip_geometry_matches_the_lua_fixture() {
    let cases: Value = serde_json::from_str(include_str!("fixtures/geom.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let d = &case["dims"];
        let o = &case["opts"];
        let dims = Dims {
            cols: d["cols"].as_u64().unwrap_or(0) as u32,
            viewport_rows: d["viewport_rows"].as_u64().unwrap_or(0) as u32,
            pixel_width: d["pixel_width"].as_f64().unwrap_or(0.0),
            pixel_height: d["pixel_height"].as_f64().unwrap_or(0.0),
            dpi: d["dpi"].as_f64(),
        };
        let opts = StripOpts {
            is_mac: o["is_mac"].as_bool().unwrap_or(false),
            integrated_buttons: o["integrated_buttons"].as_bool().unwrap_or(false),
            native_button_style: o["native_button_style"].as_bool().unwrap_or(false),
            position_left: o["position"].as_str() == Some("left"),
            is_full_screen: o["is_full_screen"].as_bool().unwrap_or(false),
            preview: o["preview"].as_bool().unwrap_or(false),
            rail: o["rail"].as_bool().unwrap_or(false),
            rail_width: o["rail_width"].as_u64().unwrap_or(0) as u32,
            padding_top: o["padding_top"].as_i64().unwrap_or(0),
            toggle_button: o["toggle_button"].as_bool().unwrap_or(false),
            card_x1: o["card_x1"].as_u64().map(|v| v as u32),
        };
        let got = strip_geometry(dims, opts);
        let want = &case["out"];
        assert_eq!(
            u64::from(got.rows),
            want["rows"].as_u64().unwrap(),
            "{name}: rows"
        );
        assert_eq!(
            u64::from(got.rows_reserved),
            want["rows_reserved"].as_u64().unwrap(),
            "{name}: rows_reserved"
        );
        assert_eq!(
            u64::from(got.cols),
            want["cols"].as_u64().unwrap(),
            "{name}: cols"
        );
        assert_eq!(
            u64::from(got.toggle_row),
            want["toggle_row"].as_u64().unwrap(),
            "{name}: toggle_row"
        );
        assert_eq!(
            u64::from(got.toggle_x),
            want["toggle_x"].as_u64().unwrap(),
            "{name}: toggle_x"
        );
        assert_eq!(
            got.width.map(u64::from),
            want["width"].as_u64(),
            "{name}: width"
        );
        match (got.cell_w, want["cell_w"].as_f64()) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-9, "{name}: cell_w {a} vs {b}"),
            (a, b) => assert_eq!(a.is_some(), b.is_some(), "{name}: cell_w presence"),
        }
    }
}

#[test]
fn icons_resolve_mechanics_match_the_lua_fixture() {
    // Values under the stub are ASCII fallbacks, so only overrides and pattern extraction compare.
    let cases: Value = serde_json::from_str(include_str!("fixtures/icons_resolve.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let map: std::collections::BTreeMap<String, String> = case["icon_map"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let got = icons::resolve(&map);
        for (key, value) in &map {
            assert_eq!(
                got.map.get(key).map(String::as_str),
                Some(value.as_str()),
                "override {key}"
            );
        }
        let want_patterns: Vec<(String, String)> = case["resolved"]["patterns"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|p| {
                (
                    p["pattern"].as_str().unwrap().to_string(),
                    p["icon"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(got.patterns, want_patterns, "patterns");
    }
}

//! glyphs::resolve must reproduce glyphs.lua; fixture from `lua scripts/export-parity-fixtures.lua`.

use std::collections::BTreeMap;

use serde_json::Value;
use vtabs_view::glyphs;

#[test]
fn resolve_matches_the_lua_fixture() {
    let cases: Value = serde_json::from_str(include_str!("fixtures/glyphs.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let base: BTreeMap<String, String> = case["base"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let got = glyphs::resolve(
            &base,
            case["custom_block_glyphs"].as_bool().unwrap(),
            case["wide_ambiguous"].as_bool().unwrap(),
        );
        let want = case["resolved"].as_object().unwrap();
        assert_eq!(
            got.corners,
            want["corners"].as_str().unwrap(),
            "{name}: corners"
        );
        for (key, value) in want {
            if key == "corners" {
                continue;
            }
            assert_eq!(
                got.glyphs.get(key).map(String::as_str),
                value.as_str(),
                "{name}: {key}"
            );
        }
        assert_eq!(got.glyphs.len(), want.len() - 1, "{name}: key count");
    }
}

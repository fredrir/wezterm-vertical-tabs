//! text.rs must reproduce util.lua's width/truncate/pad_right/shorten_path byte-for-byte;
//! fixtures come from `lua scripts/export-parity-fixtures.lua`.

use serde_json::Value;
use vtabs_view::text;

#[test]
fn width_truncate_and_pad_match_the_lua_fixture() {
    let cases: Value = serde_json::from_str(include_str!("fixtures/text.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let s = case["s"].as_str().unwrap();
        let max = case["max"].as_u64().unwrap() as usize;
        assert_eq!(
            text::width(s) as u64,
            case["width"].as_u64().unwrap(),
            "width {s:?}"
        );
        assert_eq!(
            text::truncate(s, max, "…"),
            case["truncated"].as_str().unwrap(),
            "truncate {s:?} {max}"
        );
        assert_eq!(
            text::truncate(s, max, ""),
            case["truncated_bare"].as_str().unwrap(),
            "truncate bare {s:?} {max}"
        );
        assert_eq!(
            text::pad_right(s, max),
            case["padded"].as_str().unwrap(),
            "pad {s:?} {max}"
        );
    }
}

#[test]
fn shorten_path_matches_the_lua_fixture() {
    let cases: Value = serde_json::from_str(include_str!("fixtures/paths.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let path = case["path"].as_str().unwrap();
        let budget = case["budget"].as_u64().unwrap() as usize;
        assert_eq!(
            text::shorten_path(path, budget, "…"),
            case["shortened"].as_str().unwrap(),
            "shorten {path:?} {budget}"
        );
    }
}

use super::*;

#[test]
fn the_filter_is_a_case_folded_substring_of_the_raw_key() {
    assert!(matches("padding.top", ""));
    assert!(matches("padding.top", "PAD"));
    assert!(matches("padding.top", "g.t"));
    assert!(!matches("padding.top", "width"));
}

#[test]
fn the_short_source_only_shortens_the_three_reasons_lock_for_produces() {
    assert_eq!(short_source("wezterm.lua (host)"), "host");
    assert_eq!(short_source("not editable"), "read-only");
    assert_eq!(
        short_source("mystery"),
        "mystery",
        "no panic on a new reason"
    );
}

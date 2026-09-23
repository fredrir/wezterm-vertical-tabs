use crate::{Managed, Space, lua};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn strings_escape_quotes_backslashes_and_control_characters() {
    assert_eq!(
        lua::string("a\"b\\c\nd\te\u{1}é"),
        r#""a\"b\\c\nd\te\u{1}é""#
    );
}

#[test]
fn identifier_keys_are_bare_and_others_are_bracketed() {
    assert_eq!(
        lua::source(&json!({"width": 1, "end": 2, "fish-mode": 3, "2x": 4}), 0),
        r#"{ ["2x"] = 4, ["end"] = 2, ["fish-mode"] = 3, width = 1 }"#
    );
}

#[test]
fn long_tables_wrap_one_entry_per_line() {
    let value = json!({"names": ["x".repeat(60), "y".repeat(60)]});
    assert_eq!(
        lua::source(&value, 0),
        format!(
            "{{\n  names = {{\n    \"{}\",\n    \"{}\",\n  }},\n}}",
            "x".repeat(60),
            "y".repeat(60)
        )
    );
}

#[test]
fn managed_files_omit_runtime_and_default_space_fields() {
    let mut space = Space::new("notes", "Notes");
    space.collapsed = true;
    let managed = Managed {
        settings: BTreeMap::from([("width".into(), json!(300))]),
        spaces: vec![space],
        templates: Vec::new(),
    };
    let source = managed.to_lua();
    assert!(source.starts_with("-- Managed by vertical-tabs"));
    assert!(source.contains("---@type TabsManaged\nreturn {"));
    assert!(source.contains(r#"spaces = { { icon = "◉", id = "notes", name = "Notes" } }"#));
    assert!(source.contains("settings = { width = 300 }"));
    assert!(!source.contains("collapsed"));
    assert!(!source.contains("rules"));
}

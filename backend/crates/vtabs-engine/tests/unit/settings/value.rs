use super::*;

#[test]
fn dotted_paths_build_and_read_nested_tables() {
    let mut root = table([]);
    set_path(&mut root, "padding.top", 1.into());
    assert_eq!(get_path(&root, "padding.top"), Some(&Value::Number(1.0)));
    assert_eq!(get_path(&root, "padding.left"), None);
}

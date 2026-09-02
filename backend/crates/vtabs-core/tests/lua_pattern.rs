use vtabs_core::lua_pattern::matches;

#[test]
fn literals_classes_and_anchors() {
    assert!(matches("lazygit", "git"));
    assert!(matches("git", "^git$"));
    assert!(!matches("lazygit", "^git"));
    assert!(matches("python3", "^python%d?$"));
    assert!(matches("node", "^n%a+$"));
    assert!(!matches("node2", "^n%a+$"));
    assert!(matches("cargo-watch", "^cargo%-"));
    assert!(matches("x.y", "%."));
    assert!(!matches("xy", "%."));
}

#[test]
fn sets_ranges_and_quantifiers() {
    assert!(matches("vim", "^[vn]i"));
    assert!(matches("nim", "^[vn]i"));
    assert!(!matches("him", "^[vn]i"));
    assert!(matches("a1", "^%a[0-9]*$"));
    assert!(matches("a", "^%a[0-9]*$"));
    assert!(matches("abc9", "[^%d]+%d"));
    assert!(matches("xyz", "x.-z"));
}

#[test]
fn unsupported_constructs_never_match() {
    assert!(!matches("abc", "(a)"));
    assert!(!matches("abc", "%babc"));
    assert!(!matches("abc", "%f[%a]abc"));
}

use vtabs_runtime::uservar::{Role, nonce, title_marker};

#[test]
fn title_marker_keeps_hex_only() {
    assert_eq!(
        title_marker(Role::Sidebar, "a1b2"),
        "\x1b]0;wez-vtabs:a1b2\x07\x1b]2;wez-vtabs:a1b2\x07"
    );
    assert_eq!(
        title_marker(Role::Sidebar, "\x07;evil ~"),
        "\x1b]0;wez-vtabs:e\x07\x1b]2;wez-vtabs:e\x07"
    );
    assert!(title_marker(Role::Sidebar, "").starts_with("\x1b]0;wez-vtabs:0\x07"));
}

#[test]
fn nonce_is_eight_hex_digits() {
    let n = nonce();
    assert_eq!(n.len(), 8);
    assert!(n.bytes().all(|b| b.is_ascii_hexdigit()));
}

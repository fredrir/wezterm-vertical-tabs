use super::restarted;

#[test]
fn only_a_changed_session_marks_a_restart() {
    assert!(!restarted(None, "session:a"));
    assert!(!restarted(Some("session:a"), "session:a"));
    assert!(restarted(Some("session:a"), "session:b"));
    // Older servers identify by host, executable and config; a change there is no restart.
    assert!(!restarted(Some("host\0/bin/wezterm\0"), "session:b"));
}

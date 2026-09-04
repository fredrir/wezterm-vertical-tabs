use super::{truncate, width};

#[test]
fn emoji_sequences_use_their_terminal_width_and_are_never_split() {
    assert_eq!(width("👩‍💻"), 2);
    assert_eq!(truncate("A👩‍💻B", 3, ""), "A👩‍💻");
}

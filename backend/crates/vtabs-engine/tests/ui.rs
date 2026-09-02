use vtabs_engine::{UiState, ui::SettingsUi};

#[test]
fn a_second_click_on_the_same_target_inside_the_window_is_a_double() {
    let mut ui = UiState::default();
    assert!(!ui.double_click("tab:1", 0, 300));
    assert!(ui.double_click("tab:1", 200, 300));
    // the match consumed the first, so a third click starts over
    assert!(!ui.double_click("tab:1", 250, 300));
}

#[test]
fn a_different_target_or_a_late_click_is_not() {
    let mut ui = UiState::default();
    assert!(!ui.double_click("tab:1", 0, 300));
    assert!(!ui.double_click("tab:2", 50, 300), "another target");
    let mut late = UiState::default();
    assert!(!late.double_click("space", 0, 300));
    assert!(!late.double_click("space", 400, 300), "past the window");
}

#[test]
fn hover_expires_only_once_and_never_with_the_timeout_off() {
    let mut ui = UiState::default();
    ui.set_hover(3, 4, 100);
    assert!(!ui.expire_hover(150, 120), "still fresh");
    assert!(ui.expire_hover(300, 120), "gone");
    assert!(!ui.expire_hover(400, 120), "and stays gone");

    let mut off = UiState::default();
    off.set_hover(3, 4, 0);
    assert!(!off.expire_hover(999_999, 0));
    assert!(off.hover.is_some());
}

#[test]
fn settings_filter_edits_user_perceived_characters() {
    let mut ui = SettingsUi::default();
    ui.type_filter("e");
    ui.type_filter("\u{301}");
    assert_eq!(ui.filter, "e\u{301}");
    ui.type_filter("backspace");
    assert_eq!(ui.filter, "", "backspace removes the combined grapheme");

    ui.type_filter("👩‍💻");
    assert_eq!(ui.filter, "👩‍💻", "one multi-scalar grapheme is accepted");
    ui.type_filter("backspace");
    assert!(ui.filter.is_empty());
}

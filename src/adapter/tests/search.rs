use super::*;

#[test]
fn search_palette_follows_the_theme_until_set_and_none_hides_the_border() {
    let theme = ui::Theme::default();
    let mut settings = core::Settings::default();
    let defaults = palette(&theme, &settings);
    assert_eq!(defaults.background, srgba(theme.card));
    assert_eq!(defaults.text, srgba(theme.foreground));
    assert_eq!(defaults.accent, srgba(theme.accent));
    assert_eq!(defaults.active_match.0, srgba(theme.accent));
    assert!(defaults.border.is_some());

    settings.search_border_color = Some("none".into());
    settings.search_match_color = Some("#ff0000".into());
    let custom = palette(&theme, &settings);
    assert_eq!(custom.border, None);
    assert_eq!(custom.other_match.0, SrgbaTuple::from((255u8, 0u8, 0u8)));
}

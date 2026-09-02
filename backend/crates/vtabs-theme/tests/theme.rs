use vtabs_theme::{Palette, UserTheme, luminance, resolve};

fn palette(bg: &str, fg: &str) -> Palette {
    Palette {
        background: Some(bg.to_string()),
        foreground: Some(fg.to_string()),
        ..Palette::default()
    }
}

#[test]
fn default_sidebar_is_darker_than_dark_and_light_content() {
    for (palette, expected) in [
        (palette("#1e1e2e", "#cdd6f4"), [0x17, 0x17, 0x23]),
        (palette("#eff1f5", "#4c4f69"), [0xe1, 0xe3, 0xe6]),
    ] {
        let theme = resolve(&UserTheme::default(), &palette, false);
        assert!(luminance(theme.bg) < luminance(theme.content_bg));
        assert_eq!(theme.bg, expected);
    }
}

#[test]
fn zero_elevation_is_seamless_and_an_explicit_background_wins() {
    let palette = palette("#1e1e2e", "#cdd6f4");
    let seamless = resolve(
        &UserTheme {
            elevation: Some(0.0),
            ..UserTheme::default()
        },
        &palette,
        false,
    );
    assert_eq!(seamless.bg, seamless.content_bg);

    let explicit = resolve(
        &UserTheme {
            bg: Some("#123456".to_string()),
            ..UserTheme::default()
        },
        &palette,
        false,
    );
    assert_eq!(explicit.bg, [0x12, 0x34, 0x56]);
}

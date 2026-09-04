use vtabs_engine::theme::{
    Palette, UserTheme, luminance, overlay, parse, resolve, valid_overrides,
};

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

#[test]
fn color_parser_rejects_arbitrary_utf8_without_panicking() {
    for value in ["#éx", "#xéxxx", "#💻", "#12é45", "é12345"] {
        assert_eq!(parse(value), None, "{value:?}");
    }
    assert_eq!(parse("#abc"), Some([0xaa, 0xbb, 0xcc]));
    assert_eq!(parse("#12FeA0"), Some([0x12, 0xfe, 0xa0]));
}

#[test]
fn a_hook_layers_over_the_raw_theme_without_erasing_unspecified_keys() {
    let raw = UserTheme {
        bg: Some("#123456".into()),
        accent: Some("#abcdef".into()),
        scrim: Some(0.3),
        ..Default::default()
    };
    let hook = UserTheme {
        accent: Some("#fedcba".into()),
        ..Default::default()
    };
    let merged = overlay(&raw, &hook);
    assert_eq!(merged.bg, raw.bg);
    assert_eq!(merged.scrim, raw.scrim);
    assert_eq!(merged.accent, hook.accent);
}

#[test]
fn hook_output_rejects_bad_colours_and_out_of_range_fractions() {
    assert!(valid_overrides(&UserTheme {
        accent: Some("#abc".into()),
        scrim: Some(1.0),
        ..Default::default()
    }));
    assert!(!valid_overrides(&UserTheme {
        accent: Some("not-a-colour".into()),
        ..Default::default()
    }));
    assert!(!valid_overrides(&UserTheme {
        elevation: Some(-0.1),
        ..Default::default()
    }));
}

#[test]
fn rust_matches_the_full_shipped_lua_dark_palette_golden() {
    // Captured from the pre-refactor Lua resolver with this palette and no user overrides. Keeping
    // every field here makes removal of that Lua algorithm a checked hand-off, not a visual guess.
    let got = serde_json::to_value(resolve(
        &UserTheme::default(),
        &palette("#1e1e2e", "#cdd6f4"),
        false,
    ))
    .unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        r#"{"accent":[137,180,250],"active_bg":[56,62,83],"active_fg":[205,214,244],"bg":[23,23,35],"border":[86,88,107],"border_idle":[79,83,100],"close_fg":[135,141,164],"close_hover_fg":[243,139,168],"content_bg":[30,30,46],"dim":[144,150,174],"disabled_fg":[97,101,120],"drag_bg":[63,78,110],"drag_fg":[205,214,244],"fg":[205,214,244],"focus_bg":[52,62,89],"ghost_border_hover":[112,134,179],"hover_bg":[34,34,48],"hover_fg":[205,214,244],"meta_fg":[144,150,174],"new_tab_fg":[150,157,181],"pinned_fg":[144,150,174],"popover_sel_bg":[137,180,250],"popover_sel_fg":[0,0,0],"popover_sel_hint":[55,72,100],"private_accent":[203,166,247],"scrim":0.69999999999999996,"scroll_fg":[77,80,97],"scroll_idle_fg":[47,49,63],"separator":[41,42,56],"surface_raised":[39,40,54],"title_active":[137,180,250],"title_active_contrast":5.0313134743475239,"title_idle":[183,191,219],"unseen_fg":[137,180,250]}"#,
    )
    .unwrap();
    assert_eq!(got, expected);
}

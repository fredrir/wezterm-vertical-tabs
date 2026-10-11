use serde_json::json;
use vtabs_core::{Settings, distros};

#[test]
fn partial_maps_keep_the_default_color_of_every_other_os() {
    let mut settings = Settings::default();
    settings
        .set("distro_colors", json!({"arch": "#a6e3a1"}))
        .unwrap();
    let mut expected = distros::defaults();
    expected.insert("arch".into(), "#a6e3a1".into());
    assert_eq!(settings.distro_colors, expected);

    settings.set("distro_colors", json!({})).unwrap();
    assert_eq!(settings.distro_colors, distros::defaults());
}

#[test]
fn unknown_os_ids_and_invalid_colors_do_not_change_settings() {
    for value in [
        json!({"kali": "#ffffff"}),
        json!({"arch": "red"}),
        json!({"arch": null}),
        json!(["#ffffff"]),
    ] {
        let mut settings = Settings::default();
        assert!(
            settings.set("distro_colors", value.clone()).is_err(),
            "accepted {value}"
        );
        assert_eq!(settings.distro_colors, distros::defaults());
    }
}

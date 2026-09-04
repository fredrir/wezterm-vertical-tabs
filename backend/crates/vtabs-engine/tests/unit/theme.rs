#[test]
fn behavioural_field_walks_match_the_protocol_manifest() {
    macro_rules! names {
        ($($field:ident),+ $(,)?) => {
            &[$(stringify!($field)),+]
        };
    }
    assert_eq!(
        theme_color_fields!(names),
        vtabs_protocol::payload::THEME_COLOR_FIELDS
    );
    assert_eq!(
        theme_fraction_fields!(names),
        vtabs_protocol::payload::THEME_FRACTION_FIELDS
    );
}

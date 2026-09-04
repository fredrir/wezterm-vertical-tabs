use super::*;
use crate::settings::document::RawSettings;

#[test]
fn a_document_produces_the_engine_presentation_directly() {
    let presentation =
        SettingsDocument::new(RawSettings::default()).presentation(Some("9.9.9".to_owned()));
    assert_eq!(presentation.version.as_deref(), Some("9.9.9"));
    assert!(!presentation.fields.is_empty());
    assert!(presentation.groups.iter().any(|group| group.id == "layout"));
    assert!(
        presentation
            .fields
            .iter()
            .all(|field| !field.path.0.is_empty())
    );
}

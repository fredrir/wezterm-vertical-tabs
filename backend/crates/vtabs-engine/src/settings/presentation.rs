use super::document::{Field, Group, LockReason, SettingsDocument, Widget};
use super::value::SettingPath;

/// Renderer-facing settings state derived from the canonical settings document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsPresentation {
    pub fields: Vec<PresentationField>,
    pub groups: Vec<PresentationGroup>,
    pub caveat: Option<Vec<String>>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationField {
    pub path: SettingPath,
    pub label: String,
    pub group: String,
    pub widget: Widget,
    pub value_text: String,
    pub changed: bool,
    pub locked: Option<PresentationLock>,
    pub depth: usize,
    pub help: String,
    pub editing: Option<String>,
    pub armed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationGroup {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationLock {
    pub text: String,
}

impl From<Field> for PresentationField {
    fn from(field: Field) -> Self {
        Self {
            path: field.path,
            label: field.label,
            group: field.group,
            widget: field.widget,
            value_text: field.value_text,
            changed: field.changed,
            locked: field.locked.map(PresentationLock::from),
            depth: field.depth,
            help: field.help,
            editing: field.editing,
            armed: field.armed,
        }
    }
}

impl From<LockReason> for PresentationLock {
    fn from(reason: LockReason) -> Self {
        Self {
            text: reason.text().to_owned(),
        }
    }
}

impl From<Group> for PresentationGroup {
    fn from(group: Group) -> Self {
        Self {
            id: group.id,
            label: group.label,
        }
    }
}

impl SettingsDocument {
    /// Produces the immutable presentation painted and hit-tested for this document state.
    pub fn presentation(&self, version: Option<String>) -> SettingsPresentation {
        SettingsPresentation {
            fields: self.fields().into_iter().map(Into::into).collect(),
            groups: self.groups().into_iter().map(Into::into).collect(),
            caveat: self
                .caveat()
                .map(|lines| lines.iter().map(|line| (*line).to_owned()).collect()),
            version,
        }
    }
}

#[cfg(test)]
mod tests {
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
}

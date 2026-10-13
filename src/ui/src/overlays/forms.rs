use crate::SidebarUi;
use crate::element::ElementId;
use crate::input::TextEditor;
use crate::intent::UiIntent;
use crate::overlays::colors::{Pick, hex};
use crate::overlays::{Form, FormKind, Overlay};
use crate::settings::SettingTarget;
use vtabs_core::{Intent, Model};

impl SidebarUi {
    pub(crate) fn open_form(&mut self, title: impl Into<String>, kind: FormKind, value: &str) {
        let mut editor = TextEditor::new(value);
        editor.select_all();
        self.push_overlay(Overlay::Form(Form {
            title: title.into(),
            kind,
            editor,
            error: None,
            color: None,
        }));
        self.focused = Some(ElementId::Editor);
        self.reset_caret();
    }

    pub(crate) fn submit_form(&mut self, model: &Model, intents: &mut Vec<UiIntent>) {
        let Some(Overlay::Form(form)) = self.overlays.current_mut() else {
            return;
        };
        match form_value(form).and_then(|value| form_intent(model, &form.kind, value)) {
            Ok(intent) => {
                intents.push(UiIntent::Domain(intent));
                self.overlays.submitted(model.revision);
            }
            Err(error) => form.error = Some(error),
        }
        self.frame.dirty = true;
    }
}

fn form_value(form: &Form) -> Result<String, String> {
    let Some(color) = &form.color else {
        return Ok(form.editor.text().trim().to_owned());
    };
    if color.selected == Pick::NoColor {
        return Ok("none".into());
    }
    color
        .parse(form.editor.text())
        .map(|value| value.map_or_else(String::new, hex))
        .map_err(Into::into)
}

fn form_intent(model: &Model, kind: &FormKind, value: String) -> Result<Intent, String> {
    Ok(match kind {
        FormKind::CreateFolder => Intent::CreateFolder { name: value },
        FormKind::RenameFolder(id) => Intent::RenameFolder {
            id: id.clone(),
            name: value,
        },
        FormKind::CreateSpace => Intent::CreateSpace { name: value },
        FormKind::RenameSpace(id) => Intent::RenameSpace {
            id: id.clone(),
            name: value,
        },
        FormKind::RenameTab(id) => Intent::RenameTab {
            id: *id,
            title: value,
        },
        FormKind::SpaceIcon(id) | FormKind::SpaceAccent(id) | FormKind::SpaceRules(id) => {
            let space = model
                .spaces
                .iter()
                .find(|s| &s.id == id)
                .ok_or("This space no longer exists")?;
            let mut icon = space.icon.clone();
            let mut accent = space.accent.clone();
            let mut rules = space.rules.clone();
            match kind {
                FormKind::SpaceIcon(_) => icon = value,
                FormKind::SpaceAccent(_) => accent = (!value.is_empty()).then_some(value),
                _ => {
                    rules = serde_json::from_str(&value)
                        .map_err(|error| format!("Invalid rules: {error}"))?;
                }
            }
            Intent::EditSpace {
                id: id.clone(),
                icon,
                accent,
                rules,
            }
        }
        FormKind::Setting(key) => SettingTarget::of(key).submit(&model.settings, &value)?,
    })
}

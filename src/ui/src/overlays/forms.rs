use crate::SidebarUi;
use crate::element::ElementId;
use crate::input::TextEditor;
use crate::intent::UiIntent;
use crate::overlays::{Form, FormKind, Overlay};
use serde_json::Value;
use vtabs_core::{Intent, Model, SettingKind, settings};

impl SidebarUi {
    pub(crate) fn open_form(&mut self, title: impl Into<String>, kind: FormKind, value: &str) {
        self.overlays.stash();
        let mut editor = TextEditor::new(value);
        editor.select_all();
        self.open_overlay(Overlay::Form(Form {
            title: title.into(),
            kind,
            editor,
            error: None,
        }));
        self.focused = Some(ElementId::Editor);
        self.reset_caret();
    }

    pub(crate) fn submit_form(&mut self, model: &Model, intents: &mut Vec<UiIntent>) {
        let Some(Overlay::Form(form)) = &mut self.overlays.current else {
            return;
        };
        let value = form.editor.text().trim().to_owned();
        let intent: Result<Intent, String> = (|| {
            Ok(match &form.kind {
                FormKind::CreateFolder => {
                    valid_name(&value)?;
                    Intent::CreateFolder {
                        name: value.clone(),
                    }
                }
                FormKind::RenameFolder(id) => {
                    valid_name(&value)?;
                    Intent::RenameFolder {
                        id: id.clone(),
                        name: value.clone(),
                    }
                }
                FormKind::CreateSpace => {
                    valid_name(&value)?;
                    Intent::CreateSpace {
                        name: value.clone(),
                    }
                }
                FormKind::RenameSpace(id) => {
                    valid_name(&value)?;
                    Intent::RenameSpace {
                        id: id.clone(),
                        name: value.clone(),
                    }
                }
                FormKind::RenameTab(id) => {
                    if value.chars().count() > 512 {
                        return Err("Use at most 512 characters".into());
                    }
                    Intent::RenameTab {
                        id: *id,
                        title: value.clone(),
                    }
                }
                FormKind::SpaceIcon(id) | FormKind::SpaceAccent(id) | FormKind::SpaceRules(id) => {
                    let space = model
                        .spaces
                        .iter()
                        .find(|s| &s.id == id)
                        .ok_or("This space no longer exists")?;
                    let mut icon = space.icon.clone();
                    let mut accent = space.accent.clone();
                    let mut rules = space.rules.clone();
                    match &form.kind {
                        FormKind::SpaceIcon(_) => {
                            if value.chars().count() > 16 {
                                return Err("Use at most 16 characters".into());
                            }
                            icon = value.clone();
                        }
                        FormKind::SpaceAccent(_) => {
                            if !value.is_empty() && !settings::valid_color(&value) {
                                return Err("Use #RRGGBB or leave empty".into());
                            }
                            accent = (!value.is_empty()).then_some(value.clone());
                        }
                        FormKind::SpaceRules(_) => {
                            rules = serde_json::from_str(&value)
                                .map_err(|error| format!("Invalid rules: {error}"))?;
                        }
                        _ => {}
                    }
                    Intent::EditSpace {
                        id: id.clone(),
                        icon,
                        accent,
                        rules,
                    }
                }
                FormKind::Setting(key) => {
                    let descriptor =
                        settings::descriptor(key).ok_or("This setting no longer exists")?;
                    let value = match descriptor.kind {
                        SettingKind::Number { .. }
                        | SettingKind::Object
                        | SettingKind::Colors
                        | SettingKind::List => serde_json::from_str(&value)
                            .map_err(|error| format!("Invalid value: {error}"))?,
                        SettingKind::Text if value.is_empty() => Value::Null,
                        _ => Value::String(value.clone()),
                    };
                    settings::validate_value(key, &value)?;
                    Intent::SetSetting {
                        key: key.clone(),
                        value,
                    }
                }
            })
        })();
        match intent {
            Ok(intent) => {
                intents.push(UiIntent::Domain(intent));
                self.overlays.pending_form = Some(model.revision);
                self.frame.dirty = true;
            }
            Err(error) => {
                if let Some(Overlay::Form(form)) = &mut self.overlays.current {
                    form.error = Some(error);
                }
                self.frame.dirty = true;
            }
        }
    }
}

fn valid_name(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().count() > 128 {
        Err("Use 1–128 printable characters".into())
    } else {
        Ok(())
    }
}

//! What activating an element or choosing a menu item does.
use crate::SidebarUi;
use crate::element::ElementId;
use crate::input::{Key, Modifiers, TextEditor};
use crate::intent::{HostAction, UiIntent};
use crate::overlays::{FormKind, MenuItem, Overlay};
use crate::views::sidebar::InlineRename;
use crate::views::tab_name;
use serde_json::Value;
use vtabs_core::{Intent, Model, PaneId, RailMode, SettingKind, SpaceId, TabId, settings};

#[derive(Clone, Debug)]
pub(crate) enum Action {
    Domain(Intent),
    NewSpace,
    NewFolder,
    RenameFolder(String),
    MoveToFolder(TabId),
    RenameSpace(SpaceId),
    EditSpaceIcon(SpaceId),
    EditSpaceAccent(SpaceId),
    EditSpaceRules(SpaceId),
    DeleteSpace(SpaceId),
    RenameTab(TabId),
    MoveTab(TabId),
    CloseTab(TabId),
    KillPane(TabId, PaneId),
    Host(HostAction),
    Settings,
    CloseSettings,
    EditSetting(String),
    ResetSetting(String),
    EditorCommand { key: Key, target: ElementId },
    Submenu { title: String, items: Vec<MenuItem> },
    Confirm { label: String, action: Box<Action> },
    Close,
}

impl SidebarUi {
    pub(crate) fn activate_element(
        &mut self,
        model: &Model,
        id: ElementId,
        intents: &mut Vec<UiIntent>,
    ) {
        match id {
            ElementId::Search => self.open_tab_navigator(model),
            ElementId::Refresh => intents.push(UiIntent::Refresh),
            ElementId::CreateFolder => self.open_create_folder(),
            ElementId::Folder(id) => intents.push(UiIntent::Domain(Intent::ToggleFolder(id))),
            ElementId::SettingsCategory(category) => self.settings_select_category(category),
            ElementId::SettingsSearch => {
                self.settings.search_focused = true;
                self.focused = Some(ElementId::SettingsSearch);
                self.reset_caret();
                self.frame.dirty = true;
            }
            ElementId::CloseSettings => self.close_settings(),
            ElementId::ResetSettings => self.run_action(model, reset_settings(), intents),
            ElementId::CreateSpace => self.open_create_space(),
            ElementId::NewTab => {
                self.hide_settings();
                intents.push(UiIntent::Domain(Intent::NewTab));
            }
            ElementId::Settings | ElementId::SettingsTab => self.open_settings(),
            ElementId::CloseSettingsTab => self.close_settings(),
            ElementId::Rail => intents.push(UiIntent::Domain(toggle_rail(model))),
            ElementId::Space(id) => {
                self.sidebar.scroll = 0;
                self.frame.dirty = true;
                intents.push(UiIntent::Domain(Intent::SelectSpace(id)));
                self.start_effect(model);
            }
            ElementId::Tab(id) => {
                self.hide_settings();
                intents.push(UiIntent::Domain(Intent::ActivateTab(id)));
                self.start_effect(model);
            }
            ElementId::Pane(id, pane) => {
                self.hide_settings();
                intents.push(UiIntent::Domain(Intent::ActivateTab(id)));
                intents.push(UiIntent::Host(HostAction::FocusPane(id, pane)));
                self.start_effect(model);
            }
            ElementId::SpaceTitle => intents.push(UiIntent::Domain(Intent::ToggleSpace(
                model.selected_space.clone(),
            ))),
            ElementId::CloseTab(id) => self.close_tab(model, id, intents),
            ElementId::ClosePane(tab, pane) => {
                intents.push(UiIntent::Host(if model.settings.confirm_close {
                    HostAction::ClosePane(tab, pane)
                } else {
                    HostAction::KillPane(tab, pane)
                }));
            }
            ElementId::Menu(id) => {
                let action = match &self.overlays.current {
                    Some(Overlay::Menu(menu)) => menu
                        .items
                        .iter()
                        .find(|item| item.id == id && item.enabled)
                        .map(|item| item.action.clone()),
                    _ => None,
                };
                if let Some(action) = action {
                    self.run_action(model, action, intents);
                }
            }
            ElementId::Setting(key) => {
                self.settings_focus_setting(model, &key);
                self.edit_setting(model, &key, intents);
            }
            ElementId::Submit => self.submit_form(model, intents),
            ElementId::Cancel => self.back(),
            ElementId::Editor => {}
        }
    }

    pub(crate) fn run_action(
        &mut self,
        model: &Model,
        action: Action,
        intents: &mut Vec<UiIntent>,
    ) {
        match action {
            Action::Domain(intent) => {
                intents.push(UiIntent::Domain(intent));
                self.dismiss();
            }
            Action::NewSpace => self.open_create_space(),
            Action::NewFolder => self.open_create_folder(),
            Action::RenameFolder(id) => {
                if let Some(folder) = model.folders.iter().find(|f| f.id == id) {
                    self.open_form("Rename folder", FormKind::RenameFolder(id), &folder.name);
                }
            }
            Action::MoveToFolder(tab_id) => {
                let mut items: Vec<_> = model
                    .selected_folders()
                    .map(|folder| {
                        MenuItem::new(
                            folder.id.clone(),
                            folder.name.clone(),
                            Action::Domain(Intent::AssignFolder {
                                tab_id,
                                folder_id: Some(folder.id.clone()),
                            }),
                        )
                    })
                    .collect();
                items.push(MenuItem::new(
                    "ungroup",
                    "Remove from folder",
                    Action::Domain(Intent::AssignFolder {
                        tab_id,
                        folder_id: None,
                    }),
                ));
                self.push_menu("Move to folder", items);
            }
            Action::RenameSpace(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form("Rename space", FormKind::RenameSpace(id), &space.name);
                }
            }
            Action::EditSpaceIcon(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form("Space icon", FormKind::SpaceIcon(id), &space.icon);
                }
            }
            Action::EditSpaceAccent(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form(
                        "Accent (#RRGGBB or empty)",
                        FormKind::SpaceAccent(id),
                        space.accent.as_deref().unwrap_or(""),
                    );
                }
            }
            Action::EditSpaceRules(id) => {
                if let Some(space) = model.spaces.iter().find(|s| s.id == id) {
                    self.open_form(
                        "Routing rules (JSON)",
                        FormKind::SpaceRules(id),
                        &serde_json::to_string(&space.rules).unwrap_or_else(|_| "[]".into()),
                    );
                }
            }
            Action::RenameTab(id) => {
                if self.sidebar.title_rects.iter().any(|(tab, _)| *tab == id) {
                    self.dismiss();
                    self.start_rename(model, id);
                } else if let Some(tab) = model.tabs.get(&id) {
                    self.open_form("Rename tab", FormKind::RenameTab(id), tab.display_title());
                }
            }
            Action::MoveTab(id) => {
                let items = model
                    .spaces
                    .iter()
                    .map(|space| {
                        let mut item = MenuItem::new(
                            space.id.clone(),
                            space.name.clone(),
                            Action::Domain(Intent::AssignTab {
                                id,
                                space_id: space.id.clone(),
                            }),
                        );
                        item.enabled = model
                            .tabs
                            .get(&id)
                            .is_some_and(|tab| tab.space_id != space.id);
                        item
                    })
                    .collect();
                self.push_menu("Move to space", items);
            }
            Action::DeleteSpace(id) => {
                if model.spaces.len() <= 1 {
                    return;
                }
                if model.tabs.values().any(|tab| tab.space_id == id) {
                    let items = model
                        .spaces
                        .iter()
                        .filter(|space| space.id != id)
                        .map(|space| {
                            MenuItem::new(
                                space.id.clone(),
                                format!("Move tabs to {}", space.name),
                                Action::Confirm {
                                    label: "Move tabs and delete space?".into(),
                                    action: Box::new(Action::Domain(Intent::DeleteSpace {
                                        id: id.clone(),
                                        destination: Some(space.id.clone()),
                                    })),
                                },
                            )
                        })
                        .collect();
                    self.push_menu("Choose destination", items);
                } else {
                    self.run_action(
                        model,
                        Action::Confirm {
                            label: "Delete empty space?".into(),
                            action: Box::new(Action::Domain(Intent::DeleteSpace {
                                id,
                                destination: None,
                            })),
                        },
                        intents,
                    );
                }
            }
            Action::CloseTab(id) => self.close_tab(model, id, intents),
            Action::Settings => self.open_settings(),
            Action::CloseSettings => self.close_settings(),
            Action::EditSetting(key) => self.edit_setting(model, &key, intents),
            Action::ResetSetting(key) => {
                if !model.config_owned.contains(&key) {
                    intents.push(UiIntent::Domain(Intent::ResetSetting(key)));
                    self.frame.dirty = true;
                }
            }
            Action::EditorCommand { key, target } => {
                self.restore_editor(target);
                self.key(
                    model,
                    key,
                    Modifiers {
                        control: true,
                        ..Modifiers::default()
                    },
                    intents,
                );
            }
            Action::Submenu { title, items } => self.push_menu(title, items),
            Action::Confirm { label, action } => self.confirm(label, "", "Confirm", *action),
            Action::KillPane(tab, pane) => {
                intents.push(UiIntent::Host(HostAction::KillPane(tab, pane)));
                self.dismiss();
            }
            Action::Host(action) => {
                intents.push(UiIntent::Host(action));
                self.dismiss();
            }
            Action::Close => self.back(),
        }
    }

    /// Only a tab with a running process prompts; the host owns that fact.
    pub(crate) fn close_tab(&mut self, model: &Model, id: TabId, intents: &mut Vec<UiIntent>) {
        if !model.tabs.contains_key(&id) {
            return;
        }
        if model.settings.confirm_close {
            intents.push(UiIntent::Host(HostAction::CloseTab(id)));
            self.dismiss();
        } else {
            self.run_action(model, Action::Domain(Intent::CloseTab(id)), intents);
        }
    }

    pub(crate) fn edit_setting(&mut self, model: &Model, key: &str, intents: &mut Vec<UiIntent>) {
        if model.config_owned.contains(key) {
            return;
        }
        let Some(descriptor) = settings::descriptor(key) else {
            return;
        };
        let value = model.settings.get(key).unwrap_or_default();
        match descriptor.kind {
            SettingKind::Bool => {
                intents.push(UiIntent::Domain(Intent::SetSetting {
                    key: key.into(),
                    value: Value::Bool(!value.as_bool().unwrap_or(false)),
                }));
                self.frame.dirty = true;
            }
            SettingKind::Choice(choices) => {
                let items = choices
                    .iter()
                    .map(|choice| {
                        MenuItem::new(
                            *choice,
                            *choice,
                            Action::Domain(Intent::SetSetting {
                                key: key.into(),
                                value: Value::String((*choice).into()),
                            }),
                        )
                    })
                    .collect();
                self.push_menu(descriptor.label, items);
            }
            _ => {
                let value = value.as_str().map_or_else(
                    || {
                        if value.is_null() {
                            String::new()
                        } else {
                            value.to_string()
                        }
                    },
                    str::to_owned,
                );
                self.open_form(descriptor.label, FormKind::Setting(key.into()), &value);
            }
        }
    }

    /// Settings holds one position in the tab order and answers to that index.
    pub fn activate_index(&mut self, model: &Model, index: isize, intents: &mut Vec<UiIntent>) {
        let Some(slot) = self.settings_slot(model) else {
            self.hide_settings();
            intents.push(UiIntent::Domain(Intent::ActivateIndex(index)));
            return;
        };
        let total = model.visible_ids().len() as isize + 1;
        let position = if index < 0 { total + index } else { index };
        if position == slot {
            self.open_settings();
        } else {
            self.hide_settings();
            intents.push(UiIntent::Domain(Intent::ActivateIndex(
                position - isize::from(position > slot),
            )));
        }
    }

    pub fn activate_relative(
        &mut self,
        model: &Model,
        delta: isize,
        wrap: bool,
        intents: &mut Vec<UiIntent>,
    ) {
        let Some(slot) = self.settings_slot(model) else {
            intents.push(UiIntent::Domain(Intent::ActivateRelative { delta, wrap }));
            return;
        };
        let count = model.visible_ids().len() as isize;
        let current = if self.settings.open {
            slot
        } else {
            let at = model
                .selected_tab
                .and_then(|id| model.visible_ids().iter().position(|tab| *tab == id))
                .unwrap_or(0) as isize;
            at + isize::from(at >= slot)
        };
        let next = if wrap {
            (current + delta).rem_euclid(count + 1)
        } else {
            (current + delta).clamp(0, count)
        };
        self.activate_index(model, next, intents);
    }

    fn settings_slot(&mut self, model: &Model) -> Option<isize> {
        self.sidebar.ensure_rows(model, self.settings.listed);
        self.sidebar
            .settings_place
            .filter(|_| self.settings.listed)
            .map(|place| place.slot as isize)
    }

    pub(crate) fn start_rename(&mut self, model: &Model, id: TabId) {
        let Some(tab) = model.tabs.get(&id) else {
            return;
        };
        let initial = tab_name(tab, model.home.as_deref()).unwrap_or_default();
        let mut editor = TextEditor::new(&initial);
        editor.select_all();
        self.sidebar.rename = Some(InlineRename {
            id,
            initial,
            editor,
        });
        self.focused = Some(ElementId::Editor);
        self.pointer.drag = None;
        self.pointer.press = None;
        self.pointer.last_click = None;
        self.reset_caret();
        self.frame.dirty = true;
    }

    /// An untouched label stays automatic; only an edit becomes a title override.
    pub(crate) fn finish_rename(&mut self, commit: bool, intents: &mut Vec<UiIntent>) {
        let Some(rename) = self.sidebar.rename.take() else {
            return;
        };
        let title: String = rename
            .editor
            .text()
            .trim()
            .chars()
            .filter(|character| !character.is_control())
            .take(512)
            .collect();
        if commit && title != rename.initial {
            intents.push(UiIntent::Domain(Intent::RenameTab {
                id: rename.id,
                title,
            }));
        }
        self.focused = Some(ElementId::Tab(rename.id));
        self.caret.stop();
        self.frame.dirty = true;
    }
}

pub(crate) fn toggle_rail(model: &Model) -> Intent {
    Intent::SetRail(if model.settings.rail == RailMode::Expanded {
        RailMode::Collapsed
    } else {
        RailMode::Expanded
    })
}

pub(crate) fn reset_settings() -> Action {
    Action::Confirm {
        label: "Reset saved settings?".into(),
        action: Box::new(Action::Domain(Intent::ResetSettings)),
    }
}

#[cfg(test)]
#[path = "../tests/actions.rs"]
mod tests;

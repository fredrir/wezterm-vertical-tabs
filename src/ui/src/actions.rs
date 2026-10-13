use crate::SidebarUi;
use crate::element::{ElementId, EntityRef};
use crate::input::{Key, Modifiers};
use crate::intent::{HostAction, UiIntent};
use crate::overlays::{FormKind, MenuItem, Overlay};
use vtabs_core::{Intent, Model, PaneId, RailMode, SpaceId, TabId};

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
    ReplaceKeybind(usize),
    RemoveKeybind(String, String),
    EditorCommand { key: Key, target: ElementId },
    Submenu { title: String, items: Vec<MenuItem> },
    Confirm { label: String, action: Box<Action> },
    Close,
}

impl Action {
    pub(crate) fn prune(&mut self, model: &Model) -> bool {
        match self {
            Self::Submenu { items, .. } => items.retain_mut(|item| item.prune(model)),
            Self::Confirm { action, .. } => return action.prune(model),
            _ => {}
        }
        self.exists(model)
    }

    pub(crate) fn exists(&self, model: &Model) -> bool {
        match self {
            Self::Domain(
                Intent::ActivateTab(id)
                | Intent::CloseTab(id)
                | Intent::CloseOthers(id)
                | Intent::CloseBelow(id)
                | Intent::ReturnToAuto(id)
                | Intent::MoveTabToNewWindow(id)
                | Intent::RenameTab { id, .. }
                | Intent::PinTab { id, .. }
                | Intent::MoveTab { id, .. },
            )
            | Self::RenameTab(id)
            | Self::MoveTab(id)
            | Self::MoveToFolder(id)
            | Self::CloseTab(id)
            | Self::Host(HostAction::CloseTab(id) | HostAction::MoveTabToNewWindow(id)) => {
                EntityRef::Tab(*id).exists(model)
            }
            Self::Domain(Intent::AssignTab { id, space_id }) => {
                EntityRef::Tab(*id).exists(model) && EntityRef::Space(space_id).exists(model)
            }
            Self::Domain(
                Intent::SelectSpace(id)
                | Intent::ToggleSpace(id)
                | Intent::RenameSpace { id, .. }
                | Intent::EditSpace { id, .. }
                | Intent::MoveSpace { id, .. },
            )
            | Self::RenameSpace(id)
            | Self::EditSpaceIcon(id)
            | Self::EditSpaceAccent(id)
            | Self::EditSpaceRules(id)
            | Self::DeleteSpace(id) => EntityRef::Space(id).exists(model),
            Self::Domain(Intent::DeleteSpace { id, destination }) => {
                model.spaces.len() > 1
                    && EntityRef::Space(id).exists(model)
                    && destination
                        .as_ref()
                        .is_none_or(|id| EntityRef::Space(id).exists(model))
            }
            Self::Domain(
                Intent::RenameFolder { id, .. }
                | Intent::ToggleFolder(id)
                | Intent::DeleteFolder(id)
                | Intent::MoveFolder { id, .. }
                | Intent::NewTabInFolder(id),
            )
            | Self::RenameFolder(id) => EntityRef::Folder(id).exists(model),
            Self::Domain(Intent::AssignFolder { tab_id, folder_id }) => {
                EntityRef::Tab(*tab_id).exists(model)
                    && folder_id
                        .as_ref()
                        .is_none_or(|id| EntityRef::Folder(id).exists(model))
            }
            Self::KillPane(tab, pane)
            | Self::Host(
                HostAction::FocusPane(tab, pane)
                | HostAction::ClosePane(tab, pane)
                | HostAction::KillPane(tab, pane)
                | HostAction::DetachPane { tab, pane, .. },
            ) => EntityRef::Pane(*tab, *pane).exists(model),
            Self::Host(HostAction::JoinTab { source, tab }) => {
                EntityRef::Tab(*source).exists(model) && EntityRef::Tab(*tab).exists(model)
            }
            Self::EditorCommand { target, .. } => target.exists(model),
            Self::Confirm { action, .. } => action.exists(model),
            Self::Submenu { items, .. } => items.iter().any(|item| item.action.exists(model)),
            // Foreign tabs and adoptable panes are deliberately outside this model.
            _ => true,
        }
    }
}

impl SidebarUi {
    pub(crate) fn activate_element(
        &mut self,
        model: &Model,
        id: ElementId,
        intents: &mut Vec<UiIntent>,
    ) {
        if !id.exists(model) {
            return;
        }
        match id {
            ElementId::Search => intents.push(UiIntent::Host(HostAction::SearchTabs)),
            ElementId::RecordShortcut => self.toggle_shortcut_recording(),
            ElementId::Refresh => intents.push(UiIntent::Refresh),
            ElementId::CreateFolder => self.open_create_folder(),
            ElementId::Folder(id) => intents.push(UiIntent::Domain(Intent::ToggleFolder(id))),
            ElementId::SettingsCategory(category) => self.settings_select_category(category),
            ElementId::SettingsSearch => {
                self.focused = Some(ElementId::SettingsSearch);
                self.reset_caret();
                self.frame.dirty = true;
            }
            ElementId::SettingsRecord => self.toggle_settings_recording(intents),
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
                let action = match self.overlays.current() {
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
            ElementId::MenuRemove(id) => {
                let remove = match self.overlays.current() {
                    Some(Overlay::Menu(menu)) => menu
                        .items
                        .iter()
                        .find(|item| item.id == id)
                        .and_then(|item| item.remove.clone()),
                    _ => None,
                };
                if let Some(remove) = remove {
                    self.run_action(model, remove, intents);
                }
            }
            ElementId::Submit => self.submit_form(model, intents),
            ElementId::Cancel => self.back(),
            id @ (ElementId::ColorSwatch(_)
            | ElementId::ColorFormat
            | ElementId::ColorReset
            | ElementId::ColorOriginal) => self.color_action(id),
            ElementId::ColorPlane | ElementId::ColorHue => {}
            ElementId::Editor
            | ElementId::Scrollbar(_)
            | ElementId::Slider(_)
            | ElementId::SettingsEntry => {}
        }
    }

    pub(crate) fn run_action(
        &mut self,
        model: &Model,
        action: Action,
        intents: &mut Vec<UiIntent>,
    ) {
        if !action.exists(model) {
            return;
        }
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
                    self.open_color_form(
                        "Space accent",
                        FormKind::SpaceAccent(id),
                        space.accent.as_deref().unwrap_or(""),
                        None,
                        crate::theme::Theme::parse_color(&model.settings.accent)
                            .unwrap_or(self.theme.accent),
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
            Action::ReplaceKeybind(index) => self.replace_keybind(model, index),
            Action::RemoveKeybind(action, chord) => {
                self.remove_keybind(model, &action, &chord, intents)
            }
            Action::ResetSetting(key) => self.reset_setting(model, &key, intents),
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

    pub(crate) fn close_tab(&mut self, model: &Model, id: TabId, intents: &mut Vec<UiIntent>) {
        if !EntityRef::Tab(id).exists(model) {
            return;
        }
        if model.settings.confirm_close {
            intents.push(UiIntent::Host(HostAction::CloseTab(id)));
            self.dismiss();
        } else {
            self.run_action(model, Action::Domain(Intent::CloseTab(id)), intents);
        }
    }
}

pub(crate) fn toggle_rail(model: &Model) -> Intent {
    Intent::SetRail(if model.settings.rail == RailMode::Expanded {
        RailMode::Hidden
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

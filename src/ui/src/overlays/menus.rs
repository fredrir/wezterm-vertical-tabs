use crate::SidebarUi;
use crate::actions::{Action, reset_settings};
use crate::element::ElementId;
use crate::events::EditorSlot;
use crate::input::Key;
use crate::overlays::MenuItem;
use crate::settings::SettingTarget;
use vtabs_core::{Intent, Model, RailMode, TabId, settings};

impl SidebarUi {
    pub(crate) fn root_menu(&mut self, model: &Model) {
        let mut items = vec![
            MenuItem::new("new-tab", "New tab", Action::Domain(Intent::NewTab)),
            MenuItem::new("new-space", "Create space", Action::NewSpace),
            MenuItem::new("settings", "Settings", Action::Settings),
            MenuItem::new("reset-settings", "Reset settings", reset_settings()),
            MenuItem::new(
                "hide",
                "Hide sidebar",
                Action::Domain(Intent::SetRail(RailMode::Hidden)),
            ),
        ];
        items.push(MenuItem::new("new-folder", "New folder", Action::NewFolder));
        items.extend(custom_menu(&model.settings.menus, "custom"));
        self.menu("Vertical tabs", items);
    }

    pub(crate) fn context_menu(&mut self, model: &Model, id: ElementId) {
        match id {
            ElementId::Editor | ElementId::SettingsSearch => {
                let Some(editor) = self
                    .editor_slot(&id)
                    .filter(|slot| *slot != EditorSlot::Rename)
                    .and_then(|slot| self.editor(slot))
                else {
                    return;
                };
                let has_selection = editor.selection().is_some();
                let has_text = !editor.text().is_empty();
                let items = [
                    ("cut", "Cut", 'x', has_selection),
                    ("copy", "Copy", 'c', has_selection),
                    ("paste", "Paste", 'v', true),
                    ("select-all", "Select all", 'a', has_text),
                ]
                .into_iter()
                .map(|(name, label, key, enabled)| {
                    let mut item = MenuItem::new(
                        name,
                        label,
                        Action::EditorCommand {
                            key: Key::Character(key),
                            target: id.clone(),
                        },
                    );
                    item.enabled = enabled;
                    item
                })
                .collect();
                self.focused = Some(id.clone());
                self.pointer.drag = None;
                self.pointer.origin = None;
                self.push_menu("Edit text", items);
            }
            ElementId::SpaceTitle => {
                self.context_menu(model, ElementId::Space(model.selected_space.clone()))
            }
            ElementId::Tab(id)
            | ElementId::CloseTab(id)
            | ElementId::Pane(id, _)
            | ElementId::ClosePane(id, _) => {
                let Some(tab) = model.tabs.get(&id) else {
                    return;
                };
                let mut items = tab_actions(model, id);
                items.extend(custom_menu(&model.settings.menus, "custom"));
                self.menu(tab.display_title(), items);
            }
            ElementId::Folder(id) => {
                let Some(folder) = model.folders.iter().find(|f| f.id == id) else {
                    return;
                };
                let index = model
                    .selected_folders()
                    .position(|f| f.id == id)
                    .unwrap_or(0);
                self.menu(
                    &folder.name,
                    vec![
                        MenuItem::new(
                            "new-tab",
                            "New tab in folder",
                            Action::Domain(Intent::NewTabInFolder(id.clone())),
                        ),
                        MenuItem::new(
                            "toggle",
                            if folder.collapsed {
                                "Expand"
                            } else {
                                "Collapse"
                            },
                            Action::Domain(Intent::ToggleFolder(id.clone())),
                        ),
                        MenuItem::new("rename", "Rename folder", Action::RenameFolder(id.clone())),
                        MenuItem::new(
                            "up",
                            "Move up",
                            Action::Domain(Intent::MoveFolder {
                                id: id.clone(),
                                index: index.saturating_sub(1),
                            }),
                        ),
                        MenuItem::new(
                            "down",
                            "Move down",
                            Action::Domain(Intent::MoveFolder {
                                id: id.clone(),
                                index: index + 1,
                            }),
                        ),
                        MenuItem::new(
                            "ungroup",
                            "Ungroup tabs",
                            Action::Domain(Intent::DeleteFolder(id)),
                        ),
                    ],
                );
            }
            ElementId::Space(id) => {
                let Some(space) = model.spaces.iter().find(|space| space.id == id) else {
                    return;
                };
                let at = model
                    .spaces
                    .iter()
                    .position(|space| space.id == id)
                    .unwrap_or(0);
                let mut items = vec![
                    MenuItem::new(
                        "select",
                        "Select",
                        Action::Domain(Intent::SelectSpace(id.clone())),
                    ),
                    MenuItem::new("rename", "Rename", Action::RenameSpace(id.clone())),
                    MenuItem::new("icon", "Edit icon", Action::EditSpaceIcon(id.clone())),
                    MenuItem::new("accent", "Edit accent", Action::EditSpaceAccent(id.clone())),
                    MenuItem::new(
                        "rules",
                        "Edit routing rules",
                        Action::EditSpaceRules(id.clone()),
                    ),
                    MenuItem::new(
                        "up",
                        "Move up",
                        Action::Domain(Intent::MoveSpace {
                            id: id.clone(),
                            index: at.saturating_sub(1),
                        }),
                    ),
                    MenuItem::new(
                        "down",
                        "Move down",
                        Action::Domain(Intent::MoveSpace {
                            id: id.clone(),
                            index: (at + 1).min(model.spaces.len() - 1),
                        }),
                    ),
                    MenuItem::new("delete", "Delete space", Action::DeleteSpace(id)),
                ];
                items[5].enabled = at > 0;
                items[6].enabled = at + 1 < model.spaces.len();
                items[7].enabled = model.spaces.len() > 1;
                self.menu(&space.name, items);
            }
            ElementId::Setting(key) => {
                let title = SettingTarget::of(&key)
                    .descriptor()
                    .map_or(key.as_str(), |field| field.label)
                    .to_owned();
                let mut items = vec![
                    MenuItem::new("edit", "Edit", Action::EditSetting(key.clone())),
                    MenuItem::new(
                        "reset",
                        "Reset to default",
                        Action::ResetSetting(key.clone()),
                    ),
                ];
                for item in &mut items {
                    item.enabled = !self.settings.managed_in_lua(model, &key);
                }
                self.push_menu(title, items);
            }
            ElementId::SettingsTab | ElementId::CloseSettingsTab => self.menu(
                "Settings",
                vec![MenuItem::new("close", "Close", Action::CloseSettings)],
            ),
            _ if !self.overlays.is_open() => self.root_menu(model),
            _ => {}
        }
    }
}

/// Shared by the tab context menu and the tab actions launcher; digits run the first nine.
pub(crate) fn tab_actions(model: &Model, id: TabId) -> Vec<MenuItem> {
    let Some(tab) = model.tabs.get(&id) else {
        return Vec::new();
    };
    let visible = model.visible_ids();
    let below = visible
        .iter()
        .position(|other| *other == id)
        .map_or(&[][..], |at| &visible[at + 1..]);
    let closable = |tabs: &[TabId]| {
        tabs.iter()
            .any(|other| *other != id && model.tabs.get(other).is_some_and(|tab| !tab.pinned))
    };
    let mut items = vec![
        MenuItem::new("rename", "Rename", Action::RenameTab(id)),
        MenuItem::new(
            "pin",
            if tab.pinned { "Unpin" } else { "Pin" },
            Action::Domain(Intent::PinTab {
                id,
                pinned: !tab.pinned,
            }),
        ),
        MenuItem::new(
            "move-top",
            "Move to top",
            Action::Domain(Intent::MoveTab { id, index: 0 }),
        ),
        MenuItem::new(
            "move-bottom",
            "Move to bottom",
            Action::Domain(Intent::MoveTab {
                id,
                index: visible.len(),
            }),
        ),
        MenuItem::new("move-space", "Move to space ›", Action::MoveTab(id)),
        MenuItem::new("move-folder", "Move to folder ›", Action::MoveToFolder(id)),
        MenuItem::new(
            "move-window",
            "Move to new window",
            Action::Domain(Intent::MoveTabToNewWindow(id)),
        ),
        MenuItem::new("close", "Close tab", Action::CloseTab(id)),
        MenuItem::new(
            "close-below",
            "Close tabs below",
            Action::Confirm {
                label: "Close unpinned tabs below?".into(),
                action: Box::new(Action::Domain(Intent::CloseBelow(id))),
            },
        ),
        MenuItem::new(
            "close-others",
            "Close other tabs",
            Action::Confirm {
                label: "Close other unpinned tabs?".into(),
                action: Box::new(Action::Domain(Intent::CloseOthers(id))),
            },
        ),
    ];
    items[8].enabled = closable(below);
    items[9].enabled = closable(visible);
    for (digit, item) in ('1'..='9').zip(&mut items) {
        item.hint = digit.to_string();
        item.shortcuts = vec![item.hint.clone()];
    }
    items
}

fn custom_menu(entries: &[settings::MenuEntry], prefix: &str) -> Vec<MenuItem> {
    entries
        .iter()
        .map(|entry| {
            let id = format!("{prefix}/{}", entry.id);
            let action = if entry.children.is_empty() {
                let action = Action::Domain(Intent::CustomAction(
                    entry.action.clone().unwrap_or_default(),
                ));
                if entry.confirm {
                    Action::Confirm {
                        label: entry.label.clone(),
                        action: Box::new(action),
                    }
                } else {
                    action
                }
            } else {
                Action::Submenu {
                    title: entry.label.clone(),
                    items: custom_menu(&entry.children, &id),
                }
            };
            let mut item = MenuItem::new(
                id,
                format!(
                    "{}{}",
                    entry.label,
                    if entry.children.is_empty() {
                        ""
                    } else {
                        " ›"
                    }
                ),
                action,
            );
            item.enabled = !entry.children.is_empty()
                || entry
                    .action
                    .as_ref()
                    .is_some_and(|action| !action.is_empty());
            item
        })
        .collect()
}

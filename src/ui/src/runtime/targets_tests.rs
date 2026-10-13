use crate::SidebarUi;
use crate::actions::Action;
use crate::element::ElementId;
use crate::events::{DropTarget, Press};
use crate::input::{Key, Modifiers, UiInput};
use crate::overlays::{FormKind, MenuItem, Overlay};
use ratatui::layout::Rect;
use std::time::Duration;
use vtabs_core::{HostTab, Intent, Model, TabPane};

fn model() -> Model {
    let mut model = Model::default();
    model.settings.animations = false;
    model
        .reconcile(
            vec![HostTab {
                id: 1,
                panes: vec![TabPane {
                    id: 10,
                    ..TabPane::default()
                }],
                ..HostTab::default()
            }],
            Some(1),
            true,
        )
        .unwrap();
    model
        .dispatch(Intent::CreateFolder {
            name: "Work".into(),
        })
        .unwrap();
    model
}

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 80, 24), Duration::ZERO);
}

fn enter(ui: &mut SidebarUi, model: &Model) -> Vec<crate::UiIntent> {
    ui.event(
        model,
        UiInput::Key {
            key: Key::Enter,
            modifiers: Modifiers::default(),
        },
    )
}

#[test]
fn removed_folder_dismisses_rename_before_another_frame() {
    let mut model = model();
    let folder = model.folders[0].id.clone();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.focused = Some(ElementId::Folder(folder.clone()));
    ui.run_action(
        &model,
        Action::RenameFolder(folder.clone()),
        &mut Vec::new(),
    );
    assert!(ui.text_input_active());

    model.dispatch(Intent::DeleteFolder(folder)).unwrap();
    assert!(enter(&mut ui, &model).is_empty());
    assert!(!ui.has_overlay());
    assert!(!ui.text_input_active());
    assert!(ui.focused().is_none());
}

#[test]
fn removed_folder_dismisses_stashed_form_and_editor_menu() {
    let mut model = model();
    let folder = model.folders[0].id.clone();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.open_form("Rename", FormKind::RenameFolder(folder.clone()), "Work");
    ui.context_menu(&model, ElementId::Editor);
    assert!(ui.editor_menu_target().is_some());

    model.dispatch(Intent::DeleteFolder(folder)).unwrap();
    draw(&mut ui, &model);
    assert!(!ui.has_overlay());
    ui.back();
    assert!(!ui.has_overlay());
    assert!(ui.focused().is_none());
    assert!(enter(&mut ui, &model).is_empty());
}

#[test]
fn removed_folder_clears_pointer_state_and_rejects_stale_drops() {
    let mut model = model();
    let folder = model.folders[0].id.clone();
    let id = ElementId::Folder(folder.clone());
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.focused = Some(id.clone());
    ui.pointer.hovered = Some(id.clone());
    ui.pointer.drag = Some(id.clone());
    ui.pointer.dragging = true;
    ui.pointer.origin = Some((1, 1));
    ui.pointer.last_click = Some((id.clone(), Duration::ZERO));
    ui.pointer.press = Some(Press {
        id,
        down: None,
        up: None,
        level: 0.0,
        animating: false,
    });
    ui.pointer.drop = Some(DropTarget::Folder(folder.clone()));

    model
        .dispatch(Intent::DeleteFolder(folder.clone()))
        .unwrap();
    draw(&mut ui, &model);
    assert!(ui.focused().is_none());
    assert!(ui.pointer.hovered.is_none());
    assert!(ui.pointer.press.is_none());
    assert!(ui.pointer.drag.is_none());
    assert!(!ui.pointer.dragging);
    assert!(ui.pointer.origin.is_none());
    assert!(ui.pointer.last_click.is_none());
    assert!(ui.pointer.drop.is_none());

    let mut intents = Vec::new();
    ui.apply_drop(
        &model,
        ElementId::Tab(1),
        DropTarget::Folder(folder),
        &mut intents,
    );
    assert!(intents.is_empty());
}

#[test]
fn a_live_drag_stops_targeting_a_removed_folder() {
    let mut model = model();
    let folder = model.folders[0].id.clone();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.pointer.drag = Some(ElementId::Tab(1));
    ui.pointer.dragging = true;
    ui.pointer.drop = Some(DropTarget::Folder(folder.clone()));

    model.dispatch(Intent::DeleteFolder(folder)).unwrap();
    draw(&mut ui, &model);
    assert_eq!(ui.pointer.drag, Some(ElementId::Tab(1)));
    assert!(ui.pointer.dragging);
    assert!(ui.pointer.drop.is_none());
    assert!(ui.pointer.drop_motion.is_none());
}

#[test]
fn folder_menu_actions_and_nested_destinations_are_pruned() {
    let mut model = model();
    let folder = model.folders[0].id.clone();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.context_menu(&model, ElementId::Folder(folder.clone()));
    ui.push_menu(
        "Move",
        vec![MenuItem::new(
            "submenu",
            "Destinations",
            Action::Submenu {
                title: "Destinations".into(),
                items: vec![
                    MenuItem::new(
                        "folder",
                        "Work",
                        Action::Domain(Intent::AssignFolder {
                            tab_id: 1,
                            folder_id: Some(folder.clone()),
                        }),
                    ),
                    MenuItem::new(
                        "none",
                        "Ungroup",
                        Action::Domain(Intent::AssignFolder {
                            tab_id: 1,
                            folder_id: None,
                        }),
                    ),
                ],
            },
        )],
    );

    model.dispatch(Intent::DeleteFolder(folder)).unwrap();
    draw(&mut ui, &model);
    let Some(Overlay::Menu(menu)) = ui.overlays.current() else {
        panic!("expected current menu");
    };
    let Action::Submenu { items, .. } = &menu.items[0].action else {
        panic!("expected submenu");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "none");
    ui.back();
    let Some(Overlay::Menu(stashed)) = ui.overlays.current() else {
        panic!("expected stashed folder menu");
    };
    assert!(stashed.items.is_empty());
}

#[test]
fn pane_tab_and_space_targets_keep_their_existing_liveness_rules() {
    let mut model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.focused = Some(ElementId::Pane(1, 10));
    ui.confirm_close_pane(1, 10, "shell");

    let mut tab = model.tabs[&1].clone();
    tab.panes.clear();
    model.reconcile(vec![tab.into()], Some(1), true).unwrap();
    draw(&mut ui, &model);
    let Some(Overlay::Menu(menu)) = ui.overlays.current() else {
        panic!("expected confirmation");
    };
    assert_eq!(menu.items.len(), 1);
    assert_eq!(menu.items[0].id, "cancel");

    ui.dismiss();
    assert!(ui.focused().is_none());
    ui.open_form("Rename tab", FormKind::RenameTab(1), "Tab");
    model.reconcile(Vec::new(), None, true).unwrap();
    draw(&mut ui, &model);
    assert!(!ui.has_overlay());

    model
        .dispatch(Intent::CreateSpace {
            name: "Other".into(),
        })
        .unwrap();
    let space = model.selected_space.clone();
    ui.open_form(
        "Rename space",
        FormKind::RenameSpace(space.clone()),
        "Other",
    );
    model
        .dispatch(Intent::DeleteSpace {
            id: space,
            destination: None,
        })
        .unwrap();
    assert!(enter(&mut ui, &model).is_empty());
    assert!(!ui.has_overlay());
}

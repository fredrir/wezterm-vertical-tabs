use std::time::Duration;
use vtabs_core::{Intent, Model, Space, Tab};
use vtabs_ui::{ElementId, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent};

fn model() -> Model {
    let mut model = Model::default();
    model.settings.animations = false;
    model.settings.show_metadata = false;
    model.spaces.push(Space::new("work", "Work"));
    model.spaces.push(Space::new("personal", "Personal"));
    model
        .reconcile(
            [(10, "Alpha"), (20, "Beta"), (30, "Café")]
                .into_iter()
                .map(|(id, title)| Tab {
                    id,
                    title: title.into(),
                    ..Tab::default()
                })
                .collect(),
            Some(10),
            true,
        )
        .unwrap();
    model
}

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 40, 24), Duration::ZERO);
}

fn key(ui: &mut SidebarUi, model: &Model, key: Key) -> Vec<UiIntent> {
    ui.event(model, UiInput::key(key))
}

fn command(ui: &mut SidebarUi, model: &Model, character: char) -> Vec<UiIntent> {
    ui.event(
        model,
        UiInput::Key {
            key: Key::Character(character),
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::default()
            },
        },
    )
}

fn hit(ui: &SidebarUi, id: &ElementId) -> Rect {
    ui.hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap()
        .rect
}

fn focus(ui: &mut SidebarUi, model: &Model, id: &ElementId) {
    for _ in 0..ui.hit_regions().len() + 1 {
        if ui.focused() == Some(id) {
            return;
        }
        key(ui, model, Key::Tab);
    }
    panic!("Target was unreachable by Tab: {id:?}");
}

fn apply(model: &mut Model, intents: Vec<UiIntent>) {
    for intent in intents {
        if let UiIntent::Domain(intent) = intent {
            model.dispatch(intent).unwrap();
        }
    }
}

fn results(ui: &SidebarUi) -> usize {
    ui.hit_regions()
        .iter()
        .filter(|hit| matches!(&hit.id, ElementId::Menu(id) if id.starts_with("tab/")))
        .count()
}

#[test]
fn form_buttons_accept_space_and_keep_text_and_ime_focus_separate() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_create_space();
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("Research".into()));
    key(&mut ui, &model, Key::Tab);
    assert_eq!(ui.focused(), Some(&ElementId::Submit));
    ui.event(&model, UiInput::Text("ignored".into()));
    ui.event(&model, UiInput::Paste("ignored".into()));
    ui.event(
        &model,
        UiInput::ImePreedit {
            text: "かな".into(),
            cursor: None,
        },
    );
    let frame = ui
        .render(&model, Rect::new(0, 0, 40, 24), Duration::ZERO)
        .unwrap();
    assert!(frame.cursor.is_none());
    assert!(frame.ime_rect.is_none());
    assert_eq!(ui.next_deadline(), None);
    let save = hit(&ui, &ElementId::Submit);
    assert!(
        ui.rounded_surfaces()
            .iter()
            .any(|surface| surface.rect == save && surface.fill != surface.border)
    );
    assert!(
        matches!(key(&mut ui, &model, Key::Character(' ')).as_slice(),
        [UiIntent::Domain(Intent::CreateSpace { name })] if name == "Research")
    );

    ui.dismiss();
    ui.open_create_space();
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("Discard this".into()));
    ui.event(
        &model,
        UiInput::Key {
            key: Key::Tab,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        },
    );
    assert_eq!(ui.focused(), Some(&ElementId::Cancel));
    assert!(key(&mut ui, &model, Key::Character(' ')).is_empty());
    assert!(!ui.is_modal());
}

#[test]
fn form_tab_skips_buttons_that_do_not_fit() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_create_space();
    ui.render(&model, Rect::new(0, 0, 6, 3), Duration::ZERO);
    assert!(
        ui.hit_regions()
            .iter()
            .all(|hit| hit.id == ElementId::Editor)
    );
    key(&mut ui, &model, Key::Tab);
    assert_eq!(ui.focused(), Some(&ElementId::Editor));
    ui.event(&model, UiInput::Text("Tiny".into()));
    assert!(matches!(key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::CreateSpace { name })] if name == "Tiny"));
}

#[test]
fn search_cut_refilters_and_native_paste_replaces_the_selection() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("Alpha".into()));
    draw(&mut ui, &model);
    assert_eq!(results(&ui), 1);
    command(&mut ui, &model, 'a');
    assert!(
        matches!(command(&mut ui, &model, 'x').as_slice(), [UiIntent::SetClipboard(text)] if text == "Alpha")
    );
    draw(&mut ui, &model);
    assert_eq!(results(&ui), 3);
    ui.event(&model, UiInput::Text("Café".into()));
    command(&mut ui, &model, 'a');
    assert!(matches!(
        command(&mut ui, &model, 'v').as_slice(),
        [UiIntent::RequestClipboard]
    ));
    ui.event(&model, UiInput::Paste("Beta".into()));
    draw(&mut ui, &model);
    assert_eq!(results(&ui), 1);
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::ActivateTab(20))]
    ));
}

#[test]
fn moving_the_search_caret_preserves_the_selected_result() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("a".into()));
    key(&mut ui, &model, Key::Down);
    assert!(key(&mut ui, &model, Key::Left).is_empty());
    assert!(ui.is_modal());
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::ActivateTab(20))]
    ));
}

#[test]
fn search_pointer_selection_is_visible_and_escape_cancels_ime_first() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("Alpha".into()));
    draw(&mut ui, &model);
    let editor = hit(&ui, &ElementId::Editor);
    ui.event(
        &model,
        UiInput::PointerDown {
            x: editor.x,
            y: editor.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    ui.event(
        &model,
        UiInput::PointerMove {
            x: editor.x + 5,
            y: editor.y,
        },
    );
    ui.event(
        &model,
        UiInput::PointerUp {
            x: editor.x + 5,
            y: editor.y,
            button: MouseButton::Left,
        },
    );
    draw(&mut ui, &model);
    assert!(
        matches!(command(&mut ui, &model, 'c').as_slice(), [UiIntent::SetClipboard(text)] if text == "Alpha")
    );
    let selection = Rect::new(editor.x, editor.y, 5, 1);
    assert!(
        ui.rounded_surfaces()
            .iter()
            .any(|surface| surface.rect == selection)
    );
    assert_eq!(
        (editor.x..editor.x + 5)
            .map(|x| ui.buffer()[(x, editor.y)].symbol())
            .collect::<String>(),
        "Alpha"
    );
    ui.event(
        &model,
        UiInput::ImePreedit {
            text: "かな".into(),
            cursor: Some(3),
        },
    );
    let frame = ui
        .render(&model, Rect::new(0, 0, 40, 24), Duration::ZERO)
        .unwrap();
    assert!(frame.cursor.is_some());
    assert!(frame.ime_rect.is_some());
    assert_eq!(ui.next_deadline(), None);
    key(&mut ui, &model, Key::Escape);
    assert!(ui.is_modal());
    command(&mut ui, &model, 'a');
    assert!(
        matches!(command(&mut ui, &model, 'c').as_slice(), [UiIntent::SetClipboard(text)] if text == "Alpha")
    );
    key(&mut ui, &model, Key::Escape);
    assert!(!ui.is_modal());
}

#[test]
fn folder_and_space_arrows_follow_the_focused_control() {
    let mut model = model();
    model
        .dispatch(Intent::CreateFolder {
            name: "Project".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    focus(&mut ui, &model, &ElementId::Folder(folder.clone()));
    let collapse = key(&mut ui, &model, Key::Left);
    assert!(
        matches!(collapse.as_slice(), [UiIntent::Domain(Intent::ToggleFolder(id))] if id == &folder)
    );
    apply(&mut model, collapse);
    draw(&mut ui, &model);
    assert!(key(&mut ui, &model, Key::Left).is_empty());
    let expand = key(&mut ui, &model, Key::Right);
    assert!(
        matches!(expand.as_slice(), [UiIntent::Domain(Intent::ToggleFolder(id))] if id == &folder)
    );
    apply(&mut model, expand);
    draw(&mut ui, &model);
    let first_space = ElementId::Space(model.spaces[0].id.clone());
    focus(&mut ui, &model, &first_space);
    let next = key(&mut ui, &model, Key::Right);
    assert!(matches!(next.as_slice(), [UiIntent::Domain(Intent::SelectSpace(id))] if id == "work"));
    apply(&mut model, next);
    draw(&mut ui, &model);
    assert_eq!(ui.focused(), Some(&ElementId::Space("work".into())));
    assert!(
        matches!(key(&mut ui, &model, Key::Left).as_slice(), [UiIntent::Domain(Intent::SelectSpace(id))] if id == &model.spaces[0].id)
    );
}

#[test]
fn end_and_page_down_reach_tabs_below_many_empty_folders() {
    let mut model = model();
    for index in 0..20 {
        model
            .dispatch(Intent::CreateFolder {
                name: format!("Folder {index}"),
            })
            .unwrap();
    }
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    key(&mut ui, &model, Key::Home);
    draw(&mut ui, &model);
    key(&mut ui, &model, Key::PageDown);
    draw(&mut ui, &model);
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Folder(model.folders[10].id.clone()))
    );
    key(&mut ui, &model, Key::End);
    draw(&mut ui, &model);
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Tab(30))
    );
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::NewTab)
    );
}

#[test]
fn hiding_or_unfocusing_the_window_cancels_a_pending_folder_drop() {
    for cancel in [UiInput::Focus(false), UiInput::Visibility(false)] {
        let mut model = model();
        model
            .dispatch(Intent::CreateFolder {
                name: "Project".into(),
            })
            .unwrap();
        let folder = model.folders[0].id.clone();
        let mut ui = SidebarUi::new();
        draw(&mut ui, &model);
        let tab = hit(&ui, &ElementId::Tab(10));
        let folder = hit(&ui, &ElementId::Folder(folder));
        ui.event(
            &model,
            UiInput::PointerDown {
                x: tab.x,
                y: tab.y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
        );
        ui.event(
            &model,
            UiInput::PointerMove {
                x: folder.x,
                y: folder.y,
            },
        );
        ui.event(&model, cancel);
        assert!(
            ui.event(
                &model,
                UiInput::PointerUp {
                    x: folder.x,
                    y: folder.y,
                    button: MouseButton::Left
                }
            )
            .is_empty()
        );
        assert_eq!(ui.next_deadline(), None);
    }
}

#[test]
fn dragging_outside_the_native_surface_cancels_without_mutation() {
    let model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    let tab = hit(&ui, &ElementId::Tab(10));
    ui.event(
        &model,
        UiInput::PointerDown {
            x: tab.x,
            y: tab.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    assert!(
        ui.event(
            &model,
            UiInput::PointerMove {
                x: u16::MAX,
                y: u16::MAX
            }
        )
        .is_empty()
    );
    assert!(
        ui.event(
            &model,
            UiInput::PointerUp {
                x: u16::MAX,
                y: u16::MAX,
                button: MouseButton::Left
            }
        )
        .is_empty()
    );
    draw(&mut ui, &model);
    assert_eq!(ui.next_deadline(), None);
    assert_eq!(model.selected_tab, Some(10));
}

#[test]
fn folder_context_menu_creates_a_tab_in_that_folder() {
    let mut model = model();
    model
        .dispatch(Intent::CreateFolder {
            name: "Project".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    focus(&mut ui, &model, &ElementId::Folder(folder.clone()));
    key(&mut ui, &model, Key::F10);
    draw(&mut ui, &model);
    assert!(matches!(key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::NewTabInFolder(id))] if id == &folder));
}

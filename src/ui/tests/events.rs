use super::*;
use crate::*;
use std::time::Duration;
use vtabs_core::{Intent, Model, Space, Tab};

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

fn hover(ui: &mut SidebarUi, model: &Model, id: &ElementId) {
    let rect = hit(ui, id);
    ui.event(
        model,
        UiInput::PointerMove {
            x: rect.x + 1,
            y: rect.y,
        },
    );
    draw(ui, model);
}

fn click(ui: &mut SidebarUi, model: &Model, id: &ElementId) -> Vec<UiIntent> {
    let rect = hit(ui, id);
    let mut intents = ui.event(
        model,
        UiInput::PointerDown {
            x: rect.x,
            y: rect.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    intents.extend(ui.event(
        model,
        UiInput::PointerUp {
            x: rect.x,
            y: rect.y,
            button: MouseButton::Left,
        },
    ));
    intents
}

fn context(ui: &mut SidebarUi, model: &Model, id: &ElementId) {
    let rect = hit(ui, id);
    assert!(
        ui.event(
            model,
            UiInput::PointerDown {
                x: rect.x,
                y: rect.y,
                button: MouseButton::Right,
                modifiers: Modifiers::default(),
            },
        )
        .is_empty()
    );
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
fn search_cut_refilters_and_paste_replaces_the_selection() {
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
    let cursor = ui
        .render(&model, Rect::new(0, 0, 40, 24), Duration::ZERO)
        .unwrap()
        .cursor
        .unwrap();
    let editor = Rect::new(cursor.x - 5, cursor.y, 5, 1);
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
fn sidebar_arrows_are_inert_while_activation_still_follows_the_focused_control() {
    let mut model = model();
    model
        .dispatch(Intent::CreateFolder {
            name: "Project".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    let space_id = model.spaces[0].id.clone();
    let space = ElementId::Space(space_id.clone());
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    for id in [
        ElementId::Folder(folder.clone()),
        space.clone(),
        ElementId::Tab(10),
        ElementId::Rail,
    ] {
        focus(&mut ui, &model, &id);
        for arrow in [Key::Up, Key::Down, Key::Left, Key::Right] {
            assert!(key(&mut ui, &model, arrow).is_empty());
            assert_eq!(ui.focused(), Some(&id));
        }
    }
    focus(&mut ui, &model, &ElementId::Folder(folder.clone()));
    assert!(
        matches!(key(&mut ui, &model, Key::Enter).as_slice(), [UiIntent::Domain(Intent::ToggleFolder(id))] if id == &folder)
    );
    focus(&mut ui, &model, &space);
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::SelectSpace(id))] if id == &space_id
    ));
    focus(&mut ui, &model, &ElementId::Rail);
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::SetRail(
            vtabs_core::RailMode::Collapsed
        ))]
    ));
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
fn dragging_outside_the_surface_cancels_without_mutation() {
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

#[test]
fn clipboard_commands_work_in_settings_search_and_color_forms() {
    for modifiers in [
        Modifiers {
            super_key: true,
            ..Modifiers::default()
        },
        Modifiers {
            control: true,
            ..Modifiers::default()
        },
        Modifiers {
            control: true,
            shift: true,
            ..Modifiers::default()
        },
    ] {
        let mut model = model();
        model.settings.keyboard_shortcuts = false;
        let mut ui = SidebarUi::new();
        ui.set_layout(24, 0);
        ui.open_settings();
        let draw_page = |ui: &mut SidebarUi| {
            ui.render(&model, Rect::new(0, 0, 100, 35), Duration::ZERO);
        };
        draw_page(&mut ui);
        ui.event(
            &model,
            UiInput::Key {
                key: Key::Character('f'),
                modifiers,
            },
        );
        assert!(ui.text_input_active());
        ui.event(&model, UiInput::Text("accent".into()));
        ui.event(
            &model,
            UiInput::Key {
                key: Key::Character('a'),
                modifiers,
            },
        );
        assert!(
            matches!(ui.event(&model, UiInput::Key { key: Key::Character('c'), modifiers }).as_slice(),
            [UiIntent::SetClipboard(text)] if text == "accent")
        );
        draw_page(&mut ui);
        let accent = hit(&ui, &ElementId::Setting("accent".into()));
        ui.event(
            &model,
            UiInput::PointerDown {
                x: accent.x,
                y: accent.y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
        );
        ui.event(
            &model,
            UiInput::PointerUp {
                x: accent.x,
                y: accent.y,
                button: MouseButton::Left,
            },
        );
        draw_page(&mut ui);
        assert!(ui.text_input_active());
        assert_eq!(ui.focused(), Some(&ElementId::Editor));
        assert!(
            matches!(ui.event(&model, UiInput::Key { key: Key::Character('c'), modifiers }).as_slice(),
            [UiIntent::SetClipboard(text)] if text == &model.settings.accent)
        );
        assert!(matches!(
            ui.event(
                &model,
                UiInput::Key {
                    key: Key::Character('v'),
                    modifiers
                }
            )
            .as_slice(),
            [UiIntent::RequestClipboard]
        ));
        ui.event(&model, UiInput::Paste("#123456".into()));
        assert!(matches!(key(&mut ui, &model, Key::Enter).as_slice(),
            [UiIntent::Domain(Intent::SetSetting { key, value })] if key == "accent" && value == "#123456"));
    }
}

#[test]
fn clipboard_error_preserves_search_draft_and_selection_for_retry() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_tab_navigator(&model);
    ui.event(&model, UiInput::Text("Alpha".into()));
    command(&mut ui, &model, 'a');
    ui.clipboard_failed("Clipboard unavailable. Try pasting again.");
    draw(&mut ui, &model);
    assert!(!ui.text_input_active());
    key(&mut ui, &model, Key::Escape);
    draw(&mut ui, &model);
    assert!(ui.text_input_active());
    assert_eq!(results(&ui), 1);
    assert!(matches!(
        command(&mut ui, &model, 'c').as_slice(),
        [UiIntent::SetClipboard(text)] if text == "Alpha"
    ));
}

#[test]
fn editor_context_menu_preserves_form_drafts_and_applies_clipboard_actions() {
    let mut model = model();
    model.settings.keyboard_shortcuts = false;
    let mut ui = SidebarUi::new();
    ui.open_create_folder();
    ui.event(&model, UiInput::Text("Project α".into()));
    draw(&mut ui, &model);

    context(&mut ui, &model, &ElementId::Cancel);
    draw(&mut ui, &model);
    assert!(ui.text_input_active());
    context(&mut ui, &model, &ElementId::Editor);
    draw(&mut ui, &model);
    assert!(click(&mut ui, &model, &ElementId::Menu("copy".into())).is_empty());
    assert!(ui.has_overlay());
    click(&mut ui, &model, &ElementId::Menu("select-all".into()));
    draw(&mut ui, &model);
    context(&mut ui, &model, &ElementId::Editor);
    draw(&mut ui, &model);
    assert!(matches!(
        click(&mut ui, &model, &ElementId::Menu("copy".into())).as_slice(),
        [UiIntent::SetClipboard(text)] if text == "Project α"
    ));
    assert_eq!(ui.focused(), Some(&ElementId::Editor));
    assert!(ui.text_input_active());
    let frame = ui
        .render(&model, Rect::new(0, 0, 40, 24), Duration::ZERO)
        .unwrap();
    assert!(frame.cursor.is_some());
    assert!(frame.ime_rect.is_some());

    context(&mut ui, &model, &ElementId::Editor);
    draw(&mut ui, &model);
    assert!(matches!(
        click(&mut ui, &model, &ElementId::Menu("cut".into())).as_slice(),
        [UiIntent::SetClipboard(text)] if text == "Project α"
    ));
    draw(&mut ui, &model);
    context(&mut ui, &model, &ElementId::Editor);
    draw(&mut ui, &model);
    assert!(matches!(
        click(&mut ui, &model, &ElementId::Menu("paste".into())).as_slice(),
        [UiIntent::RequestClipboard]
    ));
    ui.event(&model, UiInput::Paste("Project α".into()));
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::CreateFolder { name })] if name == "Project α"
    ));
}

#[test]
fn dismissing_editor_context_menu_retains_search_query_selection_and_results() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_tab_navigator(&model);
    ui.event(&model, UiInput::Text("a".into()));
    key(&mut ui, &model, Key::Down);
    command(&mut ui, &model, 'a');
    draw(&mut ui, &model);

    for outside in [false, true] {
        context(&mut ui, &model, &ElementId::Editor);
        draw(&mut ui, &model);
        if outside {
            ui.event(
                &model,
                UiInput::PointerDown {
                    x: 0,
                    y: 0,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                },
            );
        } else {
            key(&mut ui, &model, Key::Escape);
        }
        draw(&mut ui, &model);
        assert_eq!(results(&ui), 3);
        assert!(matches!(
            command(&mut ui, &model, 'c').as_slice(),
            [UiIntent::SetClipboard(text)] if text == "a"
        ));
    }
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::ActivateTab(20))]
    ));
}

#[test]
fn settings_context_menu_restores_search_focus_for_keyboard_cut_and_async_paste() {
    let mut model = model();
    model.settings.keyboard_shortcuts = false;
    let mut ui = SidebarUi::new();
    ui.set_layout(24, 0);
    ui.open_settings();
    let draw_page = |ui: &mut SidebarUi| {
        ui.render(&model, Rect::new(0, 0, 100, 35), Duration::ZERO);
    };
    command(&mut ui, &model, 'f');
    ui.event(&model, UiInput::Text("accent".into()));
    command(&mut ui, &model, 'a');
    draw_page(&mut ui);
    context(&mut ui, &model, &ElementId::SettingsSearch);
    draw_page(&mut ui);
    assert!(ui.text_input_active());
    assert!(matches!(
        command(&mut ui, &model, 'x').as_slice(),
        [UiIntent::SetClipboard(text)] if text == "accent"
    ));
    draw_page(&mut ui);
    assert_eq!(ui.focused(), Some(&ElementId::SettingsSearch));
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Setting("width".into()))
    );
    key(&mut ui, &model, Key::F10);
    draw_page(&mut ui);
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::RequestClipboard]
    ));
    ui.event(&model, UiInput::Paste("accent".into()));
    draw_page(&mut ui);
    assert!(ui.content_page());
    assert!(!ui.has_overlay());
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Setting("accent".into()))
    );
    assert!(
        !ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Setting("width".into()))
    );
}

#[test]
fn closing_a_tab_defers_the_running_process_check_to_the_host() {
    use crate::HostAction;
    let mut model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    assert!(
        ui.hit_regions()
            .iter()
            .all(|hit| hit.id != ElementId::CloseTab(10))
    );
    hover(&mut ui, &model, &ElementId::Tab(10));
    assert!(matches!(
        click(&mut ui, &model, &ElementId::CloseTab(10)).as_slice(),
        [UiIntent::Host(HostAction::CloseTab(10))]
    ));
    assert!(!ui.has_overlay());
    draw(&mut ui, &model);
    context(&mut ui, &model, &ElementId::Tab(20));
    draw(&mut ui, &model);
    assert!(matches!(
        click(&mut ui, &model, &ElementId::Menu("close".into())).as_slice(),
        [UiIntent::Host(HostAction::CloseTab(20))]
    ));
    assert!(!ui.has_overlay());
    draw(&mut ui, &model);
    focus(&mut ui, &model, &ElementId::Tab(30));
    assert!(matches!(
        key(&mut ui, &model, Key::Delete).as_slice(),
        [UiIntent::Host(HostAction::CloseTab(30))]
    ));
    assert!(matches!(
        command(&mut ui, &model, 'w').as_slice(),
        [UiIntent::Host(HostAction::CloseTab(10))]
    ));

    model.settings.confirm_close = false;
    model.revision += 1;
    draw(&mut ui, &model);
    hover(&mut ui, &model, &ElementId::Tab(10));
    assert!(matches!(
        click(&mut ui, &model, &ElementId::CloseTab(10)).as_slice(),
        [UiIntent::Domain(Intent::CloseTab(10))]
    ));
}

#[test]
fn host_close_prompt_names_the_process_and_defaults_to_closing() {
    let model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.confirm_close_tab(10, "vim");
    assert!(ui.is_modal());
    draw(&mut ui, &model);
    let text: String = ui
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Close tab?"), "{text}");
    assert!(text.contains("vim is still running."), "{text}");
    let (cancel, confirm) = (
        hit(&ui, &ElementId::Menu("cancel".into())),
        hit(&ui, &ElementId::Menu("confirm".into())),
    );
    assert_eq!((cancel.y, cancel.height), (confirm.y, 2));
    assert!(cancel.right() < confirm.x);
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::CloseTab(10))]
    ));
    assert!(!ui.is_modal());

    ui.confirm_close_tab(10, "vim");
    draw(&mut ui, &model);
    key(&mut ui, &model, Key::Left);
    assert!(
        key(&mut ui, &model, Key::Enter).is_empty(),
        "Cancel was selected"
    );
    assert!(!ui.is_modal());

    ui.confirm_close_pane(10, 77, "cargo");
    draw(&mut ui, &model);
    let text: String = ui
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Close split?") && text.contains("cargo is still running."));
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Host(HostAction::KillPane(10, 77))]
    ));

    ui.confirm_close_tab(20, "");
    draw(&mut ui, &model);
    let text: String = ui
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("A process is still running."), "{text}");
    assert!(key(&mut ui, &model, Key::Escape).is_empty());
    assert!(!ui.is_modal());
}

#[test]
fn folder_chord_is_no_longer_a_shortcut_but_the_button_still_creates_folders() {
    let model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    let chord = |shift: bool| Modifiers {
        super_key: true,
        shift,
        ..Modifiers::default()
    };
    for (character, shift) in [('g', false), ('G', true)] {
        assert!(!is_shortcut(&Key::Character(character), chord(shift)));
        assert!(
            ui.event(
                &model,
                UiInput::Key {
                    key: Key::Character(character),
                    modifiers: chord(shift),
                },
            )
            .is_empty()
        );
        draw(&mut ui, &model);
        assert!(!ui.has_overlay());
        assert!(!ui.text_input_active());
    }
    draw(&mut ui, &model);
    hover(&mut ui, &model, &ElementId::SpaceTitle);
    click(&mut ui, &model, &ElementId::CreateFolder);
    draw(&mut ui, &model);
    assert!(ui.has_overlay());
    assert!(ui.text_input_active());
}

fn palette(ui: &SidebarUi) -> Vec<(String, Option<usize>, String)> {
    let Some(Overlay::Menu(menu)) = &ui.overlays.current else {
        panic!("tab search is not open");
    };
    menu.items
        .iter()
        .map(|item| (item.label.clone(), item.index, item.hint.clone()))
        .collect()
}

#[test]
fn search_lists_the_current_space_first_then_every_other_space_and_hidden_tabs() {
    let mut model = model();
    model
        .dispatch(Intent::AssignTab {
            id: 20,
            space_id: "work".into(),
        })
        .unwrap();
    model.apply_filter_hook(30, false).unwrap();
    let mut ui = SidebarUi::new();
    ui.open_tab_navigator(&model);
    assert_eq!(
        palette(&ui),
        [
            ("Alpha".into(), Some(1), String::new()),
            ("Café".into(), None, "Home · hidden".into()),
            ("Beta".into(), None, "Work".into()),
        ]
    );
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("work".into()));
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Domain(Intent::ActivateTab(20))]
    ));
}

#[test]
fn search_reaches_tabs_in_other_windows_by_name_or_title() {
    let model = model();
    let mut ui = SidebarUi::new();
    let tab = Tab {
        id: 99,
        title: "nvim".into(),
        title_override: Some("Editor".into()),
        ..Tab::default()
    };
    ui.set_foreign_tabs(vec![ForeignTab::new(7, &tab, None, "Work · window 2")]);
    ui.open_tab_navigator(&model);
    assert_eq!(
        palette(&ui).last(),
        Some(&("Editor".into(), None, "Work · window 2".into()))
    );
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("nvim".into()));
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Host(HostAction::ShowTab { window: 7, tab: 99 })]
    ));
}

#[test]
fn enter_on_a_split_close_button_left_focused_beside_settings_never_edits_a_setting() {
    let mut model = model();
    model.set_home(Some("/home/me".into()));
    let pane = |id, cwd: &str| vtabs_core::TabPane {
        id,
        cwd: cwd.into(),
        active: id == 1,
        ..Default::default()
    };
    model
        .reconcile(
            vec![Tab {
                id: 10,
                panes: vec![pane(1, "/home/me/api"), pane(2, "/home/me/web")],
                ..Tab::default()
            }],
            Some(10),
            true,
        )
        .unwrap();
    let area = Rect::new(0, 0, 120, 40);
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    ui.open_settings();
    ui.render(&model, area, Duration::ZERO);
    let pane = hit(&ui, &ElementId::Pane(10, 2));
    ui.event(
        &model,
        UiInput::PointerMove {
            x: pane.x + 1,
            y: pane.y,
        },
    );
    ui.render(&model, area, Duration::ZERO);
    let close = hit(&ui, &ElementId::ClosePane(10, 2));
    ui.event(
        &model,
        UiInput::PointerDown {
            x: close.x,
            y: close.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    for (x, y) in [(close.x, close.y + 6), (0, area.bottom() - 4)] {
        ui.event(&model, UiInput::PointerMove { x, y });
    }
    ui.event(
        &model,
        UiInput::PointerUp {
            x: 0,
            y: area.bottom() - 4,
            button: MouseButton::Left,
        },
    );
    assert_eq!(ui.focused(), Some(&ElementId::ClosePane(10, 2)));

    let intents = key(&mut ui, &model, Key::Enter);
    assert!(
        !intents
            .iter()
            .any(|intent| matches!(intent, UiIntent::Domain(Intent::SetSetting { .. }))),
        "{intents:?}"
    );
    assert!(
        matches!(
            intents.as_slice(),
            [UiIntent::Host(
                HostAction::ClosePane(10, 2) | HostAction::KillPane(10, 2)
            )]
        ),
        "{intents:?}"
    );
}

#[test]
fn cutting_form_text_clears_its_error_and_restarts_the_caret_like_typing() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_create_space();
    draw(&mut ui, &model);
    ui.event(&model, UiInput::Text("   ".into()));
    key(&mut ui, &model, Key::Enter);
    command(&mut ui, &model, 'a');
    let has_error = |ui: &SidebarUi| matches!(&ui.overlays.current, Some(overlays::Overlay::Form(form)) if form.error.is_some());
    assert!(has_error(&ui));
    ui.render(&model, Rect::new(0, 0, 40, 24), Duration::from_millis(700));
    assert!(!ui.caret.visible);

    let intents = command(&mut ui, &model, 'x');
    assert!(matches!(
        intents.as_slice(),
        [UiIntent::SetClipboard(text)] if text == "   "
    ));
    assert!(!has_error(&ui));
    assert!(ui.caret.visible);
}

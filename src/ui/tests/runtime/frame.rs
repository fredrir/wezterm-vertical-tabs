use crate::*;
use std::time::Duration;
use vtabs_core::{Model, Tab};

const AREA: Rect = Rect::new(0, 0, 44, 34);

fn model() -> Model {
    let mut model = Model::default();
    model.settings.animations = false;
    model.set_home(Some("/home/me".into()));
    model
        .reconcile(
            vec![
                Tab {
                    id: 1,
                    cwd: "/home/me/notes".into(),
                    ..Tab::default()
                },
                Tab {
                    id: 2,
                    cwd: "/home/me/api".into(),
                    ..Tab::default()
                },
            ],
            Some(1),
            true,
        )
        .unwrap();
    model
}

fn draw(ui: &mut SidebarUi, model: &Model, millis: u64) -> FrameUpdate {
    ui.invalidate();
    ui.render(model, AREA, Duration::from_millis(millis))
        .expect("an invalidated surface publishes a frame")
}

fn hit(ui: &SidebarUi, id: &ElementId) -> Rect {
    ui.hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap_or_else(|| panic!("{id:?} is not reachable"))
        .rect
}

fn click(ui: &mut SidebarUi, model: &Model, (x, y): (u16, u16)) {
    ui.event(
        model,
        UiInput::PointerDown {
            x,
            y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    ui.event(
        model,
        UiInput::PointerUp {
            x,
            y,
            button: MouseButton::Left,
        },
    );
}

fn start_rename(ui: &mut SidebarUi, model: &Model, tab: u64) {
    let rect = hit(ui, &ElementId::Tab(tab));
    let title = (rect.x + rect.width / 2, rect.y);
    click(ui, model, title);
    click(ui, model, title);
}

#[test]
fn a_modal_over_an_inline_rename_publishes_no_caret() {
    let model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, 0);
    start_rename(&mut ui, &model, 2);
    let frame = draw(&mut ui, &model, 0);
    assert!(frame.cursor.is_some());
    assert!(frame.ime_rect.is_some());
    assert_eq!(frame.cursor_shift, 0.5);

    ui.confirm_close_tab(2, "vim");
    let frame = draw(&mut ui, &model, 0);
    assert_eq!(frame.cursor, None);
    assert_eq!(frame.ime_rect, None);
    assert_eq!(frame.cursor_shift, 0.0);
}

#[test]
fn turning_animation_length_to_zero_cancels_a_running_fade() {
    let mut model = model();
    model.settings.animations = true;
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, 0);
    let tab = hit(&ui, &ElementId::Tab(2));
    ui.event(&model, UiInput::PointerMove { x: tab.x, y: tab.y });
    draw(&mut ui, &model, 16);
    assert!(ui.has_animation());

    model.settings.animation_ms = 0;
    model.revision += 1;
    draw(&mut ui, &model, 32);
    assert!(!ui.has_animation());
    assert_eq!(ui.next_deadline(), None);
}

#[test]
fn a_rename_beside_open_settings_keeps_its_own_pointer_columns_and_ime() {
    let model = model();
    let area = Rect::new(0, 0, 120, 40);
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    ui.render(&model, area, Duration::ZERO);
    start_rename(&mut ui, &model, 2);
    ui.event(
        &model,
        UiInput::Key {
            key: Key::Character(','),
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::default()
            },
        },
    );
    ui.render(&model, area, Duration::ZERO);
    assert!(ui.content_page());
    let field = hit(&ui, &ElementId::Editor);
    let search = hit(&ui, &ElementId::SettingsSearch);
    assert_ne!(field.y, search.y);

    click(&mut ui, &model, (field.x + 3, field.y));
    ui.event(
        &model,
        UiInput::ImePreedit {
            text: "か".into(),
            cursor: Some(3),
        },
    );
    ui.invalidate();
    let frame = ui.render(&model, area, Duration::ZERO).unwrap();
    let rename = ui.sidebar.rename.as_ref().unwrap();
    assert_eq!(rename.editor.display_text(), "~/aかpi");
    let ime = frame.ime_rect.unwrap();
    assert_eq!((ime.x, ime.y), (field.x + 5, field.y));
    assert_eq!(
        frame.cursor,
        Some(ratatui::layout::Position::new(ime.x, ime.y))
    );
    assert_eq!(frame.cursor_shift, 0.5);
}

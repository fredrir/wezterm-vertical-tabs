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

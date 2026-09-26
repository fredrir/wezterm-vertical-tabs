use super::*;
use std::fmt::Write;
use std::time::Duration;
use vtabs_core::jobs::JobTarget;
use vtabs_core::{Intent, Model, RailMode, Side, Space, Tab, TabPane};

const SIDEBAR: Rect = Rect::new(0, 0, 44, 34);
const WINDOW: Rect = Rect::new(0, 0, 120, 40);

fn pane(id: u64, cwd: &str, active: bool, place: (u16, u16, u16, u16)) -> TabPane {
    let (left, top, width, height) = place;
    TabPane {
        id,
        cwd: cwd.into(),
        active,
        left,
        top,
        width,
        height,
        ..Default::default()
    }
}

fn tab(id: u64, cwd: &str) -> Tab {
    Tab {
        id,
        title: format!("zsh {id}"),
        cwd: cwd.into(),
        domain: "local".into(),
        ..Tab::default()
    }
}

fn fixture() -> Model {
    let mut model = Model::default();
    model.settings.animations = false;
    model.set_home(Some("/home/me".into()));
    let mut work = Space::new("work", "Work");
    work.accent = Some("#f5a97f".into());
    model.spaces.push(work);
    model.spaces.push(Space::new("personal", "Personal"));
    let mut remote_pane = pane(9, "/srv/logs", false, (41, 13, 40, 11));
    remote_pane.remote = true;
    remote_pane.os = "ubuntu".into();
    let split = Tab {
        panes: vec![
            pane(7, "/home/me/api", false, (0, 0, 40, 24)),
            pane(8, "/home/me/web", true, (41, 0, 40, 12)),
            remote_pane,
        ],
        ..tab(4, "/home/me/api")
    };
    let remote = Tab {
        remote: true,
        os: "arch".into(),
        domain: "SSH:archie".into(),
        ..tab(5, "/home/me/dotfiles")
    };
    let renamed = Tab {
        title_override: Some("Release notes".into()),
        ..tab(6, "/home/me/notes")
    };
    model
        .reconcile(
            vec![
                tab(1, "/home/me/journal"),
                tab(2, "/home/me/docs/guide"),
                tab(3, "/home/me/docs/api"),
                split,
                remote,
                renamed,
                tab(10, "/home/me/elsewhere"),
            ],
            Some(4),
            true,
        )
        .unwrap();
    model
        .dispatch(Intent::PinTab {
            id: 1,
            pinned: true,
        })
        .unwrap();
    model
        .dispatch(Intent::CreateFolder {
            name: "Docs".into(),
        })
        .unwrap();
    let folder = model.folders[0].id.clone();
    for id in [2, 3] {
        model
            .dispatch(Intent::AssignFolder {
                tab_id: id,
                folder_id: Some(folder.clone()),
            })
            .unwrap();
    }
    model
        .dispatch(Intent::AssignTab {
            id: 10,
            space_id: "work".into(),
        })
        .unwrap();
    model.footer = "main ✓ 2 changes".into();
    model
}

fn portable(tooltip: &str) -> String {
    let command = format!("{} ", icons::COMMAND);
    tooltip
        .replace(&format!("⇧{command}"), "<cmd>")
        .replace(&command, "<cmd>")
        .replace("Ctrl+Shift+", "<cmd>")
        .replace('⌥', "<opt>")
}

fn draw(ui: &mut SidebarUi, model: &Model, area: Rect, millis: u64) -> FrameUpdate {
    ui.invalidate();
    ui.render(model, area, Duration::from_millis(millis))
        .expect("an invalidated surface publishes a frame")
}

fn snapshot(ui: &SidebarUi, frame: &FrameUpdate) -> String {
    let mut out = format!("{:?}\nsurfaces:\n", ui.buffer());
    for surface in ui.rounded_surfaces() {
        writeln!(out, "  {surface:?}").unwrap();
    }
    out.push_str("hits:\n");
    for hit in ui.hit_regions() {
        writeln!(
            out,
            "  {:?} {:?} {:?}",
            hit.id,
            hit.rect,
            portable(&hit.tooltip)
        )
        .unwrap();
    }
    writeln!(
        out,
        "cursor: {:?}\ncursor_shift: {}\nime_rect: {:?}\ntransform: {:?}\nfocused: {:?}",
        frame.cursor,
        frame.cursor_shift,
        frame.ime_rect,
        frame.transform,
        ui.focused()
    )
    .unwrap();
    writeln!(
        out,
        "modal: {} overlay_surface: {} content_page: {} text_input: {}",
        ui.is_modal(),
        ui.overlay_surface(),
        ui.content_page(),
        ui.text_input_active()
    )
    .unwrap();
    out
}

fn hit(ui: &SidebarUi, id: &ElementId) -> Rect {
    ui.hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap_or_else(|| panic!("{id:?} is not reachable"))
        .rect
}

fn center(rect: Rect) -> (u16, u16) {
    (rect.x + rect.width / 2, rect.y)
}

fn down(ui: &mut SidebarUi, model: &Model, (x, y): (u16, u16), button: MouseButton) {
    ui.event(
        model,
        UiInput::PointerDown {
            x,
            y,
            button,
            modifiers: Modifiers::default(),
        },
    );
}

fn up(ui: &mut SidebarUi, model: &Model, (x, y): (u16, u16)) {
    ui.event(
        model,
        UiInput::PointerUp {
            x,
            y,
            button: MouseButton::Left,
        },
    );
}

fn hover(ui: &mut SidebarUi, model: &Model, (x, y): (u16, u16)) {
    ui.event(model, UiInput::PointerMove { x, y });
}

fn hover_on(ui: &mut SidebarUi, model: &Model, id: &ElementId) {
    let at = center(hit(ui, id));
    hover(ui, model, at);
}

fn down_on(ui: &mut SidebarUi, model: &Model, id: &ElementId, button: MouseButton) {
    let at = center(hit(ui, id));
    down(ui, model, at, button);
}

fn click_on(ui: &mut SidebarUi, model: &Model, id: &ElementId) {
    let at = center(hit(ui, id));
    down(ui, model, at, MouseButton::Left);
    up(ui, model, at);
}

fn key(ui: &mut SidebarUi, model: &Model, key: Key, modifiers: Modifiers) {
    ui.event(model, UiInput::Key { key, modifiers });
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

fn job(pane: u64, command: &str, suspended: bool, ready: bool) -> JobEntry {
    JobEntry {
        target: JobTarget {
            pane,
            shell: 100,
            number: pane as u32,
            pid: 4000 + pane as u32,
        },
        command: command.into(),
        place: "~/api".into(),
        suspended,
        ready,
    }
}

fn assert_screen(name: &str, ui: &SidebarUi, frame: &FrameUpdate) {
    insta::assert_snapshot!(name, snapshot(ui, frame));
}

#[test]
fn sidebar_cards() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("sidebar_cards", &ui, &frame);
}

#[test]
fn sidebar_metadata_on_the_right() {
    let mut model = fixture();
    model.settings.show_metadata = true;
    model.settings.side = Side::Right;
    model.settings.show_indexes = false;
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 3);
    let frame = draw(&mut ui, &model, Rect::new(0, 0, 60, 34), 0);
    assert_screen("sidebar_metadata_on_the_right", &ui, &frame);
}

#[test]
fn sidebar_compact_rail() {
    let mut model = fixture();
    model.settings.rail = RailMode::Collapsed;
    let mut ui = SidebarUi::new();
    let frame = draw(&mut ui, &model, Rect::new(0, 0, 8, 30), 0);
    assert_screen("sidebar_compact_rail", &ui, &frame);
}

#[test]
fn sidebar_single_lines_scrolled() {
    let mut model = fixture();
    model.settings.cards = false;
    model.settings.show_close = false;
    let area = Rect::new(0, 0, 30, 12);
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, area, 0);
    ui.event(
        &model,
        UiInput::Scroll {
            x: 4,
            y: 6,
            rows: 3,
        },
    );
    let frame = draw(&mut ui, &model, area, 0);
    assert_screen("sidebar_single_lines_scrolled", &ui, &frame);
}

#[test]
fn private_window_title() {
    let mut model = fixture();
    model.private = true;
    let mut ui = SidebarUi::new();
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("private_window_title", &ui, &frame);
}

#[test]
fn hovered_tab_row_and_space() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, SIDEBAR, 0);
    hover_on(&mut ui, &model, &ElementId::Tab(6));
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("hovered_tab_row", &ui, &frame);
    hover_on(&mut ui, &model, &ElementId::Space("work".into()));
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("hovered_space", &ui, &frame);
}

#[test]
fn hovered_split_pane() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, SIDEBAR, 0);
    hover_on(&mut ui, &model, &ElementId::Pane(4, 7));
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("hovered_split_pane", &ui, &frame);
}

#[test]
fn pressed_tab_mid_animation() {
    let mut model = fixture();
    model.settings.animations = true;
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, SIDEBAR, 0);
    down_on(&mut ui, &model, &ElementId::Tab(6), MouseButton::Left);
    draw(&mut ui, &model, SIDEBAR, 10);
    let frame = draw(&mut ui, &model, SIDEBAR, 40);
    assert_screen("pressed_tab_mid_animation", &ui, &frame);
}

#[test]
fn hover_effect_mid_fade() {
    let mut model = fixture();
    model.settings.animations = true;
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, SIDEBAR, 0);
    hover_on(&mut ui, &model, &ElementId::Tab(5));
    draw(&mut ui, &model, SIDEBAR, 16);
    let frame = draw(&mut ui, &model, SIDEBAR, 64);
    assert_screen("hover_effect_mid_fade", &ui, &frame);
}

#[test]
fn dragging_beside_and_into_a_tab() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, SIDEBAR, 0);
    let source = hit(&ui, &ElementId::Tab(6));
    let target = hit(&ui, &ElementId::Tab(5));
    down(&mut ui, &model, center(source), MouseButton::Left);
    ui.set_pointer_fraction(0.5, 0.1);
    hover(&mut ui, &model, (target.x + 4, target.y));
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("dragging_beside_a_tab", &ui, &frame);
    ui.set_pointer_fraction(0.5, 0.9);
    hover(&mut ui, &model, (target.x + 5, target.y));
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("dragging_into_a_tab", &ui, &frame);
}

#[test]
fn inline_rename_selects_the_title() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model, SIDEBAR, 0);
    let title = center(hit(&ui, &ElementId::Tab(6)));
    down(&mut ui, &model, title, MouseButton::Left);
    up(&mut ui, &model, title);
    down(&mut ui, &model, title, MouseButton::Left);
    up(&mut ui, &model, title);
    let frame = draw(&mut ui, &model, SIDEBAR, 0);
    assert_screen("inline_rename", &ui, &frame);
}

#[test]
fn tab_context_menu() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    draw(&mut ui, &model, WINDOW, 0);
    down_on(&mut ui, &model, &ElementId::Tab(5), MouseButton::Right);
    key(&mut ui, &model, Key::Down, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("tab_context_menu", &ui, &frame);
}

#[test]
fn root_menu_from_keyboard() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    draw(&mut ui, &model, WINDOW, 0);
    key(&mut ui, &model, Key::F10, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("root_menu", &ui, &frame);
}

#[test]
fn confirm_close_dialog() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    draw(&mut ui, &model, WINDOW, 0);
    ui.confirm_close_tab(5, "vim");
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("confirm_close_dialog", &ui, &frame);
    hover_on(&mut ui, &model, &ElementId::Menu("cancel".into()));
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("confirm_close_dialog_hovered_cancel", &ui, &frame);
}

#[test]
fn error_message_menu() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    draw(&mut ui, &model, WINDOW, 0);
    ui.show_error("Could not save the configuration");
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("error_message_menu", &ui, &frame);
}

#[test]
fn forms_show_errors_and_focus_buttons() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    draw(&mut ui, &model, WINDOW, 0);
    ui.open_create_space();
    key(&mut ui, &model, Key::Enter, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("form_error", &ui, &frame);
    ui.event(&model, UiInput::Text("Research 界".into()));
    key(&mut ui, &model, Key::Left, shift());
    key(&mut ui, &model, Key::Left, shift());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("form_selection", &ui, &frame);
    key(&mut ui, &model, Key::Tab, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("form_save_focused", &ui, &frame);
}

#[test]
fn tab_palette_filters_rows() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    let foreign = Tab {
        title: "htop".into(),
        ..tab(77, "/home/me")
    };
    ui.set_foreign_tabs(vec![ForeignTab::new(
        2,
        &foreign,
        Some("/home/me"),
        "Window 2",
    )]);
    draw(&mut ui, &model, WINDOW, 0);
    ui.open_tab_navigator(&model);
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("tab_palette", &ui, &frame);
    ui.event(&model, UiInput::Text("e".into()));
    key(&mut ui, &model, Key::Down, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("tab_palette_filtered", &ui, &frame);
    key(&mut ui, &model, Key::F10, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("tab_palette_edit_menu", &ui, &frame);
}

#[test]
fn job_palette_and_actions() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    draw(&mut ui, &model, WINDOW, 0);
    ui.open_jobs(Vec::new());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("job_palette_empty", &ui, &frame);
    ui.open_jobs(vec![
        job(7, "cargo watch -x test", false, true),
        job(8, "vim notes.md", true, true),
        job(9, "sleep 60", true, false),
    ]);
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("job_palette", &ui, &frame);
    key(&mut ui, &model, Key::F10, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("job_actions", &ui, &frame);
}

#[test]
fn settings_page_wide() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    ui.open_settings();
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("settings_page_wide", &ui, &frame);
    key(&mut ui, &model, Key::Down, Modifiers::default());
    key(&mut ui, &model, Key::Down, Modifiers::default());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("settings_page_selected_row", &ui, &frame);
    hover_on(
        &mut ui,
        &model,
        &ElementId::SettingsCategory("theme".into()),
    );
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("settings_page_hovered_chip", &ui, &frame);
}

#[test]
fn settings_search_selection() {
    let mut model = fixture();
    model.config_owned.insert("cards".into());
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    ui.open_settings();
    draw(&mut ui, &model, WINDOW, 0);
    click_on(&mut ui, &model, &ElementId::SettingsSearch);
    ui.event(&model, UiInput::Text("car".into()));
    key(&mut ui, &model, Key::Left, shift());
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("settings_search_selection", &ui, &frame);
}

#[test]
fn settings_page_small_and_categories() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(20, 0);
    ui.open_settings();
    let area = Rect::new(0, 0, 50, 14);
    draw(&mut ui, &model, area, 0);
    click_on(
        &mut ui,
        &model,
        &ElementId::SettingsCategory("layout".into()),
    );
    let frame = draw(&mut ui, &model, area, 0);
    assert_screen("settings_page_small", &ui, &frame);
}

#[test]
fn setting_context_menu_over_the_page() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(40, 0);
    ui.open_settings();
    draw(&mut ui, &model, WINDOW, 0);
    down_on(
        &mut ui,
        &model,
        &ElementId::Setting("rail".into()),
        MouseButton::Right,
    );
    let frame = draw(&mut ui, &model, WINDOW, 0);
    assert_screen("setting_context_menu", &ui, &frame);
}

#[test]
fn tooltips_beside_the_sidebar() {
    let model = fixture();
    let mut ui = SidebarUi::new();
    ui.set_layout(44, 0);
    draw(&mut ui, &model, WINDOW, 0);
    let folder = model.folders[0].id.clone();
    hover_on(&mut ui, &model, &ElementId::Folder(folder));
    draw(&mut ui, &model, WINDOW, 0);
    let frame = draw(&mut ui, &model, WINDOW, 700);
    assert_screen("tooltip_multiline", &ui, &frame);
    hover_on(&mut ui, &model, &ElementId::Space("personal".into()));
    draw(&mut ui, &model, WINDOW, 700);
    let frame = draw(&mut ui, &model, WINDOW, 1400);
    assert_screen("tooltip_single_line", &ui, &frame);
}

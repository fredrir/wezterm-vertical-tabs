use super::*;

#[test]
fn confirm_close_closes_a_tab_the_mux_does_not_report_busy_without_prompting() {
    // The process-wide mux is shared with other tests; an absent or unknown tab is idle.
    config::designate_this_as_the_main_thread();
    let mut adapter = Adapter::new(9860);
    adapter
        .app
        .update(app::HostSnapshot {
            revision: 1,
            tabs: vec![core::Tab {
                id: 1,
                title: "Shell".into(),
                ..Default::default()
            }],
            active_tab: Some(1),
            metrics: app::Metrics::default(),
            focused: true,
            configuration_epoch: 0,
        })
        .unwrap();
    adapter.render(
        Geometry {
            sidebar: Bounds {
                x: 0.,
                y: 0.,
                width: 256.,
                height: 480.,
            },
            content: Bounds {
                x: 256.,
                y: 0.,
                width: 800.,
                height: 480.,
            },
            cell_width: 8.,
            cell_height: 20.,
            dpi: 96.,
            ..Geometry::default()
        },
        Instant::now(),
    );
    let close = adapter
        .app
        .ui()
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ui::ElementId::CloseTab(1))
        .expect("selected tab shows its close control")
        .rect;
    for input in [
        ui::UiInput::PointerDown {
            x: close.x,
            y: close.y,
            button: ui::MouseButton::Left,
            modifiers: ui::Modifiers::default(),
        },
        ui::UiInput::PointerUp {
            x: close.x,
            y: close.y,
            button: ui::MouseButton::Left,
        },
    ] {
        adapter.ui_input(input);
    }
    assert!(!adapter.app.ui().has_overlay());
    assert!(matches!(
        adapter.commands().as_slice(),
        [Command::Close(1, false)]
    ));
}

#[test]
fn form_completion_publishes_settled_grid_and_pointer_origin() {
    config::designate_this_as_the_main_thread();
    for right in [false, true] {
        let mut adapter = Adapter::new(1);
        adapter
            .app
            .config(serde_json::json!({"settings":{
                "animations":false,"side":if right {"right"} else {"left"}
            }}))
            .unwrap();
        let geometry = Geometry {
            sidebar: Bounds {
                x: if right { 1140. } else { 0. },
                y: 0.,
                width: 256.,
                height: 920.,
            },
            content: Bounds {
                x: if right { 0. } else { 256. },
                y: 0.,
                width: 1140.,
                height: 920.,
            },
            cell_width: 10.,
            cell_height: 22.,
            dpi: 96.,
            ..Geometry::default()
        };
        adapter.render(geometry, Instant::now());
        adapter.app.open_create_space();
        adapter.render(geometry, Instant::now());
        assert_eq!(adapter.surface.columns, 139);
        adapter.ui_input(ui::UiInput::Text("Complete".into()));
        adapter.ui_input(ui::UiInput::Key {
            key: ui::Key::Enter,
            modifiers: ui::Modifiers::default(),
        });
        assert!(adapter.overlay_surface());
        adapter.render(geometry, Instant::now());
        assert!(!adapter.overlay_surface());
        assert_eq!(adapter.surface.columns, 25);
        assert_eq!(adapter.app.buffer().area.width, 25);
        assert_eq!(adapter.surface.offset, (0., 0.));
        assert_eq!(adapter.inspect()["grid"]["x"], geometry.sidebar.x);
        assert!(
            adapter
                .app
                .ui()
                .hit_regions()
                .iter()
                .all(|hit| hit.rect.right() <= 25)
        );
    }
}

#[test]
fn hidden_and_collapsed_modals_use_window_bounds_without_resizing_the_rail() {
    // Adapter construction reads the GUI's current Lua configuration. Initialize
    // the same thread-local host context as wezterm-gui startup before exercising it.
    config::designate_this_as_the_main_thread();
    for rail in ["hidden", "collapsed"] {
        for action in ["tab_navigator", "settings", "create_space"] {
            let mut adapter = Adapter::new(1);
            adapter
                .app
                .config(serde_json::json!({"settings":{
                    "rail":rail,"width":256,"rail_width":48
                }}))
                .unwrap();
            let saved = serde_json::to_value(&adapter.app.model().settings).unwrap();
            let original = adapter.reservation().width;
            if action == "tab_navigator" {
                adapter.navigation(Navigation::Navigator);
            } else {
                adapter.message(serde_json::json!({"action":action}));
            }
            assert_eq!(adapter.reservation().width, original);
            assert!(adapter.keyboard_focus());
            let mut geometry = Geometry::default();
            geometry.sidebar.width = adapter.reservation().width;
            geometry.sidebar.height = 480.;
            geometry.content = Bounds {
                x: original,
                y: 0.,
                width: 800.,
                height: 480.,
            };
            geometry.cell_width = 8.;
            geometry.cell_height = 20.;
            geometry.dpi = 96.;
            adapter.render(geometry, Instant::now());
            assert_eq!(adapter.surface.columns, ((original + 800.) / 8.) as usize);
            assert_eq!(adapter.content_page(), action == "settings");
            assert_eq!(adapter.overlay_surface(), action != "settings");
            let inspected = adapter.inspect();
            assert_eq!(inspected["overlay_surface"], action != "settings");
            assert_eq!(inspected["grid"]["x"], 0.);
            assert_eq!(inspected["grid"]["y"], 0.);
            assert_eq!(inspected["grid"]["columns"], adapter.surface.columns);
            if action != "settings" {
                let caret = adapter.caret().expect("editor has an IME rectangle");
                let editor = adapter
                    .app
                    .ui()
                    .hit_regions()
                    .iter()
                    .find(|hit| hit.id == ui::ElementId::Editor)
                    .expect("editor hit region")
                    .rect;
                assert!(editor.contains(ratatui::layout::Position::new(
                    caret.0 as u16,
                    caret.1 as u16
                )));
                // Dialogs center on the window; the launcher drops down from the rail.
                if action == "create_space" {
                    assert!(caret.0 > (original / 8.) as usize);
                }
            }
            assert!(!adapter.surface.rows.is_empty());
            assert_eq!(adapter.surface.offset, (0., 0.));
            assert_eq!(adapter.surface.opacity, 1.);
            let text = adapter
                .app
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            let title = match action {
                "settings" => "Settings",
                "create_space" => "Create space",
                _ => "⌕",
            };
            assert!(text.contains(title), "modal was not composed: {}", text);
            let escape = window::KeyEvent {
                key: KeyCode::Char('\u{1b}'),
                modifiers: window::Modifiers::NONE,
                leds: Default::default(),
                repeat_count: 1,
                key_is_down: true,
                raw: None,
                #[cfg(windows)]
                win32_uni_char: None,
            };
            assert!(adapter.input(Input::Key(&escape)));
            assert!(!adapter.app.is_modal());
            assert!(!adapter.keyboard_focus());
            assert_eq!(adapter.reservation().width, original);
            adapter.render(geometry, Instant::now());
            assert_eq!(adapter.surface.columns, (original / 8.) as usize);
            assert_eq!(
                serde_json::to_value(&adapter.app.model().settings).unwrap(),
                saved
            );
        }
    }
}

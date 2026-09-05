use std::time::Duration;
use vtabs_core::{Intent, Model, Space, Tab};
use vtabs_ui::{ElementId, Key, Modifiers, MouseButton, Rect, SidebarUi, UiInput, UiIntent};

fn model() -> Model {
    let mut model = Model::default();
    model.spaces.push(Space::new("empty", "Empty"));
    model.spaces.push(Space::new("work", "Work"));
    model
        .reconcile(
            vec![
                Tab {
                    id: 10,
                    title: "Editor 界 🚀".into(),
                    cwd: "~/projects".into(),
                    domain: "local".into(),
                    ..Tab::default()
                },
                Tab {
                    id: 20,
                    title: "Café\u{301}".into(),
                    cwd: "~/work".into(),
                    domain: "SSH".into(),
                    ..Tab::default()
                },
            ],
            Some(10),
            true,
        )
        .unwrap();
    model
}
fn draw(ui: &mut SidebarUi, model: &Model, millis: u64) {
    ui.render(
        model,
        Rect::new(0, 0, 32, 24),
        Duration::from_millis(millis),
    );
}
fn click(ui: &mut SidebarUi, model: &Model, id: &ElementId) -> Vec<UiIntent> {
    let hit = ui
        .hit_regions()
        .iter()
        .find(|hit| &hit.id == id)
        .unwrap()
        .clone();
    ui.event(
        model,
        UiInput::PointerDown {
            x: hit.rect.x,
            y: hit.rect.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    ui.event(
        model,
        UiInput::PointerUp {
            x: hit.rect.x,
            y: hit.rect.y,
            button: MouseButton::Left,
        },
    )
}
fn domain(intents: Vec<UiIntent>) -> Vec<Intent> {
    intents
        .into_iter()
        .filter_map(|intent| {
            if let UiIntent::Domain(intent) = intent {
                Some(intent)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn unchanged_native_paints_reuse_complete_buffer_without_deadlines() {
    let mut ui = SidebarUi::new();
    let model = model();
    let frame = ui
        .render(&model, Rect::new(0, 0, 32, 24), Duration::ZERO)
        .unwrap();
    assert!(frame.resized);
    assert_eq!(frame.dirty_rows.len(), 24);
    let pointer = ui.buffer().content.as_ptr();
    assert!(
        ui.render(&model, Rect::new(0, 0, 32, 24), Duration::from_secs(5))
            .is_none()
    );
    assert_eq!(pointer, ui.buffer().content.as_ptr());
    assert_eq!(ui.next_deadline(), None);
}

#[test]
fn private_tabs_explain_shared_spaces_and_settings_without_truncating_tooltip() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    model.private = true;
    draw(&mut ui, &model, 0);
    let header = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::PrivateInfo)
        .unwrap();
    ui.event(
        &model,
        UiInput::PointerMove {
            x: header.rect.x,
            y: header.rect.y,
        },
    );
    draw(&mut ui, &model, 1000);
    let text = ui
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    for expected in ["Private tabs", "memory.", "Spaces", "settings", "shared."] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(
        ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::CreateSpace)
    );
}

#[test]
fn create_space_is_reachable_on_every_nonempty_grid() {
    let model = model();
    for width in 1..=12 {
        for height in 1..=12 {
            let mut ui = SidebarUi::new();
            ui.render(&model, Rect::new(0, 0, width, height), Duration::ZERO);
            assert!(
                ui.hit_regions()
                    .iter()
                    .any(|hit| hit.id == ElementId::CreateSpace),
                "{width}×{height}"
            );
            click(&mut ui, &model, &ElementId::CreateSpace);
            assert!(ui.is_modal());
            ui.render(
                &model,
                Rect::new(0, 0, width, height),
                Duration::from_millis(1),
            );
            ui.event(&model, UiInput::Text("New".into()));
            assert_eq!(
                domain(ui.event(&model, UiInput::key(Key::Enter))),
                vec![Intent::CreateSpace { name: "New".into() }]
            );
        }
    }
}

#[test]
fn empty_spaces_are_selectable_without_implicitly_creating_tabs() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    draw(&mut ui, &model, 0);
    let intents = domain(click(&mut ui, &model, &ElementId::Space("empty".into())));
    assert_eq!(intents, vec![Intent::SelectSpace("empty".into())]);
    let transition = model.dispatch(intents[0].clone()).unwrap();
    assert!(transition.commands.is_empty());
    draw(&mut ui, &model, 1);
    assert!(
        !ui.hit_regions()
            .iter()
            .any(|hit| matches!(hit.id, ElementId::Tab(_)))
    );
    assert_eq!(
        domain(click(&mut ui, &model, &ElementId::NewTab)),
        vec![Intent::NewTab]
    );
    assert_eq!(model.tabs.len(), 2);
}

#[test]
fn native_tab_navigator_uses_stable_visible_ids() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    model
        .dispatch(Intent::AssignTab {
            id: 20,
            space_id: "work".into(),
        })
        .unwrap();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model, 0);
    assert!(ui.has_focus());
    assert_eq!(
        domain(ui.event(&model, UiInput::key(Key::Enter))),
        vec![Intent::ActivateTab(10)]
    );
    ui.release_focus();
    assert!(!ui.has_focus());
}

#[test]
fn native_navigator_recovers_hidden_sidebar_without_overwriting_lua_preferences() {
    for rail in [
        vtabs_core::RailMode::Hidden,
        vtabs_core::RailMode::Collapsed,
    ] {
        for owned in [false, true] {
            let mut ui = SidebarUi::new();
            let mut model = model();
            model.settings.rail = rail;
            if owned {
                model.config_owned.insert("rail".into());
            }
            ui.open_tab_navigator(&model);
            assert!(!ui.needs_expanded_space());
            assert!(ui.overlay_surface());
            draw(&mut ui, &model, 0);
            let intents = domain(click(
                &mut ui,
                &model,
                &ElementId::Menu("sidebar/expand".into()),
            ));
            assert_eq!(model.settings.rail, rail);
            if owned {
                assert!(intents.is_empty());
                let text = ui
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(text.contains("Rail controlled by Lua"));
                assert!(ui.is_modal());
            } else {
                assert_eq!(
                    intents,
                    vec![Intent::SetRail(vtabs_core::RailMode::Expanded)]
                );
                model.dispatch(intents[0].clone()).unwrap();
                assert_eq!(model.settings.rail, vtabs_core::RailMode::Expanded);
                assert!(!ui.needs_expanded_space());
            }
        }
    }
}

#[test]
fn navigator_filters_without_changing_tab_identity_or_underlying_projection() {
    let mut ui = SidebarUi::new();
    let model = model();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model, 0);
    ui.event(&model, UiInput::Text("Café".into()));
    draw(&mut ui, &model, 1);
    assert_eq!(
        domain(ui.event(&model, UiInput::key(Key::Enter))),
        vec![Intent::ActivateTab(20)]
    );
    assert_eq!(model.visible_ids(), &[10, 20]);
}

#[test]
fn rejected_domain_form_keeps_text_and_successful_revision_dismisses_it() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    ui.open_create_space();
    draw(&mut ui, &model, 0);
    ui.event(&model, UiInput::Text("Example".into()));
    let intent = domain(ui.event(&model, UiInput::key(Key::Enter))).remove(0);
    assert!(ui.is_modal());
    ui.set_error("Database is not part of this validation test");
    draw(&mut ui, &model, 1);
    assert!(ui.is_modal());
    assert_eq!(
        domain(ui.event(&model, UiInput::key(Key::Enter))),
        vec![intent.clone()]
    );
    model.dispatch(intent).unwrap();
    draw(&mut ui, &model, 2);
    assert!(!ui.is_modal());
}

#[test]
fn rail_commit_animates_only_surface_then_goes_idle() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    draw(&mut ui, &model, 0);
    model
        .dispatch(Intent::SetRail(vtabs_core::RailMode::Collapsed))
        .unwrap();
    let frame = ui
        .render(&model, Rect::new(0, 0, 5, 24), Duration::from_millis(1))
        .unwrap();
    assert!(frame.resized);
    assert!(frame.transform.translate_x < 0.0);
    let frame = ui
        .render(&model, Rect::new(0, 0, 5, 24), Duration::from_millis(200))
        .unwrap();
    assert_eq!(frame.transform.translate_x, 0.0);
    assert!(!ui.has_animation());
    assert_eq!(ui.next_deadline(), None);
}

#[test]
fn resize_and_unicode_replacement_publish_complete_latest_cells() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    draw(&mut ui, &model, 0);
    let previous = ui.buffer().clone();
    model
        .dispatch(Intent::RenameTab {
            id: 10,
            title: "a".into(),
        })
        .unwrap();
    let frame = ui
        .render(&model, Rect::new(0, 0, 32, 24), Duration::from_millis(1))
        .unwrap();
    assert!(!frame.resized);
    assert!(!frame.changed_cells.is_empty());
    let mut replay = previous;
    for (x, y) in frame.changed_cells {
        replay[(x, y)] = ui.buffer()[(x, y)].clone();
    }
    // Wide-cell continuation state is represented by the complete final buffer; changed
    // rows contain no old CJK/emoji cell after replacement.
    for row in frame.dirty_rows {
        for col in 0..32 {
            assert_eq!(
                replay[(col, row)].symbol(),
                ui.buffer()[(col, row)].symbol()
            );
        }
    }
    for (width, height) in [(1, 1), (80, 100), (7, 3), (32, 24)] {
        let frame = ui
            .render(
                &model,
                Rect::new(0, 0, width, height),
                Duration::from_millis(2),
            )
            .unwrap();
        assert!(frame.resized);
        assert_eq!(ui.buffer().area, Rect::new(0, 0, width, height));
        assert_eq!(
            frame.changed_cells.len(),
            usize::from(width) * usize::from(height)
        );
    }
}

#[test]
fn finite_tachyon_effect_stops_and_reduced_motion_cancels_it() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    draw(&mut ui, &model, 0);
    let intents = domain(click(&mut ui, &model, &ElementId::Tab(20)));
    assert!(ui.has_animation());
    for intent in intents {
        model.dispatch(intent).unwrap();
    }
    for t in (8..=200).step_by(8) {
        draw(&mut ui, &model, t);
    }
    assert!(!ui.has_animation());
    assert_eq!(ui.next_deadline(), None);
    click(&mut ui, &model, &ElementId::Tab(10));
    assert!(ui.has_animation());
    model
        .dispatch(Intent::SetSetting {
            key: "reduced_motion".into(),
            value: true.into(),
        })
        .unwrap();
    draw(&mut ui, &model, 201);
    assert!(!ui.has_animation());
    assert_eq!(ui.next_deadline(), None);
}

#[test]
fn resize_cancels_surface_motion_and_cell_effect_together() {
    let mut ui = SidebarUi::new();
    let model = model();
    draw(&mut ui, &model, 0);
    click(&mut ui, &model, &ElementId::Tab(20));
    ui.transition_surface(-1.0, 0.0, Duration::ZERO, Duration::from_millis(140));
    ui.render(&model, Rect::new(0, 0, 40, 30), Duration::from_millis(8));
    assert!(!ui.has_animation());
    assert_eq!(ui.next_deadline(), None);
}

#[test]
fn editor_ime_caret_remains_reported_when_blink_hides_cursor() {
    let mut ui = SidebarUi::new();
    let model = model();
    ui.open_create_space();
    draw(&mut ui, &model, 0);
    ui.event(&model, UiInput::Text("漢字".into()));
    ui.event(
        &model,
        UiInput::ImePreedit {
            text: "かな".into(),
            cursor: Some(3),
        },
    );
    let frame = ui
        .render(&model, Rect::new(0, 0, 32, 24), Duration::from_millis(1))
        .unwrap();
    assert_eq!(frame.ime_rect.unwrap().width, 1);
    assert!(frame.cursor.is_some());
    let frame = ui
        .render(&model, Rect::new(0, 0, 32, 24), Duration::from_millis(601))
        .unwrap();
    assert!(frame.cursor.is_none());
    assert!(frame.ime_rect.is_some());
    ui.event(&model, UiInput::Focus(false));
    assert_eq!(ui.next_deadline(), None);
}

#[test]
fn settings_validate_values_and_show_config_ownership() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    model.config_owned.insert("width".into());
    model.revision += 1;
    ui.open_settings();
    draw(&mut ui, &model, 0);
    assert!(click(&mut ui, &model, &ElementId::Setting("width".into())).is_empty());
    assert!(
        !ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Editor)
    );
    model.config_owned.clear();
    model.revision += 1;
    draw(&mut ui, &model, 1);
    click(&mut ui, &model, &ElementId::Setting("width".into()));
    draw(&mut ui, &model, 2);
    ui.event(&model, UiInput::Text("0".into()));
    assert!(ui.event(&model, UiInput::key(Key::Enter)).is_empty());
    assert!(ui.is_modal());
    ui.event(
        &model,
        UiInput::Key {
            key: Key::Character('a'),
            modifiers: Modifiers {
                control: true,
                ..Modifiers::default()
            },
        },
    );
    ui.event(&model, UiInput::Text("320".into()));
    assert_eq!(
        domain(ui.event(&model, UiInput::key(Key::Enter))),
        vec![Intent::SetSetting {
            key: "width".into(),
            value: 320.into()
        }]
    );
}

#[test]
fn nonempty_space_deletion_requires_explicit_destination_and_confirmation() {
    let mut ui = SidebarUi::new();
    let model = model();
    draw(&mut ui, &model, 0);
    let hit = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Space("home".into()))
        .unwrap()
        .clone();
    ui.event(
        &model,
        UiInput::PointerDown {
            x: hit.rect.x,
            y: hit.rect.y,
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        },
    );
    draw(&mut ui, &model, 1);
    assert!(click(&mut ui, &model, &ElementId::Menu("delete".into())).is_empty());
    draw(&mut ui, &model, 2);
    assert!(click(&mut ui, &model, &ElementId::Menu("work".into())).is_empty());
    draw(&mut ui, &model, 3);
    assert_eq!(
        domain(click(&mut ui, &model, &ElementId::Menu("confirm".into()))),
        vec![Intent::DeleteSpace {
            id: "home".into(),
            destination: Some("work".into())
        }]
    );
}

#[test]
fn dragged_tab_is_assigned_by_id_and_does_not_emit_a_pane_operation() {
    let mut ui = SidebarUi::new();
    let model = model();
    draw(&mut ui, &model, 0);
    let from = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Tab(20))
        .unwrap()
        .rect;
    let to = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Space("work".into()))
        .unwrap()
        .rect;
    ui.event(
        &model,
        UiInput::PointerDown {
            x: from.x,
            y: from.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    assert_eq!(
        domain(ui.event(
            &model,
            UiInput::PointerUp {
                x: to.x,
                y: to.y,
                button: MouseButton::Left
            }
        )),
        vec![Intent::AssignTab {
            id: 20,
            space_id: "work".into()
        }]
    );
}

#[test]
fn concurrent_close_removes_navigator_targets_before_paint_and_input() {
    let mut ui = SidebarUi::new();
    let mut model = model();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model, 0);
    let survivor = model.tabs.get(&20).unwrap().clone();
    model.reconcile(vec![survivor], Some(20), true).unwrap();
    draw(&mut ui, &model, 1);
    assert!(
        !ui.hit_regions()
            .iter()
            .any(|hit| hit.id == ElementId::Menu("tab/10".into()))
    );
    assert_eq!(
        domain(ui.event(&model, UiInput::key(Key::Enter))),
        vec![Intent::ActivateTab(20)]
    );
}

#[test]
fn text_can_be_selected_by_pointer_drag_and_copied() {
    let mut ui = SidebarUi::new();
    let model = model();
    ui.open_create_space();
    draw(&mut ui, &model, 0);
    ui.event(&model, UiInput::Text("ab界d".into()));
    draw(&mut ui, &model, 1);
    let edit = ui
        .hit_regions()
        .iter()
        .find(|hit| hit.id == ElementId::Editor)
        .unwrap()
        .rect;
    ui.event(
        &model,
        UiInput::PointerDown {
            x: edit.x + 1,
            y: edit.y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    ui.event(
        &model,
        UiInput::PointerMove {
            x: edit.x + 4,
            y: edit.y,
        },
    );
    ui.event(
        &model,
        UiInput::PointerUp {
            x: edit.x + 4,
            y: edit.y,
            button: MouseButton::Left,
        },
    );
    let result = ui.event(
        &model,
        UiInput::Key {
            key: Key::Character('c'),
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::default()
            },
        },
    );
    assert!(matches!(&result[0],UiIntent::SetClipboard(text) if text=="b界"));
}

#[test]
fn search_ime_anchor_remains_available_for_no_matches_without_idle_tick() {
    let mut ui = SidebarUi::new();
    let model = model();
    ui.open_tab_navigator(&model);
    draw(&mut ui, &model, 0);
    ui.event(&model, UiInput::Text("missing".into()));
    ui.event(
        &model,
        UiInput::ImePreedit {
            text: "かな".into(),
            cursor: Some(3),
        },
    );
    let frame = ui
        .render(&model, Rect::new(0, 0, 32, 24), Duration::from_millis(1))
        .unwrap();
    assert!(frame.ime_rect.is_some());
    assert!(frame.cursor.is_some());
    assert_eq!(ui.next_deadline(), None);
    assert!(ui.event(&model, UiInput::key(Key::Enter)).is_empty());
}

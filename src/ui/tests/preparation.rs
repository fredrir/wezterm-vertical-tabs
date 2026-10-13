use crate::SidebarUi;
use crate::element::ElementId;
use crate::input::{Key, Modifiers, UiInput};
use crate::overlays::Overlay;
use crate::runtime::motion;
use ratatui::layout::Rect;
use std::time::Duration;
use vtabs_core::{HostTab, Intent, Model};

fn model() -> Model {
    let mut model = Model::default();
    model.settings.animations = false;
    model
        .reconcile(
            vec![HostTab {
                id: 1,
                title: "Shell".into(),
                ..HostTab::default()
            }],
            Some(1),
            true,
        )
        .unwrap();
    model
}

fn draw(ui: &mut SidebarUi, model: &Model) {
    ui.render(model, Rect::new(0, 0, 80, 24), Duration::ZERO);
}

#[test]
fn successful_form_settles_before_measurement_and_prepare_is_idempotent() {
    let mut model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.focused = Some(ElementId::Tab(1));
    ui.open_create_space();
    ui.event(&model, UiInput::Text("Work".into()));
    let intents = ui.event(
        &model,
        UiInput::Key {
            key: Key::Enter,
            modifiers: Modifiers::default(),
        },
    );
    let [crate::UiIntent::Domain(intent)] = intents.as_slice() else {
        panic!("expected form submission")
    };
    model.dispatch(intent.clone()).unwrap();
    assert!(ui.overlay_surface());

    ui.prepare(&model, Duration::ZERO);
    assert!(!ui.overlay_surface());
    assert_eq!(ui.focused(), Some(&ElementId::Tab(1)));
    ui.prepare(&model, Duration::ZERO);
    assert!(!ui.overlay_surface());
    assert_eq!(ui.focused(), Some(&ElementId::Tab(1)));
    assert!(
        ui.render(&model, Rect::new(0, 0, 25, 24), Duration::ZERO)
            .is_some()
    );
    assert!(!ui.overlay_surface());
}

#[test]
fn failed_form_is_kept_through_unrelated_model_updates() {
    let mut model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.open_create_space();
    ui.event(&model, UiInput::Text("Work".into()));
    assert_eq!(
        ui.event(
            &model,
            UiInput::Key {
                key: Key::Enter,
                modifiers: Modifiers::default()
            }
        )
        .len(),
        1
    );
    ui.set_error("Could not save");
    model
        .dispatch(Intent::RenameTab {
            id: 1,
            title: "Updated".into(),
        })
        .unwrap();
    ui.prepare(&model, Duration::ZERO);
    let Some(Overlay::Form(form)) = ui.overlays.current() else {
        panic!("expected editable form")
    };
    assert_eq!(form.editor.text(), "Work");
    assert_eq!(form.error.as_deref(), Some("Could not save"));
}

#[test]
fn settings_drafts_are_canceled_when_an_overlay_takes_input() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    draw(&mut ui, &model);
    ui.start_entry(&model, "width", Some('4'));
    assert!(ui.settings.editing.has_entry());
    ui.open_create_space();
    assert!(!ui.settings.editing.has_entry());
    ui.dismiss();
    let mut intents = Vec::new();
    ui.commit_entry(&mut intents);
    assert!(intents.is_empty());

    ui.settings.editing.preview_slider("width", 400.);
    ui.open_create_space();
    assert!(ui.settings.editing.commit_slider("width").is_none());
    ui.dismiss();
    ui.toggle_settings_recording(&mut intents);
    assert!(ui.settings.editing.recording());
    ui.hide_settings();
    assert!(!ui.settings.editing.recording());
}

#[test]
fn settings_entry_commits_once_and_cancel_drops_the_draft() {
    let model = model();
    let mut ui = SidebarUi::new();
    ui.open_settings();
    ui.start_entry(&model, "width", Some('4'));
    ui.event(&model, UiInput::Text("00".into()));
    let mut intents = Vec::new();
    ui.commit_entry(&mut intents);
    ui.commit_entry(&mut intents);
    assert!(
        matches!(intents.as_slice(), [crate::UiIntent::Domain(Intent::SetSetting { key, value })] if key == "width" && value == &serde_json::json!(400))
    );

    ui.start_entry(&model, "width", Some('5'));
    ui.event(
        &model,
        UiInput::Key {
            key: Key::Escape,
            modifiers: Modifiers::default(),
        },
    );
    ui.commit_entry(&mut intents);
    assert_eq!(intents.len(), 1);
}

#[test]
fn metadata_updates_preserve_a_visible_tooltip() {
    let mut model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.pointer.hovered = Some(ElementId::Tab(1));
    ui.tooltip.arm(Some(Duration::ZERO));
    ui.prepare(&model, Duration::ZERO);
    draw(&mut ui, &model);
    assert!(ui.tooltip.shown);

    model
        .dispatch(Intent::RenameTab {
            id: 1,
            title: "New title".into(),
        })
        .unwrap();
    ui.prepare(&model, Duration::ZERO);
    draw(&mut ui, &model);
    assert!(ui.tooltip.shown);
    assert!(ui.overlay_surface());
}

#[test]
fn orphaned_tooltip_releases_its_surface_at_the_next_prepare() {
    let mut model = model();
    let mut ui = SidebarUi::new();
    draw(&mut ui, &model);
    ui.pointer.hovered = Some(ElementId::Tab(1));
    ui.tooltip.arm(Some(Duration::ZERO));
    ui.prepare(&model, Duration::ZERO);
    draw(&mut ui, &model);
    assert!(ui.tooltip.shown);

    // The tab still exists, but a different selected space removes its anchor.
    model
        .dispatch(Intent::CreateSpace {
            name: "Other".into(),
        })
        .unwrap();
    ui.prepare(&model, Duration::ZERO);
    assert!(ui.overlay_surface());
    draw(&mut ui, &model);
    assert!(
        ui.overlay_surface(),
        "the published frame keeps its chosen surface"
    );
    assert_eq!(ui.next_deadline(), Some(Duration::ZERO));
    ui.prepare(&model, Duration::ZERO);
    assert!(!ui.overlay_surface());
    assert!(!ui.tooltip.pending());
    ui.prepare(&model, motion::FRAME);
    assert!(!ui.overlay_surface());
}

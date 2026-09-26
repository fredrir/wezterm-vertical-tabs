use super::*;
use crate::*;

fn job(pane: u64) -> JobEntry {
    JobEntry {
        target: JobTarget {
            pane,
            shell: 123,
            number: 1,
            pid: 456,
        },
        command: "sleep 60".into(),
        place: format!("host-{pane}"),
        suspended: true,
        ready: true,
    }
}

fn key(ui: &mut SidebarUi, model: &Model, key: Key) -> Vec<UiIntent> {
    ui.event(model, UiInput::key(key))
}

#[test]
fn command_z_requests_jobs_without_intercepting_control_z() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    let command = Modifiers {
        super_key: true,
        ..Default::default()
    };
    assert!(matches!(
        ui.event(
            &model,
            UiInput::Key {
                key: Key::Character('z'),
                modifiers: command
            }
        )
        .as_slice(),
        [UiIntent::Host(HostAction::OpenJobs)]
    ));
    assert!(!is_shortcut(
        &Key::Character('z'),
        Modifiers {
            control: true,
            ..Default::default()
        }
    ));
    let mut disabled = model;
    disabled.settings.keyboard_shortcuts = false;
    assert!(
        ui.event(
            &disabled,
            UiInput::Key {
                key: Key::Character('z'),
                modifiers: command
            }
        )
        .is_empty()
    );
}

#[test]
fn identical_job_numbers_on_different_hosts_keep_their_owning_panes() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_jobs(vec![job(7), job(8)]);
    ui.event(&model, UiInput::Text("host-8".into()));
    ui.render(&model, Rect::new(0, 0, 100, 30), Duration::ZERO);
    let intents = key(&mut ui, &model, Key::Enter);
    assert!(
        matches!(intents.as_slice(), [UiIntent::Host(HostAction::Job(target, JobOperation::Foreground))] if *target == job(8).target)
    );
    assert!(!ui.jobs_open());
}

#[test]
fn refreshing_jobs_keeps_the_query_and_selected_identity() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_jobs(vec![job(7), job(8)]);
    ui.event(&model, UiInput::Text("sleep".into()));
    key(&mut ui, &model, Key::Down);
    ui.set_jobs(vec![
        JobEntry {
            command: "unrelated".into(),
            ..job(9)
        },
        job(8),
        job(7),
    ]);
    let Some(Overlay::Menu(menu)) = &ui.overlays.current else {
        panic!("launcher missing")
    };
    assert_eq!(menu.search.as_ref().unwrap().editor.text(), "sleep");
    assert_eq!(menu.items.len(), 2);
    let intents = key(&mut ui, &model, Key::Enter);
    assert!(
        matches!(intents.as_slice(), [UiIntent::Host(HostAction::Job(target, JobOperation::Foreground))] if target.pane == 8)
    );
}

#[test]
fn job_actions_resume_in_background_and_confirm_termination() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_jobs(vec![job(7)]);
    key(&mut ui, &model, Key::F10);
    key(&mut ui, &model, Key::Down);
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Host(HostAction::Job(_, JobOperation::Background))]
    ));

    ui.open_jobs(vec![job(7)]);
    ui.render(&model, Rect::new(0, 0, 100, 30), Duration::ZERO);
    let rect = ui
        .hit_regions()
        .iter()
        .find(|hit| matches!(hit.id, ElementId::Menu(_)))
        .unwrap()
        .rect;
    ui.event(
        &model,
        UiInput::PointerDown {
            x: rect.x,
            y: rect.y,
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        },
    );
    key(&mut ui, &model, Key::End);
    assert!(key(&mut ui, &model, Key::Enter).is_empty());
    assert!(matches!(
        key(&mut ui, &model, Key::Enter).as_slice(),
        [UiIntent::Host(HostAction::Job(_, JobOperation::Terminate))]
    ));
}

#[test]
fn busy_or_removed_jobs_cannot_be_opened() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_jobs(vec![JobEntry {
        ready: false,
        ..job(7)
    }]);
    assert!(key(&mut ui, &model, Key::Enter).is_empty());
    ui.set_jobs(Vec::new());
    assert!(key(&mut ui, &model, Key::Enter).is_empty());
    key(&mut ui, &model, Key::Escape);
    assert!(!ui.is_modal());
}

#[test]
fn switching_launchers_closes_job_actions_and_their_parent() {
    let model = Model::default();
    let mut ui = SidebarUi::new();
    ui.open_jobs(vec![job(7)]);
    key(&mut ui, &model, Key::F10);
    assert!(ui.jobs_open());
    ui.open_tab_navigator(&model);
    assert!(!ui.jobs_open());
    key(&mut ui, &model, Key::Escape);
    assert!(!ui.is_modal());
}

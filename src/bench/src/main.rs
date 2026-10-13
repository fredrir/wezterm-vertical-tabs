use divan::{AllocProfiler, Bencher};
use vtabs_core::{Folder, HostTab, LaunchSpec, Model};

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

fn main() {
    divan::Divan::from_args().run_benches();
}

fn fixture(count: usize, folder_count: usize) -> (Model, Vec<HostTab>) {
    let tabs: Vec<_> = (1..=count)
        .map(|id| HostTab {
            id: id as u64,
            title: format!("Terminal {id}"),
            cwd: format!("/home/user/projects/project-{}", id % 20),
            domain: "local".into(),
            host: "workstation".into(),
            user: "user".into(),
            process: "zsh".into(),
            ..HostTab::default()
        })
        .collect();
    let mut model = Model::default();
    model.settings.animations = false;
    model.reconcile(tabs.clone(), Some(1), true).unwrap();
    for tab in model.tabs.values_mut() {
        tab.launch = Some(LaunchSpec {
            domain: Some("local".into()),
            cwd: Some(tab.cwd.clone()),
            args: vec!["zsh".into(), "-lc".into(), "exec zsh".into()],
            env: (0..8)
                .map(|n| (format!("PROJECT_ENV_{n}"), "project-value".repeat(4)))
                .collect(),
        });
        tab.title_override = Some(format!("Project {}", tab.id));
        tab.title_hook = Some(format!("Shell {}", tab.id));
        if folder_count > 0 && tab.id % 5 != 0 {
            tab.folder_id = Some(format!("folder-{}", tab.id as usize % folder_count));
        }
    }
    model
        .load_folders(
            (0..folder_count)
                .map(|id| Folder {
                    id: format!("folder-{id}"),
                    name: format!("Project {id}"),
                    space_id: "home".into(),
                    collapsed: false,
                })
                .collect(),
        )
        .unwrap();
    (model, tabs)
}

mod core {
    use super::*;
    use vtabs_core::Intent;

    const TABS: &[usize] = &[100, 1000];

    #[divan::bench(args = TABS)]
    fn unchanged(bencher: Bencher, count: usize) {
        let (mut model, tabs) = fixture(count, count / 10);
        bencher
            .with_inputs(|| tabs.clone())
            .bench_local_values(|incoming| {
                assert!(!model.reconcile(incoming, Some(1), true).unwrap());
            });
    }

    #[divan::bench(args = TABS)]
    fn title_change(bencher: Bencher, count: usize) {
        let (mut model, tabs) = fixture(count, count / 10);
        let mut sequence = 0;
        bencher
            .with_inputs(|| {
                sequence += 1;
                let mut incoming = tabs.clone();
                incoming[1].title = format!("Updated {sequence}");
                incoming
            })
            .bench_local_values(|incoming| {
                assert!(model.reconcile(incoming, Some(1), true).unwrap());
            });
    }

    #[divan::bench(args = TABS)]
    fn folder_projection(bencher: Bencher, count: usize) {
        let (mut model, _) = fixture(count, count / 10);
        let mut sequence = 0;
        bencher
            .with_inputs(|| {
                sequence += 1;
                let index = if sequence % 2 == 0 { 0 } else { count / 10 - 1 };
                Intent::MoveFolder {
                    id: "folder-0".into(),
                    index,
                }
            })
            .bench_local_values(|intent| model.dispatch(intent).unwrap());
    }
}

mod ui {
    use super::*;
    use std::time::Duration;
    use vtabs_ui::{ElementId, Rect, SidebarUi, UiInput};

    const TABS: &[usize] = &[50, 1000, 5000];
    const AREA: Rect = Rect::new(0, 0, 40, 60);

    struct Scrolled {
        model: Model,
        ui: SidebarUi,
        tab: (u16, u16),
    }

    fn scrolled(count: usize) -> Scrolled {
        let (model, _) = fixture(count, (count / 10).min(250));
        let mut ui = SidebarUi::new();
        ui.render(&model, AREA, Duration::ZERO).unwrap();
        let tab = ui
            .hit_regions()
            .iter()
            .find(|hit| matches!(hit.id, ElementId::Tab(_)))
            .map(|hit| (hit.rect.x, hit.rect.y))
            .unwrap();
        ui.event(
            &model,
            UiInput::Scroll {
                x: tab.0,
                y: tab.1,
                rows: count as i32,
            },
        );
        ui.render(&model, AREA, Duration::ZERO).unwrap();
        Scrolled { model, ui, tab }
    }

    #[divan::bench(args = TABS)]
    fn idle(bencher: Bencher, count: usize) {
        let Scrolled { model, mut ui, .. } = scrolled(count);
        bencher.bench_local(|| assert!(ui.render(&model, AREA, Duration::ZERO).is_none()));
    }

    #[divan::bench(args = TABS)]
    fn repaint_scrolled(bencher: Bencher, count: usize) {
        let Scrolled { model, mut ui, .. } = scrolled(count);
        bencher.bench_local(|| {
            ui.invalidate();
            ui.render(&model, AREA, Duration::ZERO).unwrap()
        });
    }

    #[divan::bench(args = TABS)]
    fn revision_scrolled(bencher: Bencher, count: usize) {
        let Scrolled {
            mut model, mut ui, ..
        } = scrolled(count);
        bencher.bench_local(|| {
            model.revision = model.revision.wrapping_add(1);
            ui.render(&model, AREA, Duration::ZERO).unwrap()
        });
    }

    #[divan::bench(args = TABS)]
    fn scroll(bencher: Bencher, count: usize) {
        let Scrolled {
            model,
            mut ui,
            tab: (x, y),
        } = scrolled(count);
        let mut rows = 1;
        bencher.bench_local(|| {
            rows = -rows;
            ui.event(&model, UiInput::Scroll { x, y, rows });
            ui.render(&model, AREA, Duration::ZERO).unwrap()
        });
    }

    #[divan::bench(args = TABS)]
    fn surface_motion(bencher: Bencher, count: usize) {
        let Scrolled { model, mut ui, .. } = scrolled(count);
        let mut now = Duration::ZERO;
        ui.transition_surface(0.2, 0.0, now, Duration::from_secs(86400));
        bencher.bench_local(|| {
            now += Duration::from_millis(1);
            ui.render(&model, AREA, now).unwrap()
        });
    }
}

mod store {
    use super::*;
    use std::path::Path;
    use vtabs_store::{Key, Operation, Request, Scope, sqlite};

    fn puts(records: usize, size: usize) -> impl Iterator<Item = Operation> {
        (0..records).map(move |id| Operation::Put {
            key: Key {
                scope: Scope::profile("benchmark"),
                entity: format!("tab-{id}"),
                field: "state".into(),
            },
            value: serde_json::json!({ "title": format!("Terminal {id}"), "payload": "x".repeat(size) }),
            expected_revision: None,
        })
    }

    #[divan::bench(args = [(1024, 256), (32, 60 * 1024)])]
    fn read(bencher: Bencher, (records, size): (usize, usize)) {
        let mut connection = sqlite::open(Path::new(":memory:")).unwrap();
        let operations = puts(records, size).collect::<Vec<_>>();
        for chunk in operations.chunks(128) {
            sqlite::execute(&mut connection, &Request::new(1, chunk.to_vec())).unwrap();
        }
        let read = Request::new(
            2,
            vec![Operation::Read {
                scope: Scope::profile("benchmark"),
            }],
        );
        bencher.bench_local(|| {
            let response = sqlite::execute(&mut connection, &read).unwrap();
            assert_eq!(response.records.len(), records);
            response
        });
    }

    fn write(bencher: Bencher, conditional: bool) {
        let mut connection = sqlite::open(Path::new(":memory:")).unwrap();
        let request = Request::new(1, puts(128, 256).collect());
        let mut revision = 0;
        bencher
            .with_inputs(|| {
                let mut request = request.clone();
                for operation in &mut request.operations {
                    if let Operation::Put {
                        expected_revision, ..
                    } = operation
                    {
                        *expected_revision = conditional.then_some(revision);
                    }
                }
                revision += 1;
                request
            })
            .bench_local_values(|request| {
                let response = sqlite::execute(&mut connection, &request).unwrap();
                assert_eq!(response.records.len(), 128);
                response
            });
    }

    #[divan::bench]
    fn write_unconditional_128(bencher: Bencher) {
        write(bencher, false);
    }

    #[divan::bench]
    fn write_conditional_128(bencher: Bencher) {
        write(bencher, true);
    }
}

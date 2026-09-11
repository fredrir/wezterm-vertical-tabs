use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    time::{Duration, Instant},
};
use vtabs_core::{Folder, Intent, LaunchSpec, Model, Tab};
use vtabs_store::{Key, Operation, Request, Scope, sqlite};
use vtabs_ui::{ElementId, Rect, SidebarUi, UiInput};

struct CountedAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

fn count(bytes: usize) {
    if COUNTING.load(Relaxed) {
        ALLOCATIONS.fetch_add(1, Relaxed);
        BYTES.fetch_add(bytes as u64, Relaxed);
    }
}

// SAFETY: All requests retain System's layouts and pointers unchanged.
unsafe impl GlobalAlloc for CountedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size);
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountedAllocator = CountedAllocator;

struct Bench {
    samples: usize,
    iterations: usize,
    filter: String,
}

fn positive_env(name: &str, fallback: usize) -> usize {
    std::env::var(name).map_or(fallback, |value| {
        value.parse::<usize>().ok().filter(|n| *n > 0).expect(name)
    })
}

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

impl Bench {
    fn run<I>(&self, name: &str, mut prepare: impl FnMut() -> I, mut operation: impl FnMut(I)) {
        if !name.contains(&self.filter) {
            return;
        }
        for _ in 0..10 {
            operation(black_box(prepare()));
        }
        let mut times = Vec::with_capacity(self.samples);
        let mut allocations = Vec::with_capacity(self.samples);
        let mut bytes = Vec::with_capacity(self.samples);
        for _ in 0..self.samples {
            let mut elapsed = Duration::ZERO;
            for _ in 0..self.iterations {
                let input = black_box(prepare());
                let start = Instant::now();
                operation(input);
                elapsed += start.elapsed();
            }
            times.push((elapsed.as_nanos() / self.iterations as u128) as u64);

            let input = black_box(prepare());
            ALLOCATIONS.store(0, Relaxed);
            BYTES.store(0, Relaxed);
            COUNTING.store(true, Relaxed);
            operation(input);
            COUNTING.store(false, Relaxed);
            allocations.push(ALLOCATIONS.load(Relaxed));
            bytes.push(BYTES.load(Relaxed));
        }
        println!(
            "{name},{},{},{}",
            median(&mut times),
            median(&mut allocations),
            median(&mut bytes),
        );
    }
}

fn fixture(count: usize, folder_count: usize) -> (Model, Vec<Tab>) {
    let tabs: Vec<_> = (1..=count)
        .map(|id| Tab {
            id: id as u64,
            title: format!("Terminal {id}"),
            cwd: format!("/home/user/projects/project-{}", id % 20),
            domain: "local".into(),
            host: "workstation".into(),
            user: "user".into(),
            process: "zsh".into(),
            ..Tab::default()
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

fn core(bench: &Bench) {
    for count in [100, 1000] {
        let (mut model, tabs) = fixture(count, count / 10);
        bench.run(
            &format!("core_unchanged_{count}"),
            || tabs.clone(),
            |incoming| {
                assert!(!black_box(
                    model.reconcile(incoming, Some(1), true).unwrap()
                ));
            },
        );
        let sequence = Cell::new(0);
        bench.run(
            &format!("core_title_change_{count}"),
            || {
                let mut incoming = tabs.clone();
                sequence.set(sequence.get() + 1);
                incoming[1].title = format!("Updated {}", sequence.get());
                incoming
            },
            |incoming| {
                assert!(black_box(model.reconcile(incoming, Some(1), true).unwrap()));
            },
        );
        bench.run(
            &format!("core_folder_projection_{count}"),
            || {
                sequence.set(sequence.get() + 1);
                Intent::MoveFolder {
                    id: "folder-0".into(),
                    index: if sequence.get() % 2 == 0 {
                        0
                    } else {
                        count / 10 - 1
                    },
                }
            },
            |intent| {
                black_box(model.dispatch(intent).unwrap());
            },
        );
    }
}

fn ui(bench: &Bench) {
    for (count, folders) in [(50, 5), (1000, 100), (5000, 250)] {
        let (mut model, _) = fixture(count, folders);
        let mut ui = SidebarUi::new();
        let area = Rect::new(0, 0, 40, 60);
        ui.render(&model, area, Duration::ZERO).unwrap();
        let tab = ui
            .hit_regions()
            .iter()
            .find(|hit| matches!(hit.id, ElementId::Tab(_)))
            .unwrap();
        let (x, y) = (tab.rect.x, tab.rect.y);
        ui.event(
            &model,
            UiInput::Scroll {
                x,
                y,
                rows: count as i32,
            },
        );
        ui.render(&model, area, Duration::ZERO).unwrap();
        bench.run(
            &format!("ui_idle_{count}"),
            || (),
            |()| {
                assert!(black_box(ui.render(&model, area, Duration::ZERO)).is_none());
            },
        );
        bench.run(
            &format!("ui_repaint_scrolled_{count}"),
            || (),
            |()| {
                ui.invalidate();
                black_box(ui.render(&model, area, Duration::ZERO).unwrap());
            },
        );
        bench.run(
            &format!("ui_revision_scrolled_{count}"),
            || (),
            |()| {
                model.revision = model.revision.wrapping_add(1);
                black_box(ui.render(&model, area, Duration::ZERO).unwrap());
            },
        );
        let mut direction = 1;
        bench.run(
            &format!("ui_scroll_{count}"),
            || (),
            |()| {
                direction = -direction;
                black_box(ui.event(
                    &model,
                    UiInput::Scroll {
                        x,
                        y,
                        rows: direction,
                    },
                ));
                black_box(ui.render(&model, area, Duration::ZERO).unwrap());
            },
        );
        let mut now = Duration::ZERO;
        ui.transition_surface(0.2, 0.0, now, Duration::from_secs(86400));
        bench.run(
            &format!("ui_surface_motion_{count}"),
            || (),
            |()| {
                now += Duration::from_millis(1);
                black_box(ui.render(&model, area, now).unwrap());
            },
        );
    }
}

fn store(bench: &Bench) {
    for (records, size) in [(1024, 256), (32, 60 * 1024)] {
        let mut connection = sqlite::open(Path::new(":memory:")).unwrap();
        let scope = Scope::profile("benchmark");
        for offset in (0..records).step_by(128) {
            let request = Request::new(1, (offset..(offset + 128).min(records)).map(|id| {
                Operation::Put {
                    key: Key { scope: scope.clone(), entity: format!("tab-{id}"), field: "state".into() },
                    value: serde_json::json!({ "title": format!("Terminal {id}"), "payload": "x".repeat(size) }),
                    expected_revision: None,
                }
            }).collect());
            sqlite::execute(&mut connection, &request).unwrap();
        }
        let read = Request::new(2, vec![Operation::Read { scope }]);
        bench.run(
            &format!("store_read_{records}x{size}"),
            || (),
            |()| {
                let response = sqlite::execute(&mut connection, &read).unwrap();
                assert_eq!(response.records.len(), records);
                black_box(response);
            },
        );
    }
    for conditional in [false, true] {
        let mut connection = sqlite::open(Path::new(":memory:")).unwrap();
        let revision = Cell::new(0);
        let request = Request::new(1, (0..128).map(|id| Operation::Put {
            key: Key { scope: Scope::profile("benchmark"), entity: format!("tab-{id}"), field: "state".into() },
            value: serde_json::json!({ "title": format!("Terminal {id}"), "payload": "x".repeat(256) }),
            expected_revision: None,
        }).collect());
        let name = if conditional {
            "store_write_conditional_128"
        } else {
            "store_write_unconditional_128"
        };
        bench.run(
            name,
            || {
                let mut request = request.clone();
                if conditional {
                    for operation in &mut request.operations {
                        if let Operation::Put {
                            expected_revision, ..
                        } = operation
                        {
                            *expected_revision = Some(revision.get());
                        }
                    }
                }
                request
            },
            |request| {
                let response = sqlite::execute(&mut connection, &request).unwrap();
                revision.set(response.revision);
                assert_eq!(response.records.len(), 128);
                black_box(response);
            },
        );
    }
}

fn main() {
    let bench = Bench {
        samples: positive_env("VTABS_BENCH_SAMPLES", 9),
        iterations: positive_env("VTABS_BENCH_ITERATIONS", 40),
        filter: std::env::args().nth(1).unwrap_or_default(),
    };
    eprintln!(
        "samples={} iterations={}; preparation excluded; Rust allocations only; timings measured without allocation counting",
        bench.samples, bench.iterations
    );
    println!("case,median_ns,median_allocations,median_requested_bytes");
    core(&bench);
    ui(&bench);
    store(&bench);
}

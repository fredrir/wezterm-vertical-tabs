use super::*;

#[test]
fn terminal_is_centered_in_three_quarters_of_the_window() {
    for dpi in [96, 192] {
        let scale = dpi as f32 / 96.;
        let bounds = Bounds {
            x: 5.,
            y: 30.,
            width: 1200. * scale,
            height: 800. * scale,
        };
        let layout = Layout::new(bounds, SHELL_SIZE, 8. * scale, 16. * scale, dpi);
        assert_eq!(layout.frame.width, bounds.width * 0.75);
        assert_eq!(layout.frame.height, bounds.height * 0.75);
        assert_eq!(
            layout.frame.x + layout.frame.width / 2.,
            bounds.x + bounds.width / 2.
        );
        assert_eq!(
            layout.frame.y + layout.frame.height / 2.,
            bounds.y + bounds.height / 2.
        );
        assert!(layout.content.x >= layout.frame.x);
        assert!(layout.content.y >= layout.frame.y);
        assert!(layout.content.x + layout.content.width <= layout.frame.x + layout.frame.width);
        assert!(layout.content.y + layout.content.height <= layout.frame.y + layout.frame.height);
        assert_eq!(layout.content.width, layout.size.cols as f32 * 8. * scale);
        assert_eq!(layout.content.height, layout.size.rows as f32 * 16. * scale);
    }
}

#[test]
fn frame_and_content_sit_on_whole_pixels() {
    let bounds = Bounds {
        x: 3.5,
        y: 28.25,
        width: 1001.5,
        height: 777.3,
    };
    for size in [SHELL_SIZE, (0.77, 0.63), (0.333, 0.9)] {
        let layout = Layout::new(bounds, size, 8.4, 17.3, 144);
        for value in [
            layout.frame.x,
            layout.frame.y,
            layout.frame.width,
            layout.frame.height,
            layout.content.x,
            layout.content.y,
        ] {
            assert_eq!(value.fract(), 0., "{size:?}: {layout:?}");
        }
    }
}

#[test]
fn tiny_windows_still_have_a_valid_pty_size() {
    let layout = Layout::new(
        Bounds {
            width: 1.,
            height: 1.,
            ..Default::default()
        },
        SHELL_SIZE,
        8.,
        16.,
        96,
    );
    assert_eq!((layout.size.cols, layout.size.rows), (1, 1));
}

#[test]
fn programs_choose_their_share_of_the_window() {
    let bounds = Bounds {
        x: 5.,
        y: 30.,
        width: 1200.,
        height: 800.,
    };
    for size in [(0.5, 0.9), (1., 1.), (0.2, 0.35)] {
        let layout = Layout::new(bounds, size, 8., 16., 96);
        assert!((layout.frame.width - bounds.width * size.0).abs() < 0.01);
        assert!((layout.frame.height - bounds.height * size.1).abs() < 0.01);
        let center = |start: f32, extent: f32| start + extent / 2.;
        assert!(
            (center(layout.frame.x, layout.frame.width) - center(bounds.x, bounds.width)).abs()
                < 0.01
        );
        assert!(
            (center(layout.frame.y, layout.frame.height) - center(bounds.y, bounds.height)).abs()
                < 0.01
        );
        assert!(layout.content.x + layout.content.width <= layout.frame.x + layout.frame.width);
        assert!(layout.content.y + layout.content.height <= layout.frame.y + layout.frame.height);
    }
}

#[test]
fn programs_without_a_size_follow_the_shell() {
    let program = |width, height| {
        let mut state = State::default();
        state.program = Some(Program::new(
            1,
            TerminalProgram {
                spawn: SpawnCommand::default(),
                width,
                height,
            },
        ));
        state
    };
    let shell = (0.6, 0.9);
    assert_eq!(State::default().size(shell), shell);
    assert_eq!(program(None, None).size(shell), shell);
    assert_eq!(program(Some(0.5), None).size(shell), (0.5, 0.9));
    assert_eq!(program(Some(0.5), Some(1.)).size(shell), (0.5, 1.));
}

fn config(size: (f32, f32)) -> QuickTerminalConfig {
    QuickTerminalConfig {
        size,
        opacity: 1.,
        blur: 0.,
        border_color: None,
        background_color: None,
        auto_hide: false,
        preserve_shell: true,
        transition: Duration::from_millis(140),
    }
}

#[test]
fn steps_stay_between_the_minimum_and_the_window() {
    assert_eq!(step(600., 16., 200., 1200.), 616.);
    assert_eq!(step(600., -16., 200., 1200.), 584.);
    assert_eq!(step(1190., 16., 200., 1200.), 1200.);
    assert_eq!(step(210., -16., 200., 1200.), 200.);
    assert_eq!(step(150., -16., 200., 1200.), 150.);
    assert_eq!(step(150., 16., 200., 1200.), 166.);
}

#[test]
fn a_step_of_one_cell_changes_the_grid_by_one_cell() {
    let bounds = Bounds {
        x: 5.,
        y: 30.,
        width: 2400.,
        height: 1600.,
    };
    let (cell_width, cell_height) = (16., 32.);
    let before = Layout::new(bounds, SHELL_SIZE, cell_width, cell_height, 192);
    for amount in [1., 3.] {
        let width = (before.frame.width + amount * cell_width) / bounds.width;
        let height = (before.frame.height - amount * cell_height) / bounds.height;
        let after = Layout::new(bounds, (width, height), cell_width, cell_height, 192);
        assert_eq!(after.size.cols, before.size.cols + amount as usize);
        assert_eq!(after.size.rows, before.size.rows - amount as usize);
    }
}

#[test]
fn content_stays_inside_the_padding() {
    for (size, dpi) in [(SHELL_SIZE, 96), ((0.77, 0.63), 110), ((0.333, 0.9), 144)] {
        let layout = Layout::new(
            Bounds {
                x: 3.,
                y: 28.,
                width: 1001.,
                height: 777.,
            },
            size,
            8.,
            17.,
            dpi,
        );
        let (inner, content) = (layout.inner, layout.content);
        assert!(content.x >= inner.x && content.y >= inner.y, "{:?}", layout);
        assert!(
            content.x + content.width <= inner.x + inner.width,
            "{:?}",
            layout
        );
        assert!(
            content.y + content.height <= inner.y + inner.height,
            "{:?}",
            layout
        );
    }
}

#[test]
fn steps_change_the_window_size_unless_the_program_sets_that_axis() {
    let configured = (0.75, 0.75);
    let mut state = State::default();
    state.adjust(true, 0.8);
    assert_eq!(state.size(configured), (0.8, 0.75));
    state.program = Some(Program::new(
        1,
        TerminalProgram {
            spawn: SpawnCommand::default(),
            width: None,
            height: Some(0.9),
        },
    ));
    assert_eq!(state.size(configured), (0.8, 0.9));
    state.adjust(false, 0.5);
    state.adjust(true, 0.6);
    assert_eq!(state.size(configured), (0.6, 0.5));
    state.program = None;
    assert_eq!(state.size(configured), (0.6, 0.75));
}

#[test]
fn a_changed_size_setting_clears_its_axis_only() {
    let mut state = State::default();
    assert!(state.configure(config((0.75, 0.75))));
    state.adjust(true, 0.8);
    state.adjust(false, 0.6);
    assert!(!state.configure(config((0.75, 0.75))));
    let mut faster = config((0.75, 0.75));
    faster.transition = Duration::ZERO;
    assert!(state.configure(faster));
    assert_eq!(state.size((0.75, 0.75)), (0.8, 0.6));
    assert!(state.configure(config((0.5, 0.75))));
    assert_eq!(state.size((0.5, 0.75)), (0.5, 0.6));
}

#[test]
fn motion_eases_out_and_settles_at_its_duration() {
    let start = Instant::now();
    let mut motion = Motion {
        size: (0.5, 0.5),
        origin: (100., 50.),
        start,
        duration: Duration::from_millis(100),
        eased: 0.,
    };
    assert!(motion.advance(start));
    assert_eq!(motion.lerp((0.5, 0.5), (0.7, 0.9)), (0.5, 0.5));
    assert!(motion.advance(start + Duration::from_millis(50)));
    assert!((motion.eased - 0.875).abs() < 1e-6);
    assert!(!motion.advance(start + Duration::from_millis(100)));
    assert_eq!(motion.lerp((0.5, 0.5), (0.7, 0.9)), (0.7, 0.9));
    assert!(!motion.advance(start + Duration::from_secs(1)));
    assert_eq!(motion.eased, 1.);
}

#[test]
fn client_domains_spawn_untabbed_panes_only_with_local_layouts() {
    let mut config = config::Config::default_config();
    let unix = |name: &str, local_pane_layout: bool| config::UnixDomain {
        name: name.into(),
        local_pane_layout,
        ..Default::default()
    };
    config.unix_domains = vec![unix("localmux", false), unix("laid-out", true)];
    config.tls_clients = vec![config::TlsDomainClient {
        name: "tls".into(),
        local_pane_layout: true,
        ..Default::default()
    }];
    config.ssh_domains = Some(vec![config::SshDomain {
        name: "ssh".into(),
        ..Default::default()
    }]);
    for (name, laid_out) in [
        ("localmux", false),
        ("laid-out", true),
        ("tls", true),
        ("ssh", false),
        ("unknown", false),
    ] {
        assert_eq!(local_pane_layout(&config, name), laid_out, "{name}");
    }
}

#[test]
fn directories_decode_whichever_host_reported_them() {
    for (url, path) in [
        ("file://macie/Users/me/projects", "/Users/me/projects"),
        ("file://archie/home/me/a%20b", "/home/me/a b"),
        ("file:///Users/me/", "/Users/me/"),
    ] {
        assert_eq!(
            directory(&url::Url::parse(url).unwrap()).as_deref(),
            Some(path)
        );
    }
}

#[test]
fn discarded_shell_launch_cannot_replace_a_reopened_shell() {
    let mut state = State::default();
    state.visible = true;
    let previous = state.launch_shell();
    assert!(state.wants(previous));
    state.discard_shell();
    assert!(!state.wants(previous));
    let current = state.launch_shell();
    assert!(!state.wants(previous));
    assert!(state.wants(current));
    state.settle(current, None);
    assert!(!state.visible);
    assert!(state.pending.is_none());
}

#[test]
fn preserved_shell_launch_can_finish_while_hidden() {
    let mut state = State::default();
    let pending = state.launch_shell();
    state.visible = false;
    assert!(state.wants(pending));
    state.visible = true;
    assert!(state.wants(pending));
}

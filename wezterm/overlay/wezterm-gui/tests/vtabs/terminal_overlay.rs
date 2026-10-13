use super::*;
use crate::termwindow::ui_host::{HideOnEscape, TerminalPadding};
use config::Dimension;

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
        let layout = Layout::new(
            bounds,
            SHELL_SIZE,
            &edges(WindowPadding::default()),
            (8. * scale, 16. * scale),
            dpi,
        );
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
        let layout = Layout::new(
            bounds,
            size,
            &edges(WindowPadding::default()),
            (8.4, 17.3),
            144,
        );
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
        &edges(WindowPadding::default()),
        (8., 16.),
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
        let layout = Layout::new(
            bounds,
            size,
            &edges(WindowPadding::default()),
            (8., 16.),
            96,
        );
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
                width,
                height,
                ..Default::default()
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

fn edges(padding: WindowPadding) -> Edges {
    Edges {
        border: 0.,
        padding,
        alignment: WindowContentAlignment::default(),
    }
}

fn config(size: (f32, f32)) -> QuickTerminalConfig {
    QuickTerminalConfig {
        size,
        padding: TerminalPadding::default(),
        opacity: 1.,
        blur: 0.,
        border: TerminalBorder::Palette,
        background_color: None,
        auto_hide: false,
        hide_on_escape: HideOnEscape::Auto,
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
    let before = Layout::new(
        bounds,
        SHELL_SIZE,
        &edges(WindowPadding::default()),
        (cell_width, cell_height),
        192,
    );
    for amount in [1., 3.] {
        let width = (before.frame.width + amount * cell_width) / bounds.width;
        let height = (before.frame.height - amount * cell_height) / bounds.height;
        let after = Layout::new(
            bounds,
            (width, height),
            &edges(WindowPadding::default()),
            (cell_width, cell_height),
            192,
        );
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
            &edges(WindowPadding::default()),
            (8., 17.),
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
            height: Some(0.9),
            ..Default::default()
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

#[test]
fn padding_reads_wezterm_units_against_the_frame() {
    let bounds = Bounds {
        x: 0.,
        y: 0.,
        width: 1600.,
        height: 1000.,
    };
    let padding = WindowPadding {
        left: Dimension::Pixels(0.),
        top: Dimension::Percent(0.1),
        right: Dimension::Cells(2.),
        bottom: Dimension::Points(18.),
    };
    let layout = Layout::new(bounds, (1., 1.), &edges(padding), (10., 20.), 144);
    let (frame, inner) = (layout.frame, layout.inner);
    assert_eq!(inner.x - frame.x, 0.);
    assert_eq!(inner.y - frame.y, 100.);
    assert_eq!(frame.x + frame.width - (inner.x + inner.width), 20.);
    assert_eq!(frame.y + frame.height - (inner.y + inner.height), 36.);
    assert_eq!((layout.size.cols, layout.size.rows), (158, 43));
    assert!(layout.content.x >= inner.x && layout.content.y >= inner.y);
}

#[test]
fn padding_takes_at_most_a_quarter_of_each_side() {
    let bounds = Bounds {
        x: 0.,
        y: 0.,
        width: 800.,
        height: 600.,
    };
    let padding = WindowPadding {
        left: Dimension::Percent(2.),
        top: Dimension::Cells(100.),
        right: Dimension::Pixels(1e6),
        bottom: Dimension::Points(1e6),
    };
    let layout = Layout::new(bounds, (1., 1.), &edges(padding), (8., 16.), 96);
    assert_eq!((layout.inner.width, layout.inner.height), (400., 300.));
    assert_eq!((layout.size.cols, layout.size.rows), (50, 18));
}

#[test]
fn unset_sides_follow_the_window_padding() {
    let window = WindowPadding::default();
    let program = TerminalPadding {
        top: Some(Dimension::Pixels(3.)),
        ..Default::default()
    };
    let settings = TerminalPadding {
        top: Some(Dimension::Pixels(9.)),
        left: Some(Dimension::Cells(2.)),
        ..Default::default()
    };
    let resolved = program.or(settings).resolve(&window);
    assert_eq!(resolved.top, Dimension::Pixels(3.));
    assert_eq!(resolved.left, Dimension::Cells(2.));
    assert_eq!(resolved.right, window.right);
    assert_eq!(resolved.bottom, window.bottom);
}

#[test]
fn program_values_override_the_quick_terminal_settings() {
    let settings = QuickTerminalConfig {
        padding: TerminalPadding {
            left: Some(Dimension::Pixels(4.)),
            top: Some(Dimension::Pixels(4.)),
            ..Default::default()
        },
        opacity: 0.8,
        blur: 10.,
        auto_hide: true,
        background_color: Some(LinearRgba(0., 0., 0., 1.)),
        ..config((0.75, 0.75))
    };
    assert_eq!(settings.with(&TerminalProgram::default()), settings);
    let program = TerminalProgram {
        width: Some(0.9),
        padding: TerminalPadding {
            left: Some(Dimension::Cells(0.)),
            bottom: Some(Dimension::Cells(1.)),
            ..Default::default()
        },
        opacity: Some(1.),
        border: Some(TerminalBorder::None),
        colors: Some(config::Palette {
            background: Some((255, 0, 0).into()),
            ..Default::default()
        }),
        auto_hide: Some(false),
        hide_on_escape: Some(HideOnEscape::Never),
        ..Default::default()
    };
    let merged = settings.with(&program);
    assert_eq!(
        merged.padding,
        TerminalPadding {
            left: Some(Dimension::Cells(0.)),
            top: Some(Dimension::Pixels(4.)),
            right: None,
            bottom: Some(Dimension::Cells(1.)),
        }
    );
    assert_eq!((merged.opacity, merged.blur), (1., 10.));
    assert_eq!(merged.border, TerminalBorder::None);
    assert_eq!(merged.background_color, Some(LinearRgba(1., 0., 0., 1.)));
    assert!(!merged.auto_hide);
    assert_eq!(merged.hide_on_escape, HideOnEscape::Never);
    assert_eq!(merged.size, settings.size);
}

fn program(id: u64, command: &str, preserve: bool) -> Program {
    Program::new(
        id,
        TerminalProgram {
            spawn: SpawnCommand {
                args: Some(vec![command.into()]),
                ..Default::default()
            },
            preserve,
            ..Default::default()
        },
    )
}

#[test]
fn preserved_programs_wait_hidden_for_their_next_opening() {
    let mut state = State::default();
    state.visible = true;
    state.program = Some(program(1, "lazygit", true));
    assert!(state.put_away(|_| true).is_none());
    assert!(state.program.is_none());
    // A launch still in flight lands in the kept program.
    assert!(state.wants(Slot::Program(1)));
    let lazygit = state.kept[0].spec.clone();
    assert!(state.reusable(&lazygit, |_| true));
    assert!(!state.reusable(&program(2, "yazi", true).spec, |_| true));
    let larger = TerminalProgram {
        width: Some(0.9),
        ..lazygit
    };
    let reopened = state.take_kept(&larger).unwrap();
    assert_eq!(reopened.id, 1);
    assert_eq!(reopened.size, (Some(0.9), None));
    assert_eq!(reopened.spec, larger);
    assert!(state.kept.is_empty());
}

#[test]
fn programs_without_preserve_end_when_put_away() {
    let mut state = State::default();
    state.program = Some(program(1, "lazygit", false));
    state.put_away(|_| true);
    assert!(state.program.is_none() && state.kept.is_empty());
    assert!(!state.wants(Slot::Program(1)));
    assert!(!state.reusable(&program(2, "lazygit", false).spec, |_| true));
}

#[test]
fn a_failed_kept_launch_leaves_the_shown_program_alone() {
    let mut state = State::default();
    state.visible = true;
    state.program = Some(program(1, "lazygit", true));
    state.put_away(|_| true);
    state.program = Some(program(2, "yazi", false));
    state.settle(Slot::Program(1), None);
    assert!(state.kept.is_empty());
    assert!(state.visible);
    assert_eq!(state.program.as_ref().map(|program| program.id), Some(2));
}

#[test]
fn padding_is_measured_inside_the_border() {
    let bounds = Bounds {
        x: 0.,
        y: 0.,
        width: 1000.,
        height: 600.,
    };
    let bordered = |padding| Edges {
        border: 2.,
        ..edges(padding)
    };
    let flush = WindowPadding {
        left: Dimension::Pixels(0.),
        top: Dimension::Pixels(0.),
        right: Dimension::Pixels(0.),
        bottom: Dimension::Pixels(0.),
    };
    let layout = Layout::new(bounds, (1., 1.), &bordered(flush), (10., 20.), 96);
    assert_eq!((layout.inner.x, layout.inner.y), (2., 2.));
    assert_eq!((layout.inner.width, layout.inner.height), (996., 596.));
    let padded = WindowPadding {
        left: Dimension::Percent(0.1),
        top: Dimension::Cells(1.),
        ..flush
    };
    let layout = Layout::new(bounds, (1., 1.), &bordered(padded), (10., 20.), 96);
    assert_eq!((layout.inner.x, layout.inner.y), (2. + 99., 2. + 20.));
}

#[test]
fn content_follows_window_content_alignment() {
    let bounds = Bounds {
        x: 0.,
        y: 0.,
        width: 1005.,
        height: 610.,
    };
    let zero = WindowPadding {
        left: Dimension::Pixels(0.),
        top: Dimension::Pixels(0.),
        right: Dimension::Pixels(0.),
        bottom: Dimension::Pixels(0.),
    };
    let place = |horizontal, vertical| {
        let edges = Edges {
            alignment: WindowContentAlignment {
                horizontal,
                vertical,
            },
            ..edges(zero)
        };
        let content = Layout::new(bounds, (1., 1.), &edges, (10., 20.), 96).content;
        (content.x, content.y)
    };
    use HorizontalWindowContentAlignment as H;
    use VerticalWindowContentAlignment as V;
    assert_eq!(place(H::Left, V::Top), (0., 0.));
    assert_eq!(place(H::Center, V::Center), (2., 5.));
    assert_eq!(place(H::Right, V::Bottom), (5., 10.));
}

#[test]
fn steps_change_the_grid_by_whole_cells_through_percent_padding() {
    // Ten percent each side, as `Layout` floors it.
    let grid = |frame: f32| ((frame - 2. * (0.1 * frame).floor()) / 10.).floor();
    let bounds = (100., 2000.);
    for (current, cells) in [(500., 1.), (547., -1.), (500., 3.), (512., -1.)] {
        let target = step_cells(current, cells, 10., bounds, grid);
        assert_eq!(grid(target), grid(current) + cells, "{current} by {cells}");
        assert!((target - current).abs() < (cells.abs() + 1.) * 10. / 0.8);
    }
    assert_eq!(step_cells(105., -1., 10., bounds, grid), 100.);
    assert_eq!(step_cells(1995., 2., 10., bounds, grid), 2000.);
}

#[test]
fn only_preserved_bindings_reopen_a_kept_program() {
    let mut state = State::default();
    state.program = Some(program(1, "lazygit", true));
    state.put_away(|_| true);
    let fresh = program(2, "lazygit", false).spec;
    assert!(!state.reusable(&fresh, |_| true));
    assert!(state.take_kept(&fresh).is_none());
    assert_eq!(state.kept.len(), 1);
    // A shown program without `preserve` ends when replaced, so it cannot be reopened.
    let mut state = State::default();
    state.program = Some(program(1, "lazygit", false));
    assert!(!state.reusable(&program(2, "lazygit", true).spec, |_| true));
}

#[test]
fn a_launch_takes_the_colors_its_program_has_when_it_lands() {
    let mut state = State::default();
    let mut lazygit = program(1, "lazygit", true);
    lazygit.spec.colors = Some(config::Palette {
        background: Some((1, 2, 3).into()),
        ..Default::default()
    });
    state.program = Some(lazygit);
    state.put_away(|_| true);
    let light = TerminalProgram {
        colors: Some(config::Palette {
            background: Some((255, 255, 255).into()),
            ..Default::default()
        }),
        ..state.kept[0].spec.clone()
    };
    state.program = state.take_kept(&light);
    assert_eq!(
        state
            .colors(Slot::Program(1))
            .and_then(|colors| colors.background),
        Some((255, 255, 255).into())
    );
    assert!(state.colors(Slot::Shell(1)).is_none());
}

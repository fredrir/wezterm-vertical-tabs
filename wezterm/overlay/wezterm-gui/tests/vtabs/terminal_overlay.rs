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
            (center(layout.frame.y, layout.frame.height) - center(bounds.y, bounds.height))
                .abs()
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
        state.program = Some(Program {
            id: 1,
            spec: TerminalProgram {
                spawn: SpawnCommand::default(),
                width,
                height,
            },
            pane: None,
        });
        state
    };
    let shell = (0.6, 0.9);
    assert_eq!(State::default().size(shell), shell);
    assert_eq!(program(None, None).size(shell), shell);
    assert_eq!(program(Some(0.5), None).size(shell), (0.5, 0.9));
    assert_eq!(program(Some(0.5), Some(1.)).size(shell), (0.5, 1.));
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

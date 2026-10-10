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
        let layout = Layout::new(bounds, 8. * scale, 16. * scale, dpi);
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
        8.,
        16.,
        96,
    );
    assert_eq!((layout.size.cols, layout.size.rows), (1, 1));
}

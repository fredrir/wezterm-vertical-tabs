use super::*;

fn argv(items: &[&str]) -> std::vec::IntoIter<String> {
    items
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .into_iter()
}

fn minimal(extra: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![
        "--w",
        "100",
        "--h",
        "80",
        "--card",
        "10",
        "8",
        "60",
        "40",
        "--fill",
        "#1e1e2e",
        "--card-fill",
        "#111111",
        "--out",
        "/tmp/x.png",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    out.extend(extra.iter().map(|s| s.to_string()));
    out
}

#[test]
fn every_required_argument_is_required() {
    for drop in ["--w", "--h", "--card", "--fill", "--card-fill", "--out"] {
        let full = minimal(&[]);
        let mut kept: Vec<String> = vec![];
        let mut skip = 0;
        for item in full {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if item == drop {
                skip = if drop == "--card" { 4 } else { 1 };
                continue;
            }
            kept.push(item);
        }
        assert!(
            Args::parse(kept.into_iter()).is_err(),
            "{drop} was optional"
        );
    }
}

/// A size out of range is refused rather than clamped, and the area bound catches what the
/// per-side bounds let through.
#[test]
fn sizes_are_refused_not_clamped_including_a_long_thin_one() {
    assert!(Args::parse(argv(&["--w", "0", "--h", "10"])).is_err());
    assert!(Args::parse(argv(&["--w", "-4", "--h", "10"])).is_err());
    assert!(
        Args::parse(argv(&["--w", "60000", "--h", "1"])).is_err(),
        "per-side"
    );
    let huge = minimal(&[]);
    let mut wide: Vec<String> = huge.clone();
    wide[1] = "16000".into();
    wide[3] = "16000".into();
    assert!(Args::parse(wide.into_iter()).is_err(), "area bound");
    let ok = Args::parse(minimal(&[]).into_iter()).unwrap();
    assert_eq!((ok.width, ok.height), (100, 80));
}

#[test]
fn a_junk_number_or_an_unknown_flag_is_an_error_not_a_default() {
    let mut bad = minimal(&[]);
    bad[1] = "wide".into();
    assert!(Args::parse(bad.into_iter()).is_err());
    assert!(Args::parse(minimal(&["--nope"]).into_iter()).is_err());
    let mut inf = minimal(&[]);
    inf[1] = "inf".into();
    assert!(
        Args::parse(inf.into_iter()).is_err(),
        "a non-finite size is not a size"
    );
}

#[test]
fn a_bad_colour_is_an_error_rather_than_a_silent_black() {
    let mut bad = minimal(&[]);
    let at = bad.iter().position(|a| a == "#1e1e2e").unwrap();
    bad[at] = "not-a-colour".into();
    assert!(Args::parse(bad.into_iter()).is_err());
}

#[test]
fn the_border_width_is_a_device_pixel_unless_told_otherwise_and_never_negative() {
    let default = Args::parse(minimal(&[]).into_iter()).unwrap();
    assert_eq!(default.border_width, 1.0);
    let hairline = Args::parse(minimal(&["--border-width", "0.5"]).into_iter()).unwrap();
    assert_eq!(hairline.border_width, 0.5);
    let negative = Args::parse(minimal(&["--border-width", "-2"]).into_iter()).unwrap();
    assert_eq!(negative.border_width, 0.0);
    assert!(Args::parse(minimal(&["--border-width", "thin"]).into_iter()).is_err());
}

#[test]
fn the_rendered_frame_is_a_png_of_the_size_that_was_asked_for() {
    let args = Args::parse(minimal(&["--radius", "8", "--border", "#445566"]).into_iter()).unwrap();
    let png = args.render();
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 100);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 80);
}

/// A card of zero size leaves the page whole rather than drawing a degenerate rectangle.
#[test]
fn a_card_with_no_area_leaves_the_page_untouched() {
    let mut none = minimal(&[]);
    let at = none.iter().position(|a| a == "--card").unwrap();
    none[at + 3] = "0".into();
    let args = Args::parse(none.into_iter()).unwrap();
    assert_eq!(args.render(), Canvas::new(100, 80, args.fill).to_png());
}

#[test]
fn the_file_is_written_atomically_at_0600_and_leaves_no_temp_behind() {
    let dir = std::env::temp_dir().join(format!("vtabs-frame-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = dir.join("frame.png");
    write_private(&out, b"hello").unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"hello");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&out).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "owner only");
    }
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n != "frame.png")
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
    // Rewriting the same path replaces it, which is what regeneration does on every resize.
    write_private(&out, b"second").unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"second");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The temp file is created with `create_new`, so a name planted in the destination directory
/// cannot be followed; the write fails rather than landing wherever the link pointed.
#[cfg(unix)]
#[test]
fn a_planted_symlink_at_the_temp_name_is_refused_not_followed() {
    let dir = std::env::temp_dir().join(format!("vtabs-frame-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("frame.png");
    let target = dir.join("elsewhere");
    std::fs::write(&target, b"original").unwrap();
    let temp = dir.join(format!(".frame.png.{}.tmp", std::process::id()));
    std::os::unix::fs::symlink(&target, &temp).unwrap();
    // The planted link is removed and recreated as a fresh file, so nothing follows it.
    write_private(&out, b"png").unwrap();
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"original",
        "the link target is untouched"
    );
    assert_eq!(std::fs::read(&out).unwrap(), b"png");
    let _ = std::fs::remove_dir_all(&dir);
}

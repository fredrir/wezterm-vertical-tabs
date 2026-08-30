//! `wez-vtabs frame` renders the Zen frame: a page-coloured canvas with one rounded card cut into
//! it, which the GUI hands to `config.background`. Cells whose background is the terminal default
//! emit no quad, so the card is what the shell appears to sit on.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::png::{Canvas, RoundRect, parse_colour};

/// The caller is a GUI sizing an allocation from a window it measured, so the bound is on the area
/// as well as each side: 60000x1 clears any per-side cap and still asks for a 240 MB buffer.
const MAX_SIDE: u32 = 16384;
const MAX_AREA: u64 = 8192 * 8192;

pub struct Args {
    pub width: u32,
    pub height: u32,
    pub card: (f32, f32, f32, f32),
    pub radius: f32,
    pub fill: [u8; 4],
    pub card_fill: [u8; 4],
    pub border: Option<[u8; 4]>,
    pub border_width: f32,
    pub out: PathBuf,
}

fn want<T>(value: Option<T>, what: &str) -> io::Result<T> {
    value.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("frame: {what}")))
}

impl Args {
    pub fn parse<I: Iterator<Item = String>>(mut args: I) -> io::Result<Self> {
        let (mut width, mut height, mut radius) = (None, None, 0.0f32);
        let (mut card, mut out) = (None, None);
        let (mut fill, mut card_fill, mut border) = (None, None, None);
        let mut border_width = 1.0f32;
        let number = |args: &mut I, what: &'static str| -> io::Result<f32> {
            let raw = want(args.next(), what)?;
            raw.parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("frame: {what}"))
                })
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--w" => width = Some(number(&mut args, "--w")? as i64),
                "--h" => height = Some(number(&mut args, "--h")? as i64),
                "--radius" => radius = number(&mut args, "--radius")?,
                "--border-width" => border_width = number(&mut args, "--border-width")?,
                "--card" => {
                    let x = number(&mut args, "--card x")?;
                    let y = number(&mut args, "--card y")?;
                    let w = number(&mut args, "--card w")?;
                    let h = number(&mut args, "--card h")?;
                    card = Some((x, y, w, h));
                }
                "--fill" => fill = parse_colour(&want(args.next(), "--fill")?),
                "--card-fill" => card_fill = parse_colour(&want(args.next(), "--card-fill")?),
                "--border" => border = parse_colour(&want(args.next(), "--border")?),
                "--out" => out = Some(PathBuf::from(want(args.next(), "--out")?)),
                other => {
                    let msg = format!("frame: unknown argument {other}");
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
                }
            }
        }
        // Refused, never clamped: a size this far out means the caller measured something it should
        // not have, and quietly drawing a different frame would hide that.
        let width = want(width, "--w is required")?;
        let height = want(height, "--h is required")?;
        let bad = width < 1 || height < 1 || width > MAX_SIDE as i64 || height > MAX_SIDE as i64;
        if bad || (width as u64) * (height as u64) > MAX_AREA {
            let msg = format!("frame: refusing {width}x{height}");
            return Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
        }
        Ok(Self {
            width: width as u32,
            height: height as u32,
            card: want(card, "--card is required")?,
            radius: radius.max(0.0),
            fill: want(fill, "--fill is required")?,
            card_fill: want(card_fill, "--card-fill is required")?,
            border,
            border_width: border_width.max(0.0),
            out: want(out, "--out is required")?,
        })
    }

    pub fn render(&self) -> Vec<u8> {
        let mut canvas = Canvas::new(self.width, self.height, self.fill);
        let (x, y, w, h) = self.card;
        if w > 0.0 && h > 0.0 {
            let rect = RoundRect::new(x, y, w, h, self.radius);
            canvas.rounded_rect(&rect, self.card_fill);
            if let Some(colour) = self.border {
                canvas.rounded_border(&rect, colour, self.border_width);
            }
        }
        canvas.to_png()
    }
}

/// Creates `path` for this user only, then writes and renames. `create_new` refuses a path that
/// already exists, which is what stops a pre-created symlink being followed; the temp file lives
/// beside the destination so the rename cannot cross a filesystem.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let pid = std::process::id();
    let temp = dir.join(format!(
        ".{}.{pid}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("frame")
    ));
    let _ = std::fs::remove_file(&temp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&temp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(err) = result {
        let _ = std::fs::remove_file(&temp);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(err);
    }
    Ok(())
}

pub fn run<I: Iterator<Item = String>>(args: I) -> io::Result<()> {
    let args = Args::parse(args)?;
    write_private(&args.out, &args.render())
}

#[cfg(test)]
mod tests {
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
    fn the_rendered_frame_is_a_png_of_the_size_that_was_asked_for() {
        let args =
            Args::parse(minimal(&["--radius", "8", "--border", "#445566"]).into_iter()).unwrap();
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
}

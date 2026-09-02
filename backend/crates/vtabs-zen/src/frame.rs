//! `wez-vtabs frame` renders the Zen frame: a page-coloured canvas with one rounded card cut into
//! it, which the GUI hands to `config.background`. Cells whose background is the terminal default
//! emit no quad, so the card is what the shell appears to sit on.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::png::{Canvas, RoundRect, parse_colour};

/// The caller is a GUI sizing an allocation from a window it measured, so the bound is on the area
/// as well as each side: 60000x1 clears any per-side cap and still asks for a 240 MB buffer.
pub const MAX_SIDE: u32 = 16384;
pub const MAX_AREA: u64 = 8192 * 8192;

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
            let border = self.border.map(|colour| (colour, self.border_width));
            canvas.rounded_card(&rect, self.card_fill, border);
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
#[path = "../tests/unit/frame.rs"]
mod tests;

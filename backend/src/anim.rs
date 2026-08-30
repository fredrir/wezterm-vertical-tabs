//! Backend-side interpolation: Lua sends one final frame, the backend generates every step.

use std::time::{Duration, Instant};

use crate::command::{AnimCmd, AnimRow};

pub const MAX_DATA: usize = 8 * 1024;
pub const MAX_ROWS: usize = 128;
pub const MAX_MS: u64 = 2000;
pub const MIN_FPS: u32 = 15;
pub const MAX_FPS: u32 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub fn parse_hex(spec: &str) -> Option<Rgb> {
    let hex = spec.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// `linear` | `outCubic` | `inOutQuad`; anything else is linear.
pub fn ease(name: &str, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match name {
        "outCubic" => 1.0 - (1.0 - t).powi(3),
        "inOutQuad" => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }
        _ => t,
    }
}

/// Byte range of one row's payload and the row it addresses; `None` is the frame preamble.
fn segments(data: &str) -> Vec<(Option<u16>, usize, usize)> {
    let bytes = data.as_bytes();
    let mut cuts: Vec<(u16, usize)> = Vec::new();
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i] == 0x1b && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            let mut row: u32 = 0;
            let mut digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                row = row * 10 + u32::from(bytes[j] - b'0');
                j += 1;
                digits += 1;
            }
            if digits > 0 && bytes[j..].starts_with(b";1H") {
                cuts.push((row.min(u32::from(u16::MAX)) as u16, i));
                i = j + 3;
                continue;
            }
        }
        i += 1;
    }
    let mut out = Vec::new();
    if cuts.is_empty() {
        out.push((None, 0, data.len()));
        return out;
    }
    if cuts[0].1 > 0 {
        out.push((None, 0, cuts[0].1));
    }
    for (n, cut) in cuts.iter().enumerate() {
        let end = cuts.get(n + 1).map_or(data.len(), |next| next.1);
        out.push((Some(cut.0), cut.1, end));
    }
    out
}

/// Rewrites every `38;2`/`48;2` colour toward `anchor`; text, CUP, bold and reset pass through.
fn shade(seg: &str, anchor: Rgb, t: f32, out: &mut String) {
    let bytes = seg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'm' {
                let body = &seg[i + 2..j];
                let mut params = body.split(';');
                let kind = params.next().unwrap_or("");
                let mode = params.next().unwrap_or("");
                let rgb: Vec<Option<u8>> = params.map(|p| p.parse::<u8>().ok()).collect();
                if (kind == "38" || kind == "48")
                    && mode == "2"
                    && rgb.len() == 3
                    && rgb.iter().all(Option::is_some)
                {
                    let (r, g, b) = (rgb[0].unwrap(), rgb[1].unwrap(), rgb[2].unwrap());
                    out.push_str("\x1b[");
                    out.push_str(kind);
                    out.push_str(";2;");
                    out.push_str(&lerp(anchor.0, r, t).to_string());
                    out.push(';');
                    out.push_str(&lerp(anchor.1, g, t).to_string());
                    out.push(';');
                    out.push_str(&lerp(anchor.2, b, t).to_string());
                    out.push('m');
                    i = j + 1;
                    continue;
                }
                out.push_str(&seg[i..=j]);
                i = j + 1;
                continue;
            }
        }
        let ch_end = (1..=4)
            .map(|n| i + n)
            .find(|&n| seg.is_char_boundary(n))
            .unwrap_or(seg.len());
        out.push_str(&seg[i..ch_end]);
        i = ch_end;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Rejected {
    Size,
    Bounds,
}

impl Rejected {
    pub fn reason(&self) -> &'static str {
        match self {
            Rejected::Size => "size",
            Rejected::Bounds => "bounds",
        }
    }
}

pub struct Run {
    pub id: u64,
    ms: u64,
    frame: Duration,
    ease_name: String,
    inward: bool,
    anchor: Rgb,
    delays: Vec<(u16, u64)>,
    data: String,
    started: Instant,
    pub next_at: Instant,
    finished: bool,
}

impl Run {
    pub fn new(cmd: AnimCmd, now: Instant) -> Result<Self, Rejected> {
        if cmd.data.len() > MAX_DATA {
            return Err(Rejected::Size);
        }
        let fps = cmd.fps.unwrap_or(30);
        if cmd.rows.len() > MAX_ROWS
            || cmd.rows.is_empty()
            || cmd.ms == 0
            || cmd.ms > MAX_MS
            || !(MIN_FPS..=MAX_FPS).contains(&fps)
            || cmd.rows.iter().any(|r| r.delay > MAX_MS)
        {
            return Err(Rejected::Bounds);
        }
        let Some(anchor) = parse_hex(&cmd.anchor) else {
            return Err(Rejected::Bounds);
        };
        Ok(Self {
            id: cmd.id,
            ms: cmd.ms,
            frame: Duration::from_millis((1000 / u64::from(fps)).max(1)),
            ease_name: cmd.ease.unwrap_or_else(|| "linear".to_string()),
            inward: cmd.dir.as_deref() != Some("out"),
            anchor,
            delays: cmd.rows.iter().map(|r: &AnimRow| (r.y, r.delay)).collect(),
            data: cmd.data,
            started: now,
            next_at: now,
            finished: false,
        })
    }

    fn factor(&self, row: Option<u16>, elapsed: u64) -> Option<f32> {
        let y = row?;
        let delay = self.delays.iter().find(|(ry, _)| *ry == y)?.1;
        let raw = elapsed.saturating_sub(delay) as f32 / self.ms as f32;
        let eased = ease(&self.ease_name, raw);
        Some(if self.inward { eased } else { 1.0 - eased })
    }

    pub fn total_ms(&self) -> u64 {
        self.ms
            .saturating_add(self.delays.iter().map(|(_, d)| *d).max().unwrap_or(0))
    }

    /// The frame for `elapsed`; a late wake skips the ticks it missed instead of replaying them.
    pub fn frame_at(&self, elapsed: u64) -> String {
        if elapsed >= self.total_ms() && self.inward {
            return self.data.clone();
        }
        let mut out = String::with_capacity(self.data.len() + 64);
        for (row, from, to) in segments(&self.data) {
            let seg = &self.data[from..to];
            match self.factor(row, elapsed) {
                Some(t) => shade(seg, self.anchor, t, &mut out),
                None => out.push_str(seg),
            }
        }
        out
    }

    pub fn tick(&mut self, now: Instant) -> Option<String> {
        if self.finished || now < self.next_at {
            return None;
        }
        let elapsed = now.duration_since(self.started).as_millis() as u64;
        self.next_at = now + self.frame;
        if elapsed >= self.total_ms() {
            self.finished = true;
        }
        Some(self.frame_at(elapsed))
    }

    pub fn finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(data: &str) -> AnimCmd {
        AnimCmd {
            id: 1,
            ms: 100,
            fps: Some(30),
            ease: Some("linear".into()),
            dir: Some("in".into()),
            anchor: "#000000".into(),
            rows: vec![AnimRow { y: 3, delay: 0 }],
            data: data.into(),
        }
    }

    fn run(data: &str) -> Run {
        Run::new(cmd(data), Instant::now()).unwrap()
    }

    const ROW: &str = "\x1b[3;1H\x1b[48;2;40;40;40m\x1b[38;2;200;100;50mhi\x1b[0m";

    #[test]
    fn interpolates_both_truecolour_forms_toward_the_anchor() {
        let r = run(ROW);
        assert_eq!(
            r.frame_at(0),
            "\x1b[3;1H\x1b[48;2;0;0;0m\x1b[38;2;0;0;0mhi\x1b[0m"
        );
        assert_eq!(
            r.frame_at(50),
            "\x1b[3;1H\x1b[48;2;20;20;20m\x1b[38;2;100;50;25mhi\x1b[0m"
        );
        assert_eq!(
            r.frame_at(100),
            ROW,
            "the last frame is the payload verbatim"
        );
    }

    #[test]
    fn out_runs_from_the_target_to_the_anchor() {
        let mut c = cmd(ROW);
        c.dir = Some("out".into());
        let r = Run::new(c, Instant::now()).unwrap();
        assert_eq!(r.frame_at(0), ROW);
        assert_eq!(
            r.frame_at(100),
            "\x1b[3;1H\x1b[48;2;0;0;0m\x1b[38;2;0;0;0mhi\x1b[0m"
        );
    }

    #[test]
    fn text_cup_bold_and_reset_pass_through() {
        let data = "\x1b[?25l\x1b[3;1H\x1b[1m\x1b[38;2;255;255;255m▎ tab\x1b[22m\x1b[0m";
        let r = run(data);
        let out = r.frame_at(0);
        assert!(out.starts_with("\x1b[?25l\x1b[3;1H\x1b[1m"), "{out:?}");
        assert!(out.contains("▎ tab\x1b[22m\x1b[0m"), "{out:?}");
        assert!(out.contains("\x1b[38;2;0;0;0m"), "{out:?}");
    }

    #[test]
    fn rows_not_listed_are_left_alone() {
        let data = format!("{ROW}\x1b[9;1H\x1b[38;2;10;20;30mother\x1b[0m");
        let r = run(&data);
        assert!(
            r.frame_at(0).contains("\x1b[9;1H\x1b[38;2;10;20;30mother"),
            "unlisted row untouched"
        );
    }

    #[test]
    fn per_row_delay_staggers_the_start() {
        let mut c = cmd(&format!("{ROW}\x1b[4;1H\x1b[38;2;100;100;100mtwo\x1b[0m"));
        c.rows = vec![AnimRow { y: 3, delay: 0 }, AnimRow { y: 4, delay: 50 }];
        let r = Run::new(c, Instant::now()).unwrap();
        let at50 = r.frame_at(50);
        assert!(at50.contains("\x1b[38;2;100;50;25m"), "row 3 is halfway");
        assert!(
            at50.contains("\x1b[4;1H\x1b[38;2;0;0;0m"),
            "row 4 has not started"
        );
        assert_eq!(r.total_ms(), 150, "the stagger extends the run");
    }

    #[test]
    fn easing_curves_differ_and_unknown_is_linear() {
        assert_eq!(ease("linear", 0.5), 0.5);
        assert!(ease("outCubic", 0.5) > 0.8);
        assert!(ease("inOutQuad", 0.25) < 0.25);
        assert_eq!(ease("nope", 0.25), 0.25);
        assert_eq!(ease("outCubic", 1.0), 1.0);
        assert_eq!(ease("inOutQuad", 1.0), 1.0);
    }

    #[test]
    fn bounds_are_enforced() {
        let mut big = cmd("x");
        big.data = "x".repeat(MAX_DATA + 1);
        assert_eq!(Run::new(big, Instant::now()).err(), Some(Rejected::Size));

        let mut slow = cmd(ROW);
        slow.ms = MAX_MS + 1;
        assert_eq!(Run::new(slow, Instant::now()).err(), Some(Rejected::Bounds));

        let mut fast = cmd(ROW);
        fast.fps = Some(120);
        assert_eq!(Run::new(fast, Instant::now()).err(), Some(Rejected::Bounds));

        let mut colour = cmd(ROW);
        colour.anchor = "1e1e2e".into();
        assert_eq!(
            Run::new(colour, Instant::now()).err(),
            Some(Rejected::Bounds)
        );

        let mut empty = cmd(ROW);
        empty.rows = vec![];
        assert_eq!(
            Run::new(empty, Instant::now()).err(),
            Some(Rejected::Bounds)
        );
    }

    #[test]
    fn a_delay_past_the_duration_cap_is_refused() {
        let mut c = cmd(ROW);
        c.rows = vec![AnimRow {
            y: 3,
            delay: u64::MAX,
        }];
        assert_eq!(Run::new(c, Instant::now()).err(), Some(Rejected::Bounds));

        let mut ok = cmd(ROW);
        ok.rows = vec![AnimRow {
            y: 3,
            delay: MAX_MS,
        }];
        let run = Run::new(ok, Instant::now()).expect("a delay inside the cap plays");
        assert_eq!(run.total_ms(), 100 + MAX_MS, "and total_ms does not wrap");
    }

    #[test]
    fn a_late_wake_skips_to_now_and_finishes() {
        let start = Instant::now();
        let mut r = Run::new(cmd(ROW), start).unwrap();
        assert!(r.tick(start).is_some(), "the first frame is immediate");
        assert!(r.tick(start).is_none(), "not due yet");
        let late = start + Duration::from_millis(500);
        assert_eq!(
            r.tick(late).as_deref(),
            Some(ROW),
            "jumps straight to the end"
        );
        assert!(r.finished());
        assert!(r.tick(late).is_none(), "a finished run writes nothing more");
    }

    #[test]
    fn the_pre_cup_prefix_passes_through_untouched() {
        // ansi.lua puts HIDE_CURSOR before the first CUP; it belongs to no row and must not lerp
        let data = format!("\x1b[?25l\x1b[38;2;7;7;7m{ROW}");
        let r = run(&data);
        let out = r.frame_at(0);
        assert!(
            out.starts_with("\x1b[?25l\x1b[38;2;7;7;7m"),
            "prefix rewritten: {out:?}"
        );
        assert!(out.contains("\x1b[3;1H\x1b[48;2;0;0;0m"), "row still lerps");
    }

    #[test]
    fn a_frame_with_no_cup_is_left_whole() {
        let r = run("\x1b[38;2;9;9;9mbare\x1b[0m");
        assert_eq!(r.frame_at(0), "\x1b[38;2;9;9;9mbare\x1b[0m");
    }
}

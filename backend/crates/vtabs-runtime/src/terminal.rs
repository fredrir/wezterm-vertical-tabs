use std::io::{self, IsTerminal, Write};

use crossterm::{cursor, execute, terminal};

const ENABLE_MODES: &str =
    "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?1004h\x1b[?2004h\x1b[?7l";
const DISABLE_MODES: &str =
    "\x1b[?7h\x1b[?2004l\x1b[?1004l\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

/// Restores the terminal on drop, including during panic unwinding.
pub struct TerminalGuard {
    raw: bool,
}

impl TerminalGuard {
    pub fn enter(out: &mut impl Write, bg: Option<(u8, u8, u8)>) -> io::Result<Self> {
        let raw = io::stdin().is_terminal() && terminal::enable_raw_mode().is_ok();
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
        if let Some(bg) = bg {
            out.write_all(fill(bg).as_bytes())?;
        }
        out.write_all(ENABLE_MODES.as_bytes())?;
        out.flush()?;
        Ok(Self { raw })
    }

    pub fn is_raw(&self) -> bool {
        self.raw
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout().lock();
        let _ = out.write_all(DISABLE_MODES.as_bytes());
        let _ = execute!(out, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = out.flush();
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

/// Paints the whole pane before the first frame arrives, so a new sidebar never flashes.
fn fill(bg: (u8, u8, u8)) -> String {
    let (r, g, b) = bg;
    format!("\x1b[48;2;{r};{g};{b}m\x1b[2J\x1b[H\x1b[0m")
}

pub fn parse_bg(spec: &str) -> Option<(u8, u8, u8)> {
    let hex = spec.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

pub fn size() -> Option<(u16, u16)> {
    terminal::size().ok()
}

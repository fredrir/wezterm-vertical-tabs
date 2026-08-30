use std::io::{self, IsTerminal, Write};

use crossterm::{cursor, execute, terminal};

const ENABLE_MODES: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?1004h";
const DISABLE_MODES: &str = "\x1b[?1004l\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

/// Restores the terminal on drop, including during panic unwinding.
pub struct TerminalGuard {
    raw: bool,
}

impl TerminalGuard {
    pub fn enter(out: &mut impl Write) -> io::Result<Self> {
        let raw = io::stdin().is_terminal() && terminal::enable_raw_mode().is_ok();
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
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

pub fn size() -> Option<(u16, u16)> {
    terminal::size().ok()
}

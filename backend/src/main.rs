use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use wez_vertical_tabs_backend::app::App;
use wez_vertical_tabs_backend::event::Event;
use wez_vertical_tabs_backend::log::Logger;
use wez_vertical_tabs_backend::parser::Parser;
use wez_vertical_tabs_backend::signal;
use wez_vertical_tabs_backend::terminal::{self, TerminalGuard};
use wez_vertical_tabs_backend::uservar::{
    DEFAULT_VAR, ROLE, ROLE_VAR, nonce, set_user_var, title_marker,
};

const SIZE_POLL: Duration = Duration::from_millis(250);
const WINCH_CHECK: Duration = Duration::from_millis(100);
const SIZE_FALLBACK: Duration = Duration::from_secs(2);
const ESC_TIMEOUT: Duration = Duration::from_millis(30);
const READ_BUF: usize = 16 * 1024;
fn spawn_stdin_reader() -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut buf = vec![0u8; READ_BUF];
        loop {
            match stdin.read(&mut buf) {
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

fn run() -> io::Result<()> {
    let mut log = Logger::from_env();
    let var = std::env::var("VTABS_USERVAR").unwrap_or_else(|_| DEFAULT_VAR.to_string());
    let bg = std::env::var("VTABS_BG")
        .ok()
        .and_then(|spec| terminal::parse_bg(&spec));
    let mut out = io::stdout().lock();
    let guard = TerminalGuard::enter(&mut out, bg)?;
    if !guard.is_raw() {
        log.log("stdin is not a tty; raw mode skipped");
    }
    let size = terminal::size().unwrap_or((0, 0));
    let mut app = App {
        out,
        log,
        var,
        size,
        anim: None,
    };
    app.write(set_user_var(ROLE_VAR, ROLE).as_bytes())?;
    // Marker only: it lets the plugin find this pane again, it proves nothing and carries no token.
    app.write(title_marker(&nonce()).as_bytes())?;
    app.emit(&Event::ready(size.0, size.1))?;

    let rx = spawn_stdin_reader();
    let mut parser = Parser::new();
    let winch = signal::watch_resize();
    let tick = if winch { WINCH_CHECK } else { SIZE_POLL };
    let mut next_tick = Instant::now() + tick;
    let mut next_full = Instant::now() + SIZE_FALLBACK;
    loop {
        let mut deadline = next_tick.min(next_full);
        if let Some(at) = app.next_anim() {
            deadline = deadline.min(at);
        }
        let until_tick = deadline.saturating_duration_since(Instant::now());
        let timeout = if parser.has_pending() {
            until_tick.min(ESC_TIMEOUT)
        } else {
            until_tick
        };
        let inputs = match rx.recv_timeout(timeout) {
            Ok(chunk) => parser.feed(&chunk),
            Err(RecvTimeoutError::Timeout) => parser.flush(),
            Err(RecvTimeoutError::Disconnected) => break,
        };
        for input in inputs {
            if !app.handle(input)? {
                return Ok(());
            }
        }
        let now = Instant::now();
        app.tick_anim(now)?;
        if now >= next_tick {
            next_tick = now + tick;
            if !winch || signal::resized() {
                app.poll_size()?;
            }
        }
        if now >= next_full {
            next_full = now + SIZE_FALLBACK;
            app.poll_size()?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        Logger::from_env().log(format!("exit: {err}"));
        std::process::exit(1);
    }
}

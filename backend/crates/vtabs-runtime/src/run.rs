use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use vtabs_input::Parser;
use vtabs_protocol::Event;

use crate::app::App;
use crate::log::Logger;
use crate::signal;
use crate::terminal::{self, TerminalGuard};
use crate::uservar::{DEFAULT_VAR, ROLE_VAR, Role, nonce, set_user_var, title_marker};

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

/// `--role sidebar|settings`; anything else keeps the default and says so in the log.
fn role_from_args(log: &mut Logger) -> Role {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = match arg.strip_prefix("--role=") {
            Some(v) => Some(v.to_string()),
            None if arg == "--role" => args.next(),
            None => None,
        };
        if let Some(value) = value {
            return Role::parse(&value).unwrap_or_else(|| {
                log.log(format!("unknown role {value}; using sidebar"));
                Role::default()
            });
        }
    }
    Role::default()
}

pub fn run() -> io::Result<()> {
    let mut log = Logger::from_env();
    let role = role_from_args(&mut log);
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
    // The settings screen keeps the v1 frame path until P6, so only the sidebar announces paints.
    let paints = role == Role::Sidebar;
    let mut app = App {
        out,
        log,
        var,
        size,
        anim: None,
        seq: 0,
        v2: crate::app::V2State::default(),
        paints,
        ui: Default::default(),
        started: Instant::now(),
        popover: None,
        hover_deadline: None,
        token: None,
    };
    app.write(set_user_var(ROLE_VAR, role.name()).as_bytes())?;
    // Marker only: it lets the plugin find this pane again, it proves nothing and carries no token.
    app.write(title_marker(role, &nonce()).as_bytes())?;
    app.emit(&Event::ready(size.0, size.1, paints))?;

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
        if let Some(at) = app.next_hover() {
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
        app.tick_hover(now)?;
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

use std::io::{self, Read};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
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
const SIZE_FALLBACK: Duration = Duration::from_secs(2);
const ESC_TIMEOUT: Duration = Duration::from_millis(30);
const READ_BUF: usize = 16 * 1024;

/// Everything that wakes the loop, on one channel: stdin bytes, the pty's end, and a SIGWINCH.
enum Wake {
    Stdin(Vec<u8>),
    Eof,
    Resized,
}

fn spawn_stdin_reader(tx: Sender<Wake>) {
    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut buf = vec![0u8; READ_BUF];
        loop {
            match stdin.read(&mut buf) {
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Ok(0) | Err(_) => {
                    let _ = tx.send(Wake::Eof);
                    break;
                }
                Ok(n) => {
                    if tx.send(Wake::Stdin(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
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

/// Logs the panic and its backtrace before the default hook prints them to the pane.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut log = Logger::from_env();
        let at = info
            .location()
            .map_or(String::from("?"), ToString::to_string);
        let msg = info.payload_as_str().unwrap_or("non-string payload");
        log.log(format!("panic at {at}: {msg}"));
        for line in std::backtrace::Backtrace::force_capture()
            .to_string()
            .lines()
        {
            log.log(line);
        }
        default(info);
    }));
}

pub fn run() -> io::Result<()> {
    install_panic_hook();
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
    let mut app = App {
        out,
        log,
        var,
        size,
        probe: terminal::size,
        needs_clear: false,
        fx: None,
        last_rows: None,
        shown_is_final: false,
        seq: 0,
        v2: crate::app::V2State::default(),
        ui: Default::default(),
        started: Instant::now(),
        popover: None,
        menu_ui: Default::default(),
        settings_ui: Default::default(),
        noted_menu: None,
        hover_deadline: None,
        token: None,
        cli: crate::cli::Cli::from_env(),
    };
    app.write(set_user_var(ROLE_VAR, role.name()).as_bytes())?;
    // Marker only: it lets the plugin find this pane again, it proves nothing and carries no token.
    app.write(title_marker(role, &nonce()).as_bytes())?;
    // `paints` is the capability flip Lua reads to refuse a backend that cannot draw for itself.
    app.emit(&Event::ready(size.0, size.1))?;
    if cfg!(debug_assertions) && std::env::var_os("VTABS_PANIC_ON_READY").is_some_and(|v| v == "1")
    {
        panic!("VTABS_PANIC_ON_READY");
    }

    let (tx, rx) = mpsc::channel();
    spawn_stdin_reader(tx.clone());
    let winch = signal::watch_resize(Box::new(move || tx.send(Wake::Resized).is_ok()));
    let mut parser = Parser::new();
    // Without SIGWINCH the size is polled; with it, only the slow fallback sweep remains.
    let mut next_poll = (!winch).then(|| Instant::now() + SIZE_POLL);
    let mut next_full = Instant::now() + SIZE_FALLBACK;
    loop {
        let mut deadline = next_poll.map_or(next_full, |at| at.min(next_full));
        if let Some(at) = app.next_fx() {
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
            Ok(Wake::Stdin(chunk)) => parser.feed(&chunk),
            Ok(Wake::Resized) => {
                app.poll_size()?;
                Vec::new()
            }
            Err(RecvTimeoutError::Timeout) => parser.flush(),
            Ok(Wake::Eof) | Err(RecvTimeoutError::Disconnected) => break,
        };
        for input in inputs {
            if !app.handle(input)? {
                return Ok(());
            }
        }
        let now = Instant::now();
        app.tick_fx(now)?;
        app.tick_hover(now)?;
        if next_poll.is_some_and(|at| now >= at) {
            next_poll = Some(now + SIZE_POLL);
            app.poll_size()?;
        }
        if now >= next_full {
            next_full = now + SIZE_FALLBACK;
            app.poll_size()?;
        }
    }
    Ok(())
}

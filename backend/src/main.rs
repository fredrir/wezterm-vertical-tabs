use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use wez_vertical_tabs_backend::app::App;
use wez_vertical_tabs_backend::event::Event;
use wez_vertical_tabs_backend::log::Logger;
use wez_vertical_tabs_backend::parser::Parser;
use wez_vertical_tabs_backend::terminal::{self, TerminalGuard};
use wez_vertical_tabs_backend::uservar::{DEFAULT_VAR, ROLE, ROLE_VAR, set_user_var};

const SIZE_POLL: Duration = Duration::from_millis(250);
const ESC_TIMEOUT: Duration = Duration::from_millis(30);
const READ_BUF: usize = 16 * 1024;
fn spawn_stdin_reader() -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut buf = vec![0u8; READ_BUF];
        loop {
            match stdin.read(&mut buf) {
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
    let mut out = io::stdout().lock();
    let guard = TerminalGuard::enter(&mut out)?;
    if !guard.is_raw() {
        log.log("stdin is not a tty; raw mode skipped");
    }
    let size = terminal::size().unwrap_or((0, 0));
    let mut app = App {
        out,
        log,
        var,
        size,
    };
    app.write(set_user_var(ROLE_VAR, ROLE).as_bytes())?;
    app.emit(&Event::Ready {
        cols: size.0,
        rows: size.1,
    })?;

    let rx = spawn_stdin_reader();
    let mut parser = Parser::new();
    let mut next_poll = Instant::now() + SIZE_POLL;
    loop {
        let until_poll = next_poll.saturating_duration_since(Instant::now());
        let timeout = if parser.has_pending() {
            until_poll.min(ESC_TIMEOUT)
        } else {
            until_poll
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
        if Instant::now() >= next_poll {
            app.poll_size()?;
            next_poll = Instant::now() + SIZE_POLL;
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

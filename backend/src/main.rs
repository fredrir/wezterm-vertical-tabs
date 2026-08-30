use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use wez_vertical_tabs_backend::command::Command;
use wez_vertical_tabs_backend::event::Event;
use wez_vertical_tabs_backend::log::Logger;
use wez_vertical_tabs_backend::parser::{Input, Parser};
use wez_vertical_tabs_backend::terminal::{self, TerminalGuard};
use wez_vertical_tabs_backend::uservar::{DEFAULT_VAR, ROLE, ROLE_VAR, set_user_var};

const SIZE_POLL: Duration = Duration::from_millis(250);
const ESC_TIMEOUT: Duration = Duration::from_millis(30);
const READ_BUF: usize = 16 * 1024;
const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";

/// Sole stdout writer, so user-var OSCs never interleave with frame bytes.
struct App<W: Write> {
    out: W,
    log: Logger,
    var: String,
    size: (u16, u16),
}

impl<W: Write> App<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.out.flush()
    }

    fn emit(&mut self, event: &Event) -> io::Result<()> {
        let json = event.to_json();
        self.log.log(format!("event {json}"));
        self.write(set_user_var(&self.var, &json).as_bytes())
    }

    fn handle(&mut self, input: Input) -> io::Result<bool> {
        match input {
            Input::Mouse(m) => self.emit(&Event::from(m))?,
            Input::Focus(focused) => self.emit(&Event::Focus { focused })?,
            Input::Key { name, mods } => self.emit(&Event::key(name, mods))?,
            Input::Command(cmd) => return self.run(cmd),
        }
        Ok(true)
    }

    fn run(&mut self, cmd: Command) -> io::Result<bool> {
        match cmd {
            Command::Frame { data } => self.write(data.as_bytes())?,
            Command::Clear => self.write(CLEAR_SCREEN.as_bytes())?,
            Command::Ping => self.emit(&Event::Pong)?,
            Command::Quit => return Ok(false),
        }
        Ok(true)
    }

    fn poll_size(&mut self) -> io::Result<()> {
        let Some(size) = terminal::size() else {
            return Ok(());
        };
        if size != self.size {
            self.size = size;
            self.emit(&Event::Resize {
                cols: size.0,
                rows: size.1,
            })?;
        }
        Ok(())
    }
}

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

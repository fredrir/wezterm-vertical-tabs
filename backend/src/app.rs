use std::io::{self, Write};

use crate::command::Command;
use crate::event::Event;
use crate::log::Logger;
use crate::parser::Input;
use crate::terminal;
use crate::uservar::{TOKEN_VAR, set_user_var};

const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";

/// Sole stdout writer, so user-var OSCs never interleave with frame bytes.
pub struct App<W: Write> {
    pub out: W,
    pub log: Logger,
    pub var: String,
    pub size: (u16, u16),
}

impl<W: Write> App<W> {
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.out.flush()
    }

    pub fn emit(&mut self, event: &Event) -> io::Result<()> {
        let json = event.to_json();
        self.log.log(match event {
            Event::Key { .. } => "event key".to_string(),
            _ => format!("event {json}"),
        });
        self.write(set_user_var(&self.var, &json).as_bytes())
    }

    /// Returns `Ok(false)` when the backend should exit.
    pub fn handle(&mut self, input: Input) -> io::Result<bool> {
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
            Command::Auth { token } => self.write(set_user_var(TOKEN_VAR, &token).as_bytes())?,
            Command::Quit => return Ok(false),
        }
        Ok(true)
    }

    pub fn poll_size(&mut self) -> io::Result<()> {
        let Some(size) = terminal::size() else {
            return Ok(());
        };
        self.resize(size)
    }

    fn resize(&mut self, size: (u16, u16)) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Mods;

    fn app() -> App<Vec<u8>> {
        App {
            out: Vec::new(),
            log: Logger::from_env(),
            var: "vtabs".into(),
            size: (28, 24),
        }
    }

    #[test]
    fn frame_is_written_verbatim() {
        let mut a = app();
        assert!(
            a.handle(Input::Command(Command::Frame {
                data: "\x1b[1;1Hhi".into()
            }))
            .unwrap()
        );
        assert_eq!(a.out, b"\x1b[1;1Hhi");
    }

    #[test]
    fn quit_stops_the_loop() {
        let mut a = app();
        assert!(!a.handle(Input::Command(Command::Quit)).unwrap());
        assert!(a.out.is_empty());
    }

    #[test]
    fn ping_answers_with_pong_user_var() {
        let mut a = app();
        a.handle(Input::Command(Command::Ping)).unwrap();
        assert_eq!(a.out, set_user_var("vtabs", r#"{"t":"pong"}"#).as_bytes());
    }

    #[test]
    fn auth_echoes_token_as_user_var() {
        let mut a = app();
        a.handle(Input::Command(Command::Auth {
            token: "abc".into(),
        }))
        .unwrap();
        assert_eq!(a.out, set_user_var("vtabs_token", "abc").as_bytes());
    }

    #[test]
    fn resize_emits_only_on_change() {
        let mut a = app();
        a.resize((28, 24)).unwrap();
        assert!(a.out.is_empty());
        a.resize((30, 24)).unwrap();
        assert_eq!(
            a.out,
            set_user_var("vtabs", r#"{"t":"resize","cols":30,"rows":24}"#).as_bytes()
        );
    }

    #[test]
    fn key_events_are_emitted() {
        let mut a = app();
        a.handle(Input::Key {
            name: "x".into(),
            mods: Mods::default(),
        })
        .unwrap();
        assert_eq!(
            a.out,
            set_user_var("vtabs", r#"{"t":"key","key":"x"}"#).as_bytes()
        );
    }
}

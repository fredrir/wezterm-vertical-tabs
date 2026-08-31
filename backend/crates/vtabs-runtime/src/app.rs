use std::io::{self, Write};
use std::time::Instant;

use vtabs_input::Input;
use vtabs_protocol::limits::{MENU_MAX_ITEMS, MODEL_MAX_TABS};
use vtabs_protocol::{Command, Event, v2};

use crate::anim;
use crate::log::Logger;
use crate::terminal;
use crate::uservar::{TOKEN_VAR, set_user_var};

const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";

/// Sole stdout writer, so user-var OSCs never interleave with frame bytes.
pub struct App<W: Write> {
    pub out: W,
    pub log: Logger,
    pub var: String,
    pub size: (u16, u16),
    pub anim: Option<anim::Run>,
    pub seq: u64,
    pub v2: V2State,
}

/// Latest v2 state, stored whole per message kind; a bounds breach keeps the previous one.
#[derive(Default)]
pub struct V2State {
    pub config: Option<v2::ConfigMsg>,
    pub theme: Option<v2::ThemeMsg>,
    pub model: Option<v2::ModelMsg>,
    pub menu: Option<v2::MenuMsg>,
}

impl<W: Write> App<W> {
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.out.flush()
    }

    pub fn emit(&mut self, event: &Event) -> io::Result<()> {
        let mut json = event.to_json();
        // WezTerm fires user-var-changed only on a value change; without n, repeated identical
        // events are silently dropped. Pong is spared: it echoes the ping's own varying n.
        if !matches!(event, Event::Pong { .. }) {
            self.seq += 1;
            json.truncate(json.len() - 1);
            json.push_str(&format!(",\"n\":{}}}", self.seq));
        }
        self.log.log(match event {
            Event::Key { .. } => "event key".to_string(),
            Event::Paste { data, .. } => {
                format!("event paste {} bytes", data.as_ref().map_or(0, String::len))
            }
            _ => format!("event {json}"),
        });
        self.write(set_user_var(&self.var, &json).as_bytes())
    }

    /// Returns `Ok(false)` when the backend should exit.
    pub fn handle(&mut self, input: Input) -> io::Result<bool> {
        match input {
            Input::Mouse(m) => self.emit(&Event::from(m))?,
            Input::Focus(focused) => self.emit(&Event::Focus { focused })?,
            Input::Key { name, mods, raw } => self.emit(&Event::key(name, mods, &raw))?,
            Input::Paste(data) => self.emit(&Event::paste(data))?,
            Input::Command(cmd) => return self.run(cmd),
        }
        Ok(true)
    }

    fn run(&mut self, cmd: Command) -> io::Result<bool> {
        match cmd {
            Command::Frame { data } => {
                self.cancel_anim()?;
                self.write(data.as_bytes())?
            }
            Command::Clear => {
                self.cancel_anim()?;
                self.write(CLEAR_SCREEN.as_bytes())?
            }
            Command::Ping { n } => self.emit(&Event::Pong { n })?,
            Command::Auth { token } => self.write(set_user_var(TOKEN_VAR, &token).as_bytes())?,
            Command::Anim(cmd) => self.start_anim(cmd)?,
            Command::Config(msg) => self.v2.config = Some(*msg),
            Command::Theme(msg) => self.v2.theme = Some(*msg),
            Command::Model(msg) => {
                if msg.tabs.len() > MODEL_MAX_TABS {
                    self.emit(&Event::Dropped {
                        what: "model",
                        reason: "bounds",
                    })?;
                } else {
                    self.v2.model = Some(*msg);
                }
            }
            Command::Menu(msg) => {
                if msg.items.len() > MENU_MAX_ITEMS {
                    self.emit(&Event::Dropped {
                        what: "menu",
                        reason: "bounds",
                    })?;
                } else {
                    self.v2.menu = Some(*msg);
                }
            }
            // Rendering from this state lands in P4b; storing first keeps the wire testable now.
            Command::Fx(msg) => self.log.log(format!("fx {}", msg.phase)),
            Command::Notice(msg) => self.log.log(format!("notice {}", msg.text)),
            Command::Quit => {
                self.cancel_anim()?;
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Lua always wins: whatever was playing ends where it stands and is reported done.
    fn cancel_anim(&mut self) -> io::Result<()> {
        if let Some(run) = self.anim.take() {
            self.emit(&Event::AnimDone { id: run.id })?;
        }
        Ok(())
    }

    fn start_anim(&mut self, cmd: vtabs_protocol::AnimCmd) -> io::Result<()> {
        self.cancel_anim()?;
        let id = cmd.id;
        match anim::Run::new(cmd, Instant::now()) {
            Ok(mut run) => {
                if let Some(frame) = run.tick(Instant::now()) {
                    self.write(frame.as_bytes())?;
                }
                if run.finished() {
                    self.emit(&Event::AnimDone { id })?;
                } else {
                    self.anim = Some(run);
                }
            }
            Err(why) => {
                self.log.log(format!("anim {id} dropped: {}", why.reason()));
                self.emit(&Event::Dropped {
                    what: "anim",
                    reason: why.reason(),
                })?;
            }
        }
        Ok(())
    }

    pub fn next_anim(&self) -> Option<Instant> {
        self.anim.as_ref().map(|run| run.next_at)
    }

    /// Writes the frame for `now`, skipping any tick the loop slept through.
    pub fn tick_anim(&mut self, now: Instant) -> io::Result<()> {
        let Some(run) = self.anim.as_mut() else {
            return Ok(());
        };
        let frame = run.tick(now);
        let done = run.finished();
        let id = run.id;
        if let Some(frame) = frame {
            self.write(frame.as_bytes())?;
        }
        if done {
            self.anim = None;
            self.emit(&Event::AnimDone { id })?;
        }
        Ok(())
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
    use vtabs_protocol::Mods;

    fn app() -> App<Vec<u8>> {
        App {
            out: Vec::new(),
            log: Logger::from_env(),
            var: "vtabs".into(),
            size: (28, 24),
            anim: None,
            seq: 0,
            v2: V2State::default(),
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
        a.handle(Input::Command(Command::Ping { n: Some(7) }))
            .unwrap();
        assert_eq!(
            a.out,
            set_user_var("vtabs", r#"{"t":"pong","n":7}"#).as_bytes()
        );
    }

    #[test]
    fn auth_echoes_token_as_user_var() {
        let mut a = app();
        a.handle(Input::Command(Command::Auth {
            token: "abc".into(),
        }))
        .unwrap();
        // the token never goes in the title: window titles are readable by the whole desktop
        assert_eq!(a.out, set_user_var("vtabs_token", "abc").as_bytes());
    }

    fn anim_cmd(id: u64, ms: u64) -> Command {
        Command::Anim(vtabs_protocol::AnimCmd {
            id,
            ms,
            fps: Some(30),
            ease: Some("linear".into()),
            dir: Some("in".into()),
            anchor: "#000000".into(),
            rows: vec![vtabs_protocol::AnimRow { y: 3, delay: 0 }],
            data: "\x1b[3;1H\x1b[38;2;200;200;200mhi\x1b[0m".into(),
        })
    }

    fn payloads(a: &App<Vec<u8>>) -> Vec<String> {
        use base64::Engine as _;
        String::from_utf8_lossy(&a.out)
            .split("\x1b]1337;SetUserVar=vtabs=")
            .skip(1)
            .filter_map(|rest| rest.split('\x07').next())
            .map(|b| {
                let bytes = base64::engine::general_purpose::STANDARD.decode(b).unwrap();
                String::from_utf8(bytes).unwrap()
            })
            .collect()
    }

    fn saw(a: &App<Vec<u8>>, needle: &str) -> bool {
        payloads(a).iter().any(|p| p.contains(needle))
    }

    #[test]
    fn an_anim_writes_its_first_frame_at_once_and_keeps_running() {
        let mut a = app();
        a.handle(Input::Command(anim_cmd(1, 100))).unwrap();
        assert!(a.anim.is_some(), "still playing");
        let painted = String::from_utf8_lossy(&a.out).to_string();
        assert!(painted.contains("\x1b[38;2;0;0;0m"), "t=0 frame written");
        assert!(!saw(&a, r#"{"t":"anim_done","id":1"#), "not done yet");
    }

    #[test]
    fn a_frame_command_cancels_the_run_and_reports_it_done() {
        let mut a = app();
        a.handle(Input::Command(anim_cmd(2, 500))).unwrap();
        a.handle(Input::Command(Command::Frame { data: "x".into() }))
            .unwrap();
        assert!(a.anim.is_none(), "cancelled");
        assert!(saw(&a, r#"{"t":"anim_done","id":2"#));
    }

    #[test]
    fn a_new_anim_cancels_the_old_one() {
        let mut a = app();
        a.handle(Input::Command(anim_cmd(3, 500))).unwrap();
        a.handle(Input::Command(anim_cmd(4, 500))).unwrap();
        assert!(saw(&a, r#"{"t":"anim_done","id":3"#));
        assert_eq!(a.anim.as_ref().map(|r| r.id), Some(4));
    }

    #[test]
    fn clear_and_quit_cancel_too() {
        let mut a = app();
        a.handle(Input::Command(anim_cmd(5, 500))).unwrap();
        a.handle(Input::Command(Command::Clear)).unwrap();
        assert!(saw(&a, r#"{"t":"anim_done","id":5"#));

        let mut b = app();
        b.handle(Input::Command(anim_cmd(6, 500))).unwrap();
        assert!(!b.handle(Input::Command(Command::Quit)).unwrap());
        assert!(saw(&b, r#"{"t":"anim_done","id":6"#));
    }

    #[test]
    fn an_oversized_anim_is_dropped_with_a_reason() {
        let mut a = app();
        let Command::Anim(mut cmd) = anim_cmd(7, 100) else {
            unreachable!()
        };
        cmd.data = "x".repeat(crate::anim::MAX_DATA + 1);
        a.handle(Input::Command(Command::Anim(cmd))).unwrap();
        assert!(a.anim.is_none(), "nothing plays");
        assert!(saw(&a, r#"{"t":"dropped","what":"anim","reason":"size""#));
    }

    #[test]
    fn a_finished_run_emits_done_once() {
        let mut a = app();
        a.handle(Input::Command(anim_cmd(8, 30))).unwrap();
        let late = std::time::Instant::now() + std::time::Duration::from_millis(200);
        a.tick_anim(late).unwrap();
        assert!(a.anim.is_none());
        assert!(saw(&a, r#"{"t":"anim_done","id":8"#));
        let before = a.out.len();
        a.tick_anim(late).unwrap();
        assert_eq!(a.out.len(), before, "nothing more is written");
    }

    #[test]
    fn resize_emits_only_on_change() {
        let mut a = app();
        a.resize((28, 24)).unwrap();
        assert!(a.out.is_empty());
        a.resize((30, 24)).unwrap();
        assert_eq!(
            a.out,
            set_user_var("vtabs", r#"{"t":"resize","cols":30,"rows":24,"n":1}"#).as_bytes()
        );
    }

    #[test]
    fn repeated_identical_keys_stay_distinct_user_vars() {
        let mut a = app();
        for _ in 0..2 {
            a.handle(Input::Key {
                name: "j".into(),
                mods: Mods::default(),
                raw: b"j".to_vec(),
            })
            .unwrap();
        }
        assert_eq!(
            payloads(&a),
            vec![
                r#"{"t":"key","key":"j","raw":"ag==","n":1}"#.to_string(),
                r#"{"t":"key","key":"j","raw":"ag==","n":2}"#.to_string(),
            ]
        );
    }

    #[test]
    fn pong_echoes_the_ping_n_untouched() {
        let mut a = app();
        a.handle(Input::Key {
            name: "j".into(),
            mods: Mods::default(),
            raw: b"j".to_vec(),
        })
        .unwrap();
        a.handle(Input::Command(Command::Ping { n: Some(7) }))
            .unwrap();
        assert_eq!(payloads(&a)[1], r#"{"t":"pong","n":7}"#);
    }

    #[test]
    fn key_events_are_emitted() {
        let mut a = app();
        a.handle(Input::Key {
            name: "x".into(),
            mods: Mods::default(),
            raw: b"x".to_vec(),
        })
        .unwrap();
        assert_eq!(
            a.out,
            set_user_var("vtabs", r#"{"t":"key","key":"x","raw":"eA==","n":1}"#).as_bytes()
        );
    }
}

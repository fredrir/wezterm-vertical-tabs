use std::io::{self, Write};
use std::time::Instant;

use vtabs_core::ui::UiState;
use vtabs_input::Input;
use vtabs_input::resolve::{self, Knobs, MenuView, MirroredDrag};
use vtabs_protocol::limits::{MENU_MAX_ITEMS, MODEL_MAX_TABS};
use vtabs_protocol::{Command, Event, v2};
use vtabs_view::enrich::{Enriched, PopoverHits, enrich, theme_of};
use vtabs_view::layout;
use vtabs_view::menu::{self, MenuCfg, MenuState, Outcome};
use vtabs_view::render::frame_of;

use crate::anim;
use crate::log::Logger;
use crate::paint::frame_bytes;
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
    /// True for the sidebar role: this process owns the pane's pixels and Lua sends it no frames.
    pub paints: bool,
    pub ui: UiState,
    pub started: Instant,
    /// Where the menu Lua composed sits, from the last frame; the bridge reports against it.
    pub popover: Option<PopoverHits>,
    /// The menu's own state: the selection Lua no longer drives and the rename buffer Rust owns.
    pub menu_ui: MenuState,
    /// The menu rev a `menu_refused` was already sent for, so a resize does not repeat it.
    pub noted_menu: Option<u64>,
    pub hover_deadline: Option<Instant>,
    /// The last token Lua authed with; a change means the plugin restarted around us.
    pub token: Option<String>,
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

    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Nothing can be drawn or hit-tested until all three commands have landed at least once.
    fn dressed(&self) -> bool {
        self.paints
            && self.v2.config.is_some()
            && self.v2.theme.is_some()
            && self.v2.model.is_some()
    }

    /// Returns `Ok(false)` when the backend should exit.
    pub fn handle(&mut self, input: Input) -> io::Result<bool> {
        match input {
            Input::Mouse(m) => {
                if !self.paints {
                    self.emit(&Event::from(m))?;
                } else if self.dressed() {
                    // a painting backend speaks the gesture vocabulary; `mouse` is never sent
                    self.gesture(&m)?;
                }
            }
            Input::Focus(focused) => self.emit(&Event::Focus { focused })?,
            Input::Key { name, mods, raw } => {
                if self.dressed() {
                    self.key(&name, mods, &raw)?;
                } else {
                    self.emit(&Event::key(name, mods, &raw))?;
                }
            }
            Input::Paste(data) => self.emit(&Event::paste(data))?,
            Input::Command(cmd) => return self.run(cmd),
        }
        Ok(true)
    }

    fn gesture(&mut self, m: &vtabs_protocol::Mouse) -> io::Result<()> {
        if self.menu_gesture(m)? {
            return Ok(());
        }
        let now = self.now_ms();
        let (ui, events, repaint) = {
            let e = self.scene().0;
            let (cfg, _, model) = self.state();
            let plan = layout::plan(&e.view);
            let ordered = ordered_ids(model);
            let k = knobs(cfg, model, &e.view, &ordered);
            let r = resolve::mouse(&plan, &k, &self.ui, m, now);
            (r.ui, r.events, r.repaint)
        };
        self.ui = ui;
        self.arm_hover();
        for event in &events {
            self.emit(event)?;
        }
        if repaint {
            self.repaint()?;
        }
        Ok(())
    }

    /// The open menu answers the pointer itself; `false` hands the gesture back to the list, which
    /// is v1's `on_popover_down` returning true after it dismissed the menu.
    fn menu_gesture(&mut self, m: &vtabs_protocol::Mouse) -> io::Result<bool> {
        let now = self.now_ms();
        let resolved = match (self.menu_outcome(), self.v2.menu.as_ref()) {
            (Outcome::Open(placed), Some(msg)) => {
                let view = MenuView {
                    level: placed.level,
                    items: &msg.items,
                    hits: &placed.hits,
                    follow_pointer: self.menu_cfg().follow_pointer,
                };
                resolve::menu_mouse(&view, &self.menu_ui, &self.ui, m, now)
            }
            _ => return Ok(false),
        };
        self.ui = resolved.ui;
        if let Some(state) = resolved.menu {
            self.menu_ui = state;
        }
        self.arm_hover();
        for event in &resolved.events {
            self.emit(event)?;
        }
        if resolved.fall_through {
            return Ok(false);
        }
        if resolved.repaint {
            self.repaint()?;
        }
        Ok(true)
    }

    fn key(&mut self, name: &str, mods: vtabs_protocol::Mods, raw: &[u8]) -> io::Result<()> {
        if self.menu_key(name, mods)? {
            return Ok(());
        }
        let events = {
            let (cfg, theme, model) = self.state();
            let e = enrich(cfg, theme, model, self.dims(), &self.ui);
            let ordered = ordered_ids(model);
            let k = knobs(cfg, model, &e.view, &ordered);
            resolve::key(&k, name, mods, raw).events
        };
        for event in &events {
            self.emit(event)?;
        }
        Ok(())
    }

    /// While the menu is open it consumes every key: navigation, first-letter jump, edit buffer.
    fn menu_key(&mut self, name: &str, mods: vtabs_protocol::Mods) -> io::Result<bool> {
        let resolved = match (self.menu_outcome(), self.v2.menu.as_ref()) {
            (Outcome::Open(placed), Some(msg)) => {
                let view = MenuView {
                    level: placed.level,
                    items: &msg.items,
                    hits: &placed.hits,
                    follow_pointer: self.menu_cfg().follow_pointer,
                };
                resolve::menu_key(&view, &self.menu_ui, &self.ui, name, mods)
            }
            _ => return Ok(false),
        };
        if let Some(state) = resolved.menu {
            self.menu_ui = state;
        }
        for event in &resolved.events {
            self.emit(event)?;
        }
        if resolved.repaint {
            self.repaint()?;
        }
        Ok(true)
    }

    fn menu_cfg(&self) -> MenuCfg {
        let Some(cfg) = self.v2.config.as_ref() else {
            return MenuCfg::default();
        };
        let popover = cfg.popover.clone().unwrap_or_default();
        let padding = cfg.render.as_ref().map(|r| r.padding).unwrap_or_default();
        MenuCfg {
            padding_left: padding.left,
            padding_right: padding.right,
            want_width: popover.fixed_width(),
            ellipsis: cfg.ellipsis.clone().unwrap_or_else(|| "…".into()),
            follow_pointer: popover.follow_pointer,
        }
    }

    /// What the stored menu message asks for, against this pane's size.
    fn menu_outcome(&self) -> Outcome {
        let (Some(msg), Some(theme), Some(model)) = (
            self.v2.menu.as_ref(),
            self.v2.theme.as_ref(),
            self.v2.model.as_ref(),
        ) else {
            return Outcome::Closed;
        };
        let theme = theme_of(theme, model.private);
        menu::plan(msg, &self.menu_ui, &self.menu_cfg(), &theme, self.dims())
    }

    /// The frame's input, with the menu overlaid: an open menu wins over the P4b bridge, and a
    /// menu that is closed or unplaceable takes the bridge's rect with it.
    fn scene(&self) -> (Enriched, Outcome) {
        let (cfg, theme, model) = self.state();
        let mut e = enrich(cfg, theme, model, self.dims(), &self.ui);
        let outcome = match self.v2.menu.as_ref() {
            Some(msg) => menu::plan(
                msg,
                &self.menu_ui,
                &self.menu_cfg(),
                &e.view.theme,
                self.dims(),
            ),
            None => Outcome::Closed,
        };
        match &outcome {
            Outcome::Open(placed) => {
                e.view.popover = Some(placed.rect.clone());
                e.popover = Some(placed.hits.clone());
            }
            Outcome::Closed | Outcome::Refused { .. } => {
                e.view.popover = None;
                e.popover = None;
            }
        }
        (e, outcome)
    }

    fn state(&self) -> (&v2::ConfigMsg, &v2::ThemeMsg, &v2::ModelMsg) {
        (
            self.v2.config.as_ref().expect("dressed"),
            self.v2.theme.as_ref().expect("dressed"),
            self.v2.model.as_ref().expect("dressed"),
        )
    }

    fn dims(&self) -> (i64, i64) {
        (i64::from(self.size.0), i64::from(self.size.1))
    }

    /// Repaints from the stored state; the pane's pixels are this process's to own now.
    pub fn repaint(&mut self) -> io::Result<()> {
        if !self.dressed() {
            return Ok(());
        }
        let (bytes, popover, outcome, selected) = {
            let (e, outcome) = self.scene();
            let selected = match &outcome {
                Outcome::Open(placed) => Some(placed.selected),
                _ => None,
            };
            let frame = frame_of(&e.view);
            (
                frame_bytes(&frame, e.view.theme.bg),
                e.popover,
                outcome,
                selected,
            )
        };
        self.popover = popover;
        if let Some(selected) = selected {
            self.menu_ui.selected = selected;
        }
        self.refuse(&outcome)?;
        self.write(bytes.as_bytes())
    }

    /// An open level that cannot be placed is Lua's to unwind; the note is sent once per message.
    fn refuse(&mut self, outcome: &Outcome) -> io::Result<()> {
        let Outcome::Refused { why, level } = outcome else {
            return Ok(());
        };
        let rev = self.v2.menu.as_ref().map(|m| m.rev);
        if self.noted_menu == rev {
            return Ok(());
        }
        self.noted_menu = rev;
        let id = self.v2.menu.as_ref().and_then(|m| m.target);
        self.emit(&Event::Note {
            k: "menu_refused",
            why: Some(why),
            id,
            a: Some(level.name()),
        })
    }

    /// Hover goes stale on its own clock, so a pointer that left the pane stops lighting a row.
    fn arm_hover(&mut self) {
        let ms = self.v2.config.as_ref().map_or(0, |c| c.hover_timeout_ms);
        self.hover_deadline = (ms > 0 && self.ui.hover.is_some())
            .then(|| Instant::now() + std::time::Duration::from_millis(ms + 1));
    }

    pub fn next_hover(&self) -> Option<Instant> {
        self.hover_deadline
    }

    pub fn tick_hover(&mut self, now: Instant) -> io::Result<()> {
        if self.hover_deadline.is_some_and(|at| now >= at) {
            self.hover_deadline = None;
            let ms = self.v2.config.as_ref().map_or(0, |c| c.hover_timeout_ms);
            if self.ui.expire_hover(self.now_ms(), ms) {
                self.repaint()?;
            }
        }
        Ok(())
    }

    fn run(&mut self, cmd: Command) -> io::Result<bool> {
        let paints_this = matches!(
            cmd,
            Command::Config(_) | Command::Theme(_) | Command::Model(_) | Command::Menu(_)
        );
        let alive = self.apply(cmd)?;
        if alive && paints_this && self.paints {
            self.settle_scroll();
            self.repaint()?;
        }
        Ok(alive)
    }

    /// The optimistic wheel override retires once the model comes back carrying it.
    fn settle_scroll(&mut self) {
        let landed = self
            .v2
            .model
            .as_ref()
            .and_then(|m| m.scroll)
            .is_some_and(|s| s.user && Some(s.top) == self.ui.scroll);
        if landed {
            self.ui.scroll = None;
        }
    }

    fn apply(&mut self, cmd: Command) -> io::Result<bool> {
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
            Command::Auth { token } => {
                self.write(set_user_var(TOKEN_VAR, &token).as_bytes())?;
                // A new token is a new Lua process: the mux kept this backend but the plugin lost
                // `store.proto`/`store.paints` with its old state, and only `ready` restores them.
                // Re-announcing on the same token would ping-pong, since Lua re-auths on ready.
                if self.token.as_deref() != Some(token.as_str()) {
                    self.token = Some(token);
                    self.emit(&Event::ready(self.size.0, self.size.1, self.paints))?;
                }
            }
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
                    self.menu_ui.adopt(&msg);
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
            if self.paints {
                self.repaint()?;
            }
        }
        Ok(())
    }
}

/// `model.ordered`: pinned first, then the rest, both in the order Lua sent them.
fn ordered_ids(model: &v2::ModelMsg) -> Vec<i64> {
    let mut ids: Vec<i64> = model
        .tabs
        .iter()
        .filter(|t| t.pinned)
        .map(|t| t.id)
        .collect();
    ids.extend(model.tabs.iter().filter(|t| !t.pinned).map(|t| t.id));
    ids
}

fn knobs<'a>(
    cfg: &'a v2::ConfigMsg,
    model: &'a v2::ModelMsg,
    view: &'a vtabs_view::scene::RenderInput,
    ordered: &'a [i64],
) -> Knobs<'a> {
    let focus = model.focus.unwrap_or_default();
    Knobs {
        cols: view.cols,
        position: &view.cfg.position,
        double_click_ms: cfg.double_click_ms,
        tear_off: cfg.tear_off,
        wheel: cfg.wheel.as_deref().unwrap_or("scroll"),
        context: cfg.context.as_deref().unwrap_or("popover"),
        hover_mode: &view.cfg.hover,
        slot_rows: layout::slot_rows(&view.cfg),
        focus_on: focus.on,
        focus_index: focus.index,
        ordered,
        drag: model.drag.map(|d| MirroredDrag {
            id: d.id,
            active: d.active,
            origin_x: d.origin.x,
            origin_y: d.origin.y,
            outside: d.outside,
        }),
        scroll_top: model.scroll.unwrap_or_default().top,
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
            // the v1 path is what these tests pin; the painting path has its own below
            paints: false,
            ui: Default::default(),
            started: Instant::now(),
            popover: None,
            menu_ui: Default::default(),
            noted_menu: None,
            hover_deadline: None,
            token: None,
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
        let out = String::from_utf8_lossy(&a.out).to_string();
        assert!(out.starts_with(&set_user_var("vtabs_token", "abc")));
        // and it lands before the ready that follows it, so `is_ready` passes when that arrives
        assert!(saw(&a, r#""t":"ready""#));
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

    const CONFIG: &str = r#"{"rev":1,"desired_width":28,"position":"left","icons":true,
        "meta":"auto","meta_sep":" ","double_click_ms":300,"tear_off":true,"wheel":"scroll",
        "context":"popover","hover_timeout_ms":1500,
        "render":{"meta":true,"padding":{"left":1,"right":1,"top":0,"bottom":0},
        "tab_height":"card","separator":"gap","pinned_style":"compact","close_button":"hover",
        "scroll_indicator":"auto","new_tab_button":true,"new_tab_label":"New tab","hover":"follow"}}"#;
    const THEME: &str = r#"{"rev":1,"scheme":{"ansi":[],"brights":[]},"overrides":{}}"#;
    const MODEL: &str = r#"{"rev":1,"screen":"sidebar","active":1,
        "strip":{"buttons":[{"id":"toggle"},{"id":"new_tab"}]},
        "tabs":[{"id":1,"index":1,"title":"one"},{"id":2,"index":2,"title":"two"}]}"#;

    fn painting() -> App<Vec<u8>> {
        let mut a = app();
        a.paints = true;
        a
    }

    fn dress(a: &mut App<Vec<u8>>) {
        a.handle(Input::Command(Command::Config(Box::new(
            serde_json::from_str(CONFIG).unwrap(),
        ))))
        .unwrap();
        a.handle(Input::Command(Command::Theme(Box::new(
            serde_json::from_str(THEME).unwrap(),
        ))))
        .unwrap();
        a.handle(Input::Command(Command::Model(Box::new(
            serde_json::from_str(MODEL).unwrap(),
        ))))
        .unwrap();
    }

    fn click(kind: vtabs_protocol::MouseKind, x: u16, y: u16) -> Input {
        Input::Mouse(vtabs_protocol::Mouse {
            kind,
            button: vtabs_protocol::Button::Left,
            x,
            y,
            dy: 0,
            mods: Mods::default(),
        })
    }

    #[test]
    fn a_new_token_re_announces_ready_but_the_same_one_does_not() {
        let mut a = painting();
        let auth = |a: &mut App<Vec<u8>>, token: &str| {
            a.handle(Input::Command(Command::Auth {
                token: token.into(),
            }))
            .unwrap();
        };
        auth(&mut a, "first");
        assert!(saw(&a, r#""t":"ready""#), "the first auth announces");
        let after = payloads(&a).len();

        // Lua re-auths from its own ready branch; announcing again would ping-pong forever
        auth(&mut a, "first");
        assert_eq!(payloads(&a).len(), after, "the same token says nothing new");

        // a gui restart mints a fresh token: store.proto/store.paints must be restored
        auth(&mut a, "second");
        let sent = &payloads(&a)[after..];
        let ready = sent
            .iter()
            .find(|p| p.contains(r#""t":"ready""#))
            .expect("a new token re-announces");
        assert!(ready.contains(r#""paints":true"#));
        assert!(ready.contains(r#""v":2"#));
    }

    #[test]
    fn a_painting_backend_draws_the_pane_once_the_state_is_complete() {
        let mut a = painting();
        a.handle(Input::Command(Command::Config(Box::new(
            serde_json::from_str(CONFIG).unwrap(),
        ))))
        .unwrap();
        assert!(
            !String::from_utf8_lossy(&a.out).contains("\x1b[?25l"),
            "config alone paints nothing"
        );
        dress(&mut a);
        let painted = String::from_utf8_lossy(&a.out).to_string();
        assert!(painted.contains("\x1b[?25l"), "the frame hides the cursor");
        assert!(
            painted.contains("one") && painted.contains("two"),
            "both tabs listed"
        );
    }

    #[test]
    fn a_painting_backend_speaks_do_and_never_mouse() {
        let mut a = painting();
        dress(&mut a);
        let before = payloads(&a).len();
        a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
            .unwrap();
        let sent = &payloads(&a)[before..];
        assert!(
            !sent.iter().any(|p| p.contains(r#""t":"mouse""#)),
            "v1 mouse is gone: {sent:?}"
        );
        assert!(
            sent.iter().any(|p| p.contains(r#""t":"do""#)),
            "a gesture was reported: {sent:?}"
        );
    }

    #[test]
    fn input_before_the_first_full_state_is_swallowed() {
        let mut a = painting();
        a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
            .unwrap();
        assert!(a.out.is_empty(), "nothing is drawn and nothing is reported");
    }

    #[test]
    fn a_v1_pane_still_gets_the_old_mouse_events() {
        let mut a = app();
        a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
            .unwrap();
        assert!(saw(&a, r#"{"t":"mouse","k":"down""#));
    }

    #[test]
    fn hover_expiry_clears_the_highlight_without_a_round_trip() {
        let mut a = painting();
        dress(&mut a);
        a.handle(Input::Mouse(vtabs_protocol::Mouse {
            kind: vtabs_protocol::MouseKind::Move,
            button: vtabs_protocol::Button::None,
            x: 6,
            y: 3,
            dy: 0,
            mods: Mods::default(),
        }))
        .unwrap();
        assert!(a.ui.hover.is_some());
        assert!(a.next_hover().is_some(), "the clock is armed");
        a.hover_deadline = Some(Instant::now());
        a.started = Instant::now() - std::time::Duration::from_secs(60);
        a.tick_hover(Instant::now()).unwrap();
        assert!(a.ui.hover.is_none(), "stale hover is dropped");
        assert!(a.next_hover().is_none());
    }

    const MENU: &str = r#"{"rev":1,"open":true,"level":"root","anchor":{"row":3,"col":2},
        "target":1,"selected":1,"header":{"title":"one"},
        "items":[{"id":"activate","label":"Switch to tab"},
                 {"id":"close","label":"Close tab","danger":true}]}"#;
    /// The P4b bridge rect, with text no widget of ours would ever draw.
    fn send(a: &mut App<Vec<u8>>, cmd: Command) {
        a.handle(Input::Command(cmd)).unwrap();
    }

    fn menu(json: &str) -> Command {
        Command::Menu(Box::new(serde_json::from_str(json).unwrap()))
    }

    fn painted(a: &App<Vec<u8>>) -> String {
        String::from_utf8_lossy(&a.out).to_string()
    }

    #[test]
    fn an_open_menu_paints_and_a_closed_one_draws_nothing() {
        let mut a = painting();
        dress(&mut a);
        a.out.clear();
        send(&mut a, menu(MENU));
        assert!(painted(&a).contains("Switch to tab"), "the widget drew it");

        a.out.clear();
        send(&mut a, menu(r#"{"rev":3,"open":false}"#));
        assert!(
            !painted(&a).contains("Switch to tab"),
            "a closed menu leaves the list alone"
        );
    }

    #[test]
    fn a_menu_that_cannot_be_placed_is_refused_once_and_draws_nothing() {
        let mut a = painting();
        a.size = (28, 2);
        dress(&mut a);
        let before = payloads(&a).len();
        send(&mut a, menu(MENU));
        let sent = &payloads(&a)[before..];
        let note = sent
            .iter()
            .find(|p| p.contains(r#""t":"note""#))
            .expect("a refusal");
        assert!(note.contains(r#""k":"menu_refused""#));
        assert!(note.contains(r#""why":"rows""#));
        assert!(note.contains(r#""id":1"#) && note.contains(r#""a":"root""#));
        assert!(!painted(&a).contains("Switch to tab"), "nothing is drawn");

        let after = payloads(&a).len();
        a.repaint().unwrap();
        assert_eq!(payloads(&a).len(), after, "and the note is not repeated");
    }

    #[test]
    fn while_the_menu_is_open_the_pane_answers_to_it_and_not_to_the_list() {
        let mut a = painting();
        dress(&mut a);
        send(&mut a, menu(MENU));
        let hits = a.popover.clone().expect("the menu placed itself");
        let row = hits
            .rows
            .iter()
            .position(|(id, _)| id.as_deref() == Some("activate"))
            .expect("the item was drawn");
        let (x, y) = ((hits.x + 1) as u16, (hits.y + row as i64) as u16);

        let before = payloads(&a).len();
        a.handle(click(vtabs_protocol::MouseKind::Press, x, y))
            .unwrap();
        let sent = &payloads(&a)[before..];
        assert!(
            sent.iter().any(|p| p.contains(r#""a":"menu_pick""#)),
            "the item ran: {sent:?}"
        );
        assert!(
            !sent.iter().any(|p| p.contains(r#""a":"press_card""#)),
            "and the list under it never saw the click"
        );

        // keys belong to the menu too: escape closes it instead of blurring the sidebar
        let before = payloads(&a).len();
        a.handle(Input::Key {
            name: "escape".into(),
            mods: Mods::default(),
            raw: b"\x1b".to_vec(),
        })
        .unwrap();
        let sent = &payloads(&a)[before..];
        assert!(sent.iter().any(|p| p.contains(r#""a":"menu_closed""#)));
        assert!(!sent.iter().any(|p| p.contains(r#""a":"blur_sidebar""#)));
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

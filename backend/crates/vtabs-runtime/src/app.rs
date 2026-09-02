use std::io::{self, Write};
use std::time::Instant;

use vtabs_core::ui::{SettingsUi, UiState};
use vtabs_input::Input;
use vtabs_input::resolve::{self, Knobs, MenuView, MirroredDrag, SettingsScreen};
use vtabs_protocol::limits::{
    FX_MAX_FPS, FX_MAX_MS, FX_MIN_FPS, MENU_MAX_ITEMS, MODEL_MAX_FIELDS, MODEL_MAX_SPACES,
    MODEL_MAX_TABS,
};
use vtabs_protocol::{Command, Event, v2};
use vtabs_view::enrich::{Enriched, PopoverHits, enrich, glyph_map, theme_of};
use vtabs_view::frame::Cell;
use vtabs_view::fx;
use vtabs_view::layout;
use vtabs_view::menu::{self, MenuCfg, MenuState, Outcome};
use vtabs_view::render::frame_of;
use vtabs_view::settings::{self, SettingsView};

use crate::cli::Cli;
use crate::log::Logger;
use crate::paint::{changed_rows_bytes, rows_bytes};
use crate::uservar::{TOKEN_VAR, set_user_var};

const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";
/// Any-motion mouse tracking; the terminal guard turns it on, `config.hover_highlight` may turn it off.
const MOTION_ON: &str = "\x1b[?1003h";
const MOTION_OFF: &str = "\x1b[?1003l";

/// Sole stdout writer, so user-var OSCs never interleave with frame bytes.
pub struct App<W: Write> {
    pub out: W,
    pub log: Logger,
    pub var: String,
    pub size: (u16, u16),
    /// Asked before every frame, so no paint trusts a size the pane no longer has.
    pub probe: fn() -> Option<(u16, u16)>,
    /// A size change wipes the pane before the next frame.
    pub needs_clear: bool,
    pub fx: Option<FxRun>,
    /// The last full frame laid out: the final frame any fade lands on.
    pub last_rows: Option<PaintedRows>,
    /// True while the pane shows exactly `last_rows`, so the next repaint may write only the rows
    /// that differ; a fade tick, a wipe or a resize leaves the screen elsewhere and clears it.
    pub shown_is_final: bool,
    pub seq: u64,
    pub v2: V2State,
    pub ui: UiState,
    pub started: Instant,
    /// Where the menu Lua composed sits, from the last frame; the bridge reports against it.
    pub popover: Option<PopoverHits>,
    /// The menu's own state: the selection Lua no longer drives and the rename buffer Rust owns.
    pub menu_ui: MenuState,
    pub settings_ui: SettingsUi,
    /// The menu rev a `menu_refused` was already sent for, so a resize does not repeat it.
    pub noted_menu: Option<u64>,
    pub hover_deadline: Option<Instant>,
    /// The last token Lua authed with; a change means the plugin restarted around us.
    pub token: Option<String>,
    /// The server's own `wezterm cli`, where this pane has one to act for.
    pub cli: Option<Cli>,
}

/// The rows a repaint wrote, kept so a fade has a final frame to land on.
pub struct PaintedRows {
    pub rows: Vec<Option<Vec<Cell>>>,
    pub fades: Vec<Option<f64>>,
    pub page_bg: [u8; 3],
    /// The open menu's rows (1-based y, height), the only rows `popover_in` touches.
    pub menu_rows: Option<(i64, i64)>,
}

/// One fade in flight; frames are generated here, nothing crosses the wire per tick.
pub struct FxRun {
    rows: Vec<Option<Vec<Cell>>>,
    fades: Vec<Option<f64>>,
    page_bg: [u8; 3],
    animated: Vec<usize>,
    delays: Vec<u64>,
    phase: fx::Phase,
    frame_ms: u64,
    started: Instant,
    pub next_at: Instant,
}

impl FxRun {
    fn elapsed_ms(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.started).as_millis() as u64
    }

    /// The frame for `now`; a late wake renders now, skipped ticks are never replayed.
    fn tick(&mut self, now: Instant) -> String {
        let elapsed = self
            .elapsed_ms(now)
            .min(fx::total_ms(&self.phase, &self.delays));
        let rows = fx::frame_at(
            &self.rows,
            &self.animated,
            &self.delays,
            &self.phase,
            self.page_bg,
            elapsed,
        );
        self.next_at = now + std::time::Duration::from_millis(self.frame_ms);
        rows_bytes(&rows, &self.fades, self.page_bg)
    }

    fn finished(&self, now: Instant) -> bool {
        self.elapsed_ms(now) >= fx::total_ms(&self.phase, &self.delays)
    }
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
        // events are silently dropped.
        self.seq += 1;
        json.truncate(json.len() - 1);
        json.push_str(&format!(",\"n\":{}}}", self.seq));
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

    /// On until a config says otherwise, which is also the terminal's state at startup.
    fn hover_highlight(&self) -> bool {
        self.v2.config.as_ref().is_none_or(|c| c.hover_highlight)
    }

    /// Nothing can be drawn or hit-tested until all three commands have landed at least once.
    fn dressed(&self) -> bool {
        self.v2.config.is_some() && self.v2.theme.is_some() && self.v2.model.is_some()
    }

    /// Returns `Ok(false)` when the backend should exit.
    pub fn handle(&mut self, input: Input) -> io::Result<bool> {
        match input {
            // the backend speaks the gesture vocabulary; a pane with no state yet swallows the click
            Input::Mouse(m) => {
                if self.dressed() {
                    self.gesture(&m)?;
                }
            }
            // Focus is the pane's own business: the pointer left with it, so the lit row goes out
            // here, at once, instead of a round trip to Lua and a wait for the hover timeout.
            Input::Focus(false) => {
                if self.ui.hover.take().is_some() {
                    self.arm_hover();
                    self.repaint()?;
                }
            }
            Input::Focus(true) => {}
            Input::Dropped { what, reason } => self.emit(&Event::Dropped { what, reason })?,
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
        if self.settings_gesture(m)? || self.menu_gesture(m)? {
            return Ok(());
        }
        let now = self.now_ms();
        let (ui, events, repaint) = {
            let e = self.scene().0;
            let (cfg, _, model) = self.state();
            let plan = layout::plan(&e.view);
            let ordered = ordered_ids(model);
            let space_ids = space_ids(model);
            let k = knobs(cfg, model, &e.view, &ordered, &space_ids);
            let r = resolve::mouse(&plan, &k, &self.ui, m, now);
            (r.ui, r.events, r.repaint)
        };
        self.ui = ui;
        if !self.hover_highlight() {
            self.ui.hover = None;
        }
        self.arm_hover();
        for event in &events {
            self.emit(event)?;
        }
        if repaint {
            self.repaint()?;
        }
        Ok(())
    }

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

    /// The settings screen owns its pane whole: no sidebar plan, no menu, no key forwarded.
    fn settings_gesture(&mut self, m: &vtabs_protocol::Mouse) -> io::Result<bool> {
        let resolved = {
            let Some(view) = self.settings_view() else {
                return Ok(false);
            };
            let plan = settings::plan(&view);
            let screen = SettingsScreen {
                plan: &plan,
                editing: view.editing(),
                armed: view.armed(),
            };
            resolve::settings_mouse(&screen, &self.settings_ui, m)
        };
        self.apply_settings(resolved)?;
        Ok(true)
    }

    fn settings_key(&mut self, name: &str, mods: vtabs_protocol::Mods) -> io::Result<bool> {
        let resolved = {
            let Some(view) = self.settings_view() else {
                return Ok(false);
            };
            let plan = settings::plan(&view);
            let screen = SettingsScreen {
                plan: &plan,
                editing: view.editing(),
                armed: view.armed(),
            };
            resolve::settings_key(&screen, &self.settings_ui, name, mods)
        };
        self.apply_settings(resolved)?;
        Ok(true)
    }

    fn apply_settings(&mut self, resolved: resolve::Resolution) -> io::Result<()> {
        if let Some(state) = resolved.settings {
            self.settings_ui = state;
        }
        for event in &resolved.events {
            self.emit(event)?;
        }
        if resolved.repaint {
            self.repaint()?;
        }
        Ok(())
    }

    /// The settings widget's input, from the same three messages the sidebar reads.
    fn settings_view(&self) -> Option<SettingsView<'_>> {
        let (cfg, theme, model) = (
            self.v2.config.as_ref()?,
            self.v2.theme.as_ref()?,
            self.v2.model.as_ref()?,
        );
        if model.screen.as_deref() != Some("settings") {
            return None;
        }
        Some(SettingsView {
            cols: self.dims().0,
            rows: self.dims().1,
            model,
            ui: &self.settings_ui,
            theme: theme_of(theme, model.private),
            glyphs: glyph_map(cfg),
        })
    }

    fn key(&mut self, name: &str, mods: vtabs_protocol::Mods, raw: &[u8]) -> io::Result<()> {
        if self.settings_key(name, mods)? || self.menu_key(name, mods)? {
            return Ok(());
        }
        let events = {
            let (cfg, theme, model) = self.state();
            let e = enrich(cfg, theme, model, self.dims(), &self.ui);
            let ordered = ordered_ids(model);
            let space_ids = space_ids(model);
            let k = knobs(cfg, model, &e.view, &ordered, &space_ids);
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
    /// A state change ends any fade: the frame it lands on is this one.
    pub fn repaint(&mut self) -> io::Result<()> {
        if !self.dressed() {
            return Ok(());
        }
        self.sync_size()?;
        self.fx = None;
        let cleared = std::mem::take(&mut self.needs_clear);
        if cleared {
            self.out.write_all(CLEAR_SCREEN.as_bytes())?;
        }
        // The frame on screen, when there is one to diff against; a wipe leaves nothing there.
        let shown = if cleared || !self.shown_is_final {
            None
        } else {
            self.last_rows.take()
        };
        if let Some(painted) = self.settings_view().map(|view| {
            let (cells, fades) = settings::cells(&view);
            PaintedRows {
                rows: cells,
                fades,
                page_bg: view.theme.bg,
                menu_rows: None,
            }
        }) {
            return self.paint(painted, shown.as_ref());
        }
        let (painted, popover, outcome, selected) = {
            let (e, outcome) = self.scene();
            let selected = match &outcome {
                Outcome::Open(placed) => Some(placed.selected),
                _ => None,
            };
            let menu_rows = match &outcome {
                Outcome::Open(placed) => Some((placed.rect.y, placed.rect.h)),
                _ => None,
            };
            let frame = frame_of(&e.view);
            let painted = PaintedRows {
                rows: frame.cells,
                fades: frame.fades,
                page_bg: e.view.theme.bg,
                menu_rows,
            };
            (painted, e.popover, outcome, selected)
        };
        self.popover = popover;
        if let Some(selected) = selected {
            self.menu_ui.selected = selected;
        }
        self.refuse(&outcome)?;
        self.paint(painted, shown.as_ref())
    }

    /// Writes `painted` over `shown`: only the rows that differ when the two share a page colour,
    /// every row otherwise. A frame identical to the one on screen writes nothing at all.
    fn paint(&mut self, painted: PaintedRows, shown: Option<&PaintedRows>) -> io::Result<()> {
        let bytes = match shown {
            Some(prev) if prev.page_bg == painted.page_bg => changed_rows_bytes(
                &painted.rows,
                &painted.fades,
                painted.page_bg,
                &prev.rows,
                &prev.fades,
            ),
            _ => Some(rows_bytes(&painted.rows, &painted.fades, painted.page_bg)),
        };
        self.last_rows = Some(painted);
        self.shown_is_final = true;
        if let Some(bytes) = bytes {
            self.write(bytes.as_bytes())?;
            self.log_paint();
        }
        Ok(())
    }

    fn log_paint(&mut self) {
        let rev = self.v2.model.as_ref().map_or(0, |m| m.rev);
        let (cols, rows) = self.size;
        self.log.log(format!("paint {cols}x{rows} rev {rev}"));
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
        if alive && paints_this {
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
            Command::Clear => {
                self.needs_clear = true;
                self.repaint()?
            }
            Command::Ping { n } => self.emit(&Event::Pong { echo: n })?,
            Command::Auth { token } => {
                self.write(set_user_var(TOKEN_VAR, &token).as_bytes())?;
                // A new token is a new Lua process: the mux kept this backend but the plugin lost
                // `store.proto`/`store.paints` with its old state, and only `ready` restores them.
                // Re-announcing on the same token would ping-pong, since Lua re-auths on ready.
                if self.token.as_deref() != Some(token.as_str()) {
                    self.token = Some(token);
                    self.emit(&Event::ready(self.size.0, self.size.1))?;
                }
            }
            Command::Config(msg) => {
                // Motion reports are the one input the pane can decline: every pointer move over
                // it is otherwise written through the mux to this pty.
                if msg.hover_highlight != self.hover_highlight() {
                    self.write(
                        if msg.hover_highlight {
                            MOTION_ON
                        } else {
                            MOTION_OFF
                        }
                        .as_bytes(),
                    )?;
                }
                self.v2.config = Some(*msg)
            }
            Command::Theme(msg) => self.v2.theme = Some(*msg),
            Command::Model(msg) => {
                if msg.tabs.len() > MODEL_MAX_TABS
                    || msg.fields.len() > MODEL_MAX_FIELDS
                    || msg.spaces.len() > MODEL_MAX_SPACES
                {
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
            Command::Fx(msg) => self.start_fx(&msg)?,
            Command::Notice(msg) => self.log.log(format!("notice {}", msg.text)),
            Command::Kill { title } => {
                let done = self
                    .cli
                    .as_ref()
                    .ok_or_else(|| "no cli here".to_string())
                    .and_then(|cli| cli.kill_by_title(&title));
                self.report("kill", done.map(|()| String::new()))?;
            }
            Command::Rescue { band, position } => {
                let right = position.as_deref() == Some("right");
                let done = self
                    .cli
                    .as_ref()
                    .ok_or_else(|| "no cli here".to_string())
                    .and_then(|cli| cli.rescue(i64::from(band), right));
                self.report("rescue", done.map(|n| n.to_string()))?;
            }
            Command::Quit => return Ok(false),
        }
        Ok(true)
    }

    /// One `cli` event per server-side verb: the count or an empty detail on success, the error otherwise.
    fn report(&mut self, op: &'static str, done: Result<String, String>) -> io::Result<()> {
        self.log.log(format!("cli {op}: {done:?}"));
        let (ok, detail) = match done {
            Ok(detail) => (true, detail),
            Err(detail) => (false, detail),
        };
        self.emit(&Event::Cli { op, ok, detail })
    }

    pub fn next_fx(&self) -> Option<Instant> {
        self.fx.as_ref().map(|run| run.next_at)
    }

    /// A fade over the frame the pane shows: the phase's rows rise out of the page colour.
    fn start_fx(&mut self, msg: &v2::FxMsg) -> io::Result<()> {
        let Some(mut phase) = fx::phase_named(&msg.phase) else {
            self.log.log(format!("fx {}: unknown phase", msg.phase));
            return Ok(());
        };
        let Some(painted) = self.last_rows.as_ref() else {
            return Ok(());
        };
        if let Some(ms) = msg.ms {
            phase.ms = ms.clamp(1, FX_MAX_MS);
        }
        let fps = msg.fps.unwrap_or(30).clamp(FX_MIN_FPS, FX_MAX_FPS);
        let animated: Vec<usize> = painted
            .rows
            .iter()
            .enumerate()
            .filter(|(i, row)| {
                row.is_some()
                    && match (msg.phase.as_str(), painted.menu_rows) {
                        ("popover_in", Some((y, h))) => {
                            let row = *i as i64 + 1;
                            row >= y && row < y + h
                        }
                        ("popover_in", None) => false,
                        _ => true,
                    }
            })
            .map(|(i, _)| i)
            .collect();
        if animated.is_empty() {
            return Ok(());
        }
        let delays = fx::delays(&phase, &animated);
        let now = Instant::now();
        let mut run = FxRun {
            rows: painted.rows.clone(),
            fades: painted.fades.clone(),
            page_bg: painted.page_bg,
            animated,
            delays,
            phase,
            frame_ms: 1000 / u64::from(fps),
            started: now,
            next_at: now,
        };
        let first = run.tick(now);
        self.write(first.as_bytes())?;
        self.shown_is_final = false;
        self.fx = Some(run);
        Ok(())
    }

    /// Writes the frame for `now`, skipping any tick the loop slept through; a pane that changed
    /// size under the fade gets the final frame instead.
    pub fn tick_fx(&mut self, now: Instant) -> io::Result<()> {
        if !self.fx.as_ref().is_some_and(|run| now >= run.next_at) {
            return Ok(());
        }
        if self.sync_size()? {
            return self.repaint();
        }
        let Some(run) = self.fx.as_mut() else {
            return Ok(());
        };
        let bytes = run.tick(now);
        let done = run.finished(now);
        self.write(bytes.as_bytes())?;
        self.shown_is_final = false;
        if done {
            self.fx = None;
        }
        Ok(())
    }

    pub fn poll_size(&mut self) -> io::Result<()> {
        if self.sync_size()? {
            self.repaint()?;
        }
        Ok(())
    }

    /// Asks the terminal; `true` when the size moved and the pane needs a fresh frame.
    fn sync_size(&mut self) -> io::Result<bool> {
        match (self.probe)() {
            Some(size) if size != self.size => self.adopt_size(size).map(|()| true),
            _ => Ok(false),
        }
    }

    /// Applies a size without asking the terminal.
    pub fn resize(&mut self, size: (u16, u16)) -> io::Result<()> {
        if size != self.size {
            self.adopt_size(size)?;
            self.repaint()?;
        }
        Ok(())
    }

    fn adopt_size(&mut self, size: (u16, u16)) -> io::Result<()> {
        let ((cols, rows), (w, h)) = (self.size, size);
        self.log.log(format!("resize {cols}x{rows} -> {w}x{h}"));
        self.size = size;
        self.needs_clear = true;
        self.shown_is_final = false;
        self.fx = None;
        self.emit(&Event::Resize { cols: w, rows: h })
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

fn space_ids(model: &v2::ModelMsg) -> Vec<&str> {
    model.spaces.iter().map(|s| s.id.as_str()).collect()
}

fn knobs<'a>(
    cfg: &'a v2::ConfigMsg,
    model: &'a v2::ModelMsg,
    view: &'a vtabs_view::scene::RenderInput,
    ordered: &'a [i64],
    space_ids: &'a [&'a str],
) -> Knobs<'a> {
    let focus = model.focus.unwrap_or_default();
    let active_space = model
        .space
        .as_deref()
        .and_then(|active| space_ids.iter().position(|id| *id == active));
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
        space_ids,
        active_space,
    }
}

#[cfg(test)]
#[path = "../tests/unit/app.rs"]
mod tests;

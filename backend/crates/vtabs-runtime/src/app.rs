use std::io::{self, Write};
use std::time::Instant;

use vtabs_core::ui::{SettingsUi, UiState};
use vtabs_input::Input;
use vtabs_input::resolve::{self, Knobs, MenuView, MirroredDrag, SettingsScreen};
use vtabs_protocol::limits::{
    FX_MAX_FPS, FX_MAX_MS, FX_MIN_FPS, MENU_MAX_ITEMS, MODEL_MAX_FIELDS, MODEL_MAX_TABS,
};
use vtabs_protocol::{Command, Event, v2};
use vtabs_view::enrich::{Enriched, PopoverHits, enrich, glyph_map, theme_of};
use vtabs_view::frame::Cell;
use vtabs_view::fx;
use vtabs_view::layout;
use vtabs_view::menu::{self, MenuCfg, MenuState, Outcome};
use vtabs_view::render::frame_of;
use vtabs_view::settings::{self, SettingsView};

use crate::log::Logger;
use crate::paint::{frame_bytes, rows_bytes};
use crate::terminal;
use crate::uservar::{TOKEN_VAR, set_user_var};

const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";

/// Sole stdout writer, so user-var OSCs never interleave with frame bytes.
pub struct App<W: Write> {
    pub out: W,
    pub log: Logger,
    pub var: String,
    pub size: (u16, u16),
    pub fx: Option<FxRun>,
    /// What the pane shows right now: the final frame any fade lands on.
    pub last_rows: Option<PaintedRows>,
    pub seq: u64,
    pub v2: V2State,
    pub ui: UiState,
    pub started: Instant,
    /// Where the menu Lua composed sits, from the last frame; the bridge reports against it.
    pub popover: Option<PopoverHits>,
    /// The menu's own state: the selection Lua no longer drives and the rename buffer Rust owns.
    pub menu_ui: MenuState,
    /// The settings screen's nav, focus and filter; local by design, nothing persists (§4.3 #9).
    pub settings_ui: SettingsUi,
    /// The menu rev a `menu_refused` was already sent for, so a resize does not repeat it.
    pub noted_menu: Option<u64>,
    pub hover_deadline: Option<Instant>,
    /// The last token Lua authed with; a change means the plugin restarted around us.
    pub token: Option<String>,
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
        if self.settings_gesture(m)? || self.menu_gesture(m)? {
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

    /// The settings widget's input, from the same three messages the sidebar reads. `position` and
    /// `meta_sep` come off the config: `preview.render` carries neither.
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
            position: cfg
                .position
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "left".into()),
            meta_sep: cfg.meta_sep.clone(),
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
    /// A state change ends any fade: the frame it lands on is this one.
    pub fn repaint(&mut self) -> io::Result<()> {
        if !self.dressed() {
            return Ok(());
        }
        self.fx = None;
        if let Some((bytes, painted)) = self.settings_view().map(|view| {
            let (cells, fades) = settings::cells(&view);
            let bytes = rows_bytes(&cells, &fades, view.theme.bg);
            (
                bytes,
                PaintedRows {
                    rows: cells,
                    fades,
                    page_bg: view.theme.bg,
                    menu_rows: None,
                },
            )
        }) {
            self.last_rows = Some(painted);
            return self.write(bytes.as_bytes());
        }
        let (bytes, popover, outcome, selected, painted) = {
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
                rows: frame.cells.clone(),
                fades: frame.fades.clone(),
                page_bg: e.view.theme.bg,
                menu_rows,
            };
            (
                frame_bytes(&frame, e.view.theme.bg),
                e.popover,
                outcome,
                selected,
                painted,
            )
        };
        self.last_rows = Some(painted);
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
                self.write(CLEAR_SCREEN.as_bytes())?;
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
            Command::Config(msg) => self.v2.config = Some(*msg),
            Command::Theme(msg) => self.v2.theme = Some(*msg),
            Command::Model(msg) => {
                if msg.tabs.len() > MODEL_MAX_TABS || msg.fields.len() > MODEL_MAX_FIELDS {
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
            Command::Quit => return Ok(false),
        }
        Ok(true)
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
        self.fx = Some(run);
        Ok(())
    }

    /// Writes the frame for `now`, skipping any tick the loop slept through.
    pub fn tick_fx(&mut self, now: Instant) -> io::Result<()> {
        let Some(run) = self.fx.as_mut() else {
            return Ok(());
        };
        if now < run.next_at {
            return Ok(());
        }
        let bytes = run.tick(now);
        let done = run.finished(now);
        self.write(bytes.as_bytes())?;
        if done {
            self.fx = None;
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
            self.repaint()?;
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
            fx: None,
            last_rows: None,
            seq: 0,
            v2: V2State::default(),
            ui: Default::default(),
            started: Instant::now(),
            popover: None,
            settings_ui: Default::default(),
            menu_ui: Default::default(),
            noted_menu: None,
            hover_deadline: None,
            token: None,
        }
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
            set_user_var("vtabs", r#"{"t":"pong","echo":7,"n":1}"#).as_bytes()
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
    fn pong_echoes_the_ping_in_echo_and_carries_its_own_n() {
        let mut a = app();
        a.handle(Input::Key {
            name: "j".into(),
            mods: Mods::default(),
            raw: b"j".to_vec(),
        })
        .unwrap();
        a.handle(Input::Command(Command::Ping { n: Some(7) }))
            .unwrap();
        assert_eq!(payloads(&a)[1], r#"{"t":"pong","echo":7,"n":2}"#);
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
        let mut a = app();
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
        let mut a = app();
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
    fn clear_wipes_the_pane_and_draws_it_again() {
        let mut a = app();
        dress(&mut a);
        a.out.clear();
        a.handle(Input::Command(Command::Clear)).unwrap();
        let out = painted(&a);
        // a repaint alone leaves untouched rows as they were; only the wipe clears stale bytes
        assert!(out.starts_with(CLEAR_SCREEN), "the pane is wiped first");
        assert!(out.contains("one") && out.contains("two"), "then redrawn");
    }

    const SETTINGS_MODEL: &str = r#"{"rev":2,"screen":"settings","version":"9.9.9",
        "groups":[{"id":"layout","label":"Layout"},{"id":"cards","label":"Cards"}],
        "fields":[
          {"key":"width","label":"width","group":"layout","widget":"stepper","value_text":"< 28 >"},
          {"key":"position","label":"position","group":"layout","widget":"picker","value_text":"< left >"}]}"#;

    #[test]
    fn a_settings_model_paints_the_page_and_answers_for_its_own_keys() {
        let mut a = app();
        a.size = (100, 21);
        dress(&mut a);
        a.out.clear();
        a.handle(Input::Command(Command::Model(Box::new(
            serde_json::from_str(SETTINGS_MODEL).unwrap(),
        ))))
        .unwrap();
        let painted = String::from_utf8_lossy(&a.out).to_string();
        assert!(painted.contains("Settings"), "the header names the screen");
        assert!(
            painted.contains("Layout") && painted.contains("width"),
            "nav and form: {painted:?}"
        );
        assert!(!painted.contains("one"), "the sidebar's tabs are not here");

        let before = payloads(&a).len();
        a.handle(Input::Key {
            name: "r".into(),
            mods: Mods::default(),
            raw: b"r".to_vec(),
        })
        .unwrap();
        let sent = &payloads(&a)[before..];
        assert!(
            sent.iter()
                .any(|p| p.contains(r#""a":"reset_option""#) && p.contains(r#""key":"width""#)),
            "the verb names the focused key: {sent:?}"
        );
        assert!(
            !sent.iter().any(|p| p.contains(r#""t":"key""#)),
            "the page owns the keyboard: {sent:?}"
        );
    }

    #[test]
    fn a_painting_backend_speaks_do_and_never_mouse() {
        let mut a = app();
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
        let mut a = app();
        a.handle(click(vtabs_protocol::MouseKind::Press, 6, 3))
            .unwrap();
        assert!(a.out.is_empty(), "nothing is drawn and nothing is reported");
    }

    #[test]
    fn hover_expiry_clears_the_highlight_without_a_round_trip() {
        let mut a = app();
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
    fn a_fade_runs_on_the_frame_shown_and_a_repaint_lands_it_on_the_final_frame() {
        let mut a = app();
        dress(&mut a);
        a.out.clear();
        a.repaint().unwrap();
        let final_frame = painted(&a);
        a.out.clear();
        send(
            &mut a,
            Command::Fx(v2::FxMsg {
                phase: "expand_in".into(),
                ms: Some(200),
                fps: Some(30),
            }),
        );
        assert!(a.fx.is_some(), "the fade is running");
        assert_ne!(
            painted(&a),
            final_frame,
            "t=0 is the anchor colour, not the final frame"
        );
        a.out.clear();
        a.repaint().unwrap();
        assert!(a.fx.is_none(), "a state repaint ends the fade");
        assert_eq!(
            painted(&a),
            final_frame,
            "and lands exactly on the final frame"
        );
        send(
            &mut a,
            Command::Fx(v2::FxMsg {
                phase: "nope".into(),
                ms: None,
                fps: None,
            }),
        );
        assert!(a.fx.is_none(), "an unknown phase plays nothing");
    }

    #[test]
    fn an_open_menu_paints_and_a_closed_one_draws_nothing() {
        let mut a = app();
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
        let mut a = app();
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
        let mut a = app();
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

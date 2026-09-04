use std::collections::BTreeSet;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use vtabs_engine::color::Color;
use vtabs_engine::config::EngineConfig;
use vtabs_engine::enrich::{Enriched, PopoverHits, enrich, glyph_map};
use vtabs_engine::frame::Cell;
use vtabs_engine::fx;
use vtabs_engine::interaction::{self, Knobs, MenuView, MirroredDrag, SettingsScreen};
use vtabs_engine::layout;
use vtabs_engine::menu::{self, MenuCfg, MenuState, Outcome};
use vtabs_engine::render::frame_of;
use vtabs_engine::settings::document::{
    ApplyMode, DocumentAction, DocumentEffect, RawSettings, SettingsDocument,
};
use vtabs_engine::settings::presentation::SettingsPresentation;
use vtabs_engine::settings::value::{SettingPath, Value};
use vtabs_engine::settings::{self, SettingsView};
use vtabs_engine::spaces::{self, Plan as SpacesPlan};
use vtabs_engine::theme::{self, Theme};
use vtabs_engine::ui::{SettingsUi, UiState};
use vtabs_protocol::limits::{
    FX_MAX_FPS, FX_MAX_MS, FX_MIN_FPS, MENU_MAX_ITEMS, MODEL_MAX_FIELDS, MODEL_MAX_SPACES,
    MODEL_MAX_TABS, SETTINGS_BODY_MAX_BYTES,
};
use vtabs_protocol::{
    Command, Event, Intent, Modifier, SettingsApplyMode, SettingsChange, SettingsPatch, payload,
};

use crate::cli::Cli;
use crate::inbox::{Batch, Inbox, Message, Offer};
use crate::input::{Input, decode_control_line};
use crate::log::Logger;
use crate::paint::{changed_rows_bytes, rows_bytes};
use crate::uservar::{TOKEN_VAR, set_user_var};

const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";
/// Any-motion mouse tracking; the terminal guard turns it on, `config.hover_highlight` may turn it off.
const MOTION_ON: &str = "\x1b[?1003h";
const MOTION_OFF: &str = "\x1b[?1003l";
/// A hole in the inbox sequence waits this long for the file before it counts as lost.
const GAP_GRACE: Duration = Duration::from_millis(100);
/// A window drag resizes the pane once per frame, and a mux server deals every frame to every
/// tab: the frame is adopted once the size has sat still this long, or at the latest after
/// `RESIZE_MAX_WAIT`, so a burst costs a few paints and reports rather than one of each per frame.
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(40);
const RESIZE_MAX_WAIT: Duration = Duration::from_millis(150);
/// An `auth` framed with a token this session does not hold is answered by publishing the one it
/// does, at most this often: a fresh GUI has no user vars for a pane the mux kept.
const TOKEN_REANNOUNCE: Duration = Duration::from_secs(1);

/// A size the terminal reported that the pane has not adopted yet.
#[derive(Debug, Clone, Copy)]
pub struct PendingResize {
    pub size: (u16, u16),
    pub first: Instant,
    pub due: Instant,
}

/// Sole stdout writer, so user-var OSCs never interleave with frame bytes.
pub struct App<W: Write> {
    pub out: W,
    pub log: Logger,
    pub var: String,
    pub size: (u16, u16),
    /// Asked before every frame, so no paint trusts a size the pane no longer has.
    pub probe: fn() -> Option<(u16, u16)>,
    /// The pty's pixel size, asked beside `probe`: the strip's cell metrics come from here, so a
    /// resize lays it out again with no model round trip.
    pub pixel_probe: fn() -> Option<(u16, u16)>,
    /// A size change wipes the pane before the next frame.
    pub needs_clear: bool,
    pub fx: Option<FxRun>,
    /// The last full frame laid out: the final frame any fade lands on.
    pub last_rows: Option<PaintedRows>,
    /// True while the pane shows exactly `last_rows`, so the next repaint may write only the rows
    /// that differ; a fade tick, a wipe or a resize leaves the screen elsewhere and clears it.
    pub shown_is_final: bool,
    pub seq: u64,
    pub sync: SyncState,
    pub ui: UiState,
    pub started: Instant,
    /// Where the menu Lua composed sits, from the last frame; the bridge reports against it.
    pub popover: Option<PopoverHits>,
    /// The menu's own state: the selection Lua no longer drives and the rename buffer Rust owns.
    pub menu_ui: MenuState,
    pub settings_ui: SettingsUi,
    /// Whether `menu_refused` was already sent for the current publication, so a resize does not
    /// repeat it.
    pub menu_refused: bool,
    pub hover_deadline: Option<Instant>,
    /// The last token Lua authed with; a change means the plugin restarted around us.
    pub token: Option<String>,
    /// When the held token was last re-published to a client that framed an auth without it.
    pub token_announced: Option<Instant>,
    /// The newest size a resize burst reported, adopted once the burst has paused.
    pub resize: Option<PendingResize>,
    /// Last effective theme reported to Lua.
    pub last_reported_theme: Option<Theme>,
    /// Last Rust-computed traffic-light reserve reported to this Lua process.
    pub last_rail_reserve: Option<i64>,
    /// The server's own `wezterm cli`, where this pane has one to act for.
    pub cli: Option<Cli>,
    /// The inbox root Lua passed and the way a session wakes the loop; None keeps stdin only.
    pub inbox: Option<Offer>,
    pub transport: Transport,
    /// `auth.keys == "server"`: forwarded keys are written into the content pane by the cli.
    pub server_keys: bool,
    pub metrics: RuntimeMetrics,
}

/// Where this session's frames arrive: stdin only, an offered inbox waiting for its barrier, or
/// the inbox itself, applied in sequence.
#[derive(Default)]
pub enum Transport {
    #[default]
    Off,
    Negotiating {
        inbox: Inbox,
    },
    Active {
        inbox: Inbox,
        last_seq: u32,
        /// When the first hole in the sequence was seen; None while it reads contiguously.
        gap_since: Option<Instant>,
    },
}

impl Transport {
    pub fn session(&self) -> Option<&str> {
        match self {
            Transport::Off => None,
            Transport::Negotiating { inbox } | Transport::Active { inbox, .. } => {
                Some(inbox.session())
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Transport::Off => "off",
            Transport::Negotiating { .. } => "negotiating",
            Transport::Active { .. } => "active",
        }
    }
}

/// What one scanned message means against the sequence so far.
enum Step {
    Duplicate,
    Apply,
    Hold,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeMetrics {
    pub commits: u64,
    pub terminal_paints: u64,
    pub paint_bytes: u64,
}

/// The rows a repaint wrote, kept so a fade has a final frame to land on.
pub struct PaintedRows {
    pub rows: Vec<Option<Vec<Cell>>>,
    pub fades: Vec<Option<f64>>,
    pub page_bg: Color,
    /// The open menu's rows (1-based y, height), the only rows `popover_in` touches.
    pub menu_rows: Option<(i64, i64)>,
}

/// One fade in flight; frames are generated here, nothing crosses the wire per tick.
pub struct FxRun {
    rows: Vec<Option<Vec<Cell>>>,
    fades: Vec<Option<f64>>,
    page_bg: Color,
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

/// One transaction's sections, published together.
#[derive(Clone, Default)]
pub struct Sections {
    pub config: Option<EngineConfig>,
    /// Raw theme input. `resolved_theme` is the only theme rendering consumes.
    pub theme: Option<payload::ThemeMsg>,
    pub resolved_theme: Option<Theme>,
    /// Raw full-window facts and the last Rust-owned projection derived from them.
    pub spaces: Option<payload::SpacesMsg>,
    pub space_resolution: Option<payload::SpaceResolution>,
    pub model: Option<payload::SidebarModel>,
    pub settings: Option<SettingsPresentation>,
    pub settings_document: Option<SettingsDocument>,
    pub menu: Option<payload::MenuMsg>,
}

const SEEN_CONFIG: u8 = 1 << 0;
const SEEN_THEME: u8 = 1 << 1;
const SEEN_MODEL: u8 = 1 << 2;
const SEEN_MENU: u8 = 1 << 3;
const SEEN_SPACES: u8 = 1 << 4;
const SEEN_SETTINGS: u8 = 1 << 5;
const HOOK_TIMEOUT: Duration = Duration::from_millis(500);

struct PendingSync {
    sections: Sections,
    seen: u8,
    valid: bool,
    /// Exact tab ids in the one route-hook batch awaiting a host answer.
    space_hook_requested: Option<Vec<i64>>,
    /// The base-theme hook follows space resolution and is the last publication barrier.
    hook_requested: bool,
    hook_deadline: Option<Instant>,
}

impl PendingSync {
    fn awaiting_hook(&self) -> bool {
        self.space_hook_requested.is_some() || self.hook_requested
    }
}

/// Committed state plus at most one unpublished transaction.
#[derive(Default)]
pub struct SyncState {
    committed: Sections,
    pending: Option<PendingSync>,
    /// A Begin rejected while a hook is outstanding still owns the untagged sections up to its
    /// Commit. Quarantine that batch so it cannot alter the sole hooked transaction.
    discarding: bool,
    /// After a request times out, skip that host hook for the rest of this authenticated session.
    /// With no result identifier, this fail-closed circuit breaker prevents a late answer from
    /// being mistaken for an answer to a future request.
    skip_space_hook: bool,
    skip_theme_hook: bool,
}

impl std::ops::Deref for SyncState {
    type Target = Sections;

    fn deref(&self) -> &Self::Target {
        &self.committed
    }
}

impl std::ops::DerefMut for SyncState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.committed
    }
}

enum Applied {
    Continue,
    Repaint,
    Quit,
}

fn settings_input(msg: payload::SettingsMsg) -> Option<(SettingsPresentation, SettingsDocument)> {
    let version = msg.version;
    let values: Value = serde_json::from_value(msg.values).ok()?;
    if !matches!(values, Value::Table(_)) {
        return None;
    }
    let key_defaults = msg
        .key_defaults
        .into_iter()
        .map(|(key, value)| serde_json::from_value(value).ok().map(|value| (key, value)))
        .collect::<Option<_>>()?;
    let explicit = msg
        .explicit
        .into_iter()
        .filter(|path| !path.is_empty())
        .map(SettingPath)
        .collect();
    let opaque = msg
        .opaque
        .into_iter()
        .filter(|path| !path.is_empty())
        .map(SettingPath)
        .collect();
    let document = SettingsDocument::new(RawSettings {
        values,
        explicit,
        host_values: msg.host_values.into_iter().collect(),
        opaque,
        key_defaults,
        is_macos: msg.is_macos,
    });
    if !document.issues().is_empty() {
        return None;
    }
    let presentation = document.presentation(version);
    Some((presentation, document))
}

fn sidebar_model(msg: payload::ModelMsg) -> payload::SidebarModel {
    payload::SidebarModel {
        rail: msg.rail,
        active: msg.active,
        focus: msg.focus,
        scroll: msg.scroll,
        drag: msg.drag,
        strip: msg.strip,
        footer: msg.footer,
        ..Default::default()
    }
}

fn apply_mode(mode: ApplyMode) -> SettingsApplyMode {
    match mode {
        ApplyMode::Instant => SettingsApplyMode::Instant,
        ApplyMode::Override => SettingsApplyMode::Override,
        ApplyMode::Reload => SettingsApplyMode::Reload,
    }
}

fn modifier_name(modifier: Modifier) -> String {
    match modifier {
        Modifier::Shift => "shift",
        Modifier::Ctrl => "ctrl",
        Modifier::Alt => "alt",
    }
    .to_owned()
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
        self.sync.config.as_ref().is_none_or(|c| c.hover_highlight)
    }

    /// Nothing can be drawn or hit-tested until all three commands have landed at least once.
    fn dressed(&self) -> bool {
        self.sync.config.is_some()
            && self.sync.resolved_theme.is_some()
            && (self.sync.model.is_some() || self.sync.settings.is_some())
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
            Input::Dropped {
                token,
                what,
                reason,
            } => {
                if token.as_deref() == self.token.as_deref() && self.token.is_some() {
                    // A rejected busy batch is quarantined through its Commit; malformed content
                    // in it must not poison the older transaction that is waiting on a hook.
                    if self.sync.discarding {
                        return Ok(true);
                    }
                    // An invalid framed command matters only while it can corrupt a staged
                    // transaction. Outside one, retain the quiet refusal malformed commands have
                    // always had. Size/transport drops remain observable in either state.
                    if what != "command" || self.sync.pending.is_some() {
                        self.invalidate_pending();
                        self.emit(&Event::dropped(what, reason))?;
                    }
                }
            }
            Input::Key { name, mods, raw } => {
                if self.dressed() {
                    self.key(&name, mods, &raw)?;
                } else {
                    self.emit_key(Event::key(name, mods, &raw), &raw)?;
                }
            }
            Input::Paste(data) => self.emit(&Event::paste(data))?,
            Input::Control { token, command } => {
                let authorized = match &command {
                    Command::Auth { token: claimed, .. } => {
                        vtabs_protocol::limits::valid_control_token(claimed)
                            && self
                                .token
                                .as_deref()
                                .map_or(claimed == &token, |active| active == token.as_str())
                    }
                    _ => self.token.as_deref() == Some(token.as_str()),
                };
                if authorized {
                    return self.run(command);
                }
                if matches!(command, Command::Auth { .. }) {
                    self.reannounce_token(Instant::now())?;
                }
            }
            #[cfg(test)]
            Input::Command(command) => return self.run(command),
        }
        Ok(true)
    }

    /// An `auth` this session could not read: a GUI that just attached to the mux frames blind,
    /// since the pane's user vars reach a client only as they change. The token the session holds
    /// is published again so that client can frame its next one; the value was theirs to read all
    /// along, and a paste at the pane gets it once a second at most.
    fn reannounce_token(&mut self, now: Instant) -> io::Result<()> {
        let Some(token) = self.token.clone() else {
            return Ok(());
        };
        if self
            .token_announced
            .is_some_and(|at| now.duration_since(at) < TOKEN_REANNOUNCE)
        {
            return Ok(());
        }
        self.token_announced = Some(now);
        self.log
            .log("auth refused: re-publishing the token this session holds");
        self.write(set_user_var(TOKEN_VAR, &token).as_bytes())
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
            let r = interaction::mouse(&plan, &k, &self.ui, m, now);
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
        let resolved = match (self.menu_outcome(), self.sync.menu.as_ref()) {
            (Outcome::Open(placed), Some(msg)) => {
                let view = MenuView {
                    level: placed.level,
                    items: &msg.items,
                    hits: &placed.hits,
                    follow_pointer: self.menu_cfg().follow_pointer,
                };
                interaction::menu_mouse(&view, &self.menu_ui, &self.ui, m, now)
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
            interaction::settings_mouse(&screen, &self.settings_ui, m)
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
            interaction::settings_key(&screen, &self.settings_ui, name, mods)
        };
        self.apply_settings(resolved)?;
        Ok(true)
    }

    fn apply_settings(&mut self, resolved: interaction::Resolution) -> io::Result<()> {
        if let Some(state) = resolved.settings {
            self.settings_ui = state;
        }
        for event in &resolved.events {
            if let Event::Intent { intent } = event
                && self.apply_document_intent(intent)?
            {
                continue;
            }
            self.emit(event)?;
        }
        if resolved.repaint {
            self.repaint()?;
        }
        Ok(())
    }

    /// Runs settings semantics locally against the committed canonical settings document.
    fn apply_document_intent(&mut self, intent: &Intent) -> io::Result<bool> {
        let Some(document) = self.sync.settings_document.as_ref() else {
            return Ok(false);
        };
        let action = match intent {
            Intent::NudgeOption { key, delta } => {
                document.path_for_key(key).map(|path| DocumentAction::Step {
                    path,
                    delta: *delta,
                })
            }
            Intent::ActivateOption { key } => {
                document.path_for_key(key).map(DocumentAction::Activate)
            }
            Intent::ResetOption { key } => document.path_for_key(key).map(DocumentAction::Reset),
            Intent::SettingsCopy => Some(DocumentAction::Copy),
            Intent::EditKey { key } => Some(DocumentAction::EditKey(key.clone())),
            Intent::RecordChord { key, mods } => Some(DocumentAction::RecordChord {
                key: key.clone(),
                mods: mods.iter().copied().map(modifier_name).collect(),
            }),
            _ => return Ok(false),
        };
        let Some(action) = action else {
            return Ok(true);
        };
        let previous_document = self
            .sync
            .settings_document
            .as_ref()
            .expect("document checked above")
            .clone();
        let effect = self
            .sync
            .settings_document
            .as_mut()
            .expect("document checked above")
            .apply(action)
            .map_err(io::Error::other)?;
        match effect {
            DocumentEffect::None | DocumentEffect::Rejected { .. } => {}
            DocumentEffect::StateChanged => self.refresh_settings_presentation(),
            DocumentEffect::Copy { lua } => self.emit(&Event::SettingsCopy { lua })?,
            DocumentEffect::Commit(effect)
                if effect.persistence_json.len() > SETTINGS_BODY_MAX_BYTES =>
            {
                self.sync.settings_document = Some(previous_document);
                self.refresh_settings_presentation();
                self.emit(&Event::dropped("settings", "size"))?;
            }
            DocumentEffect::Commit(effect) => {
                self.refresh_settings_presentation();
                let change = protocol_setting_change(effect.value)?;
                let derived = effect
                    .derived
                    .into_iter()
                    .map(|change| {
                        Ok(SettingsPatch {
                            path: change.path.0,
                            change: protocol_setting_change(change.value)?,
                        })
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                self.emit(&Event::SettingsCommit {
                    path: effect.path.0,
                    change,
                    derived,
                    mode: apply_mode(effect.mode),
                    persistence_json: effect.persistence_json,
                })?;
            }
        }
        Ok(true)
    }

    fn refresh_settings_presentation(&mut self) {
        let Some(document) = self.sync.settings_document.as_ref() else {
            return;
        };
        let Some(current) = self.sync.settings.as_ref() else {
            return;
        };
        self.sync.settings = Some(document.presentation(current.version.clone()));
    }

    /// The settings widget's input, from the same three messages the sidebar reads.
    fn settings_view(&self) -> Option<SettingsView<'_>> {
        let (cfg, theme, presentation) = (
            self.sync.config.as_ref()?,
            self.sync.resolved_theme.as_ref()?,
            self.sync.settings.as_ref()?,
        );
        Some(SettingsView {
            cols: self.dims().0,
            rows: self.dims().1,
            presentation,
            ui: &self.settings_ui,
            theme: theme.clone(),
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
            interaction::key(&k, name, mods, raw).events
        };
        for event in events {
            match event {
                Event::Key { .. } => self.emit_key(event, raw)?,
                other => self.emit(&other)?,
            }
        }
        Ok(())
    }

    /// A key the sidebar hands to the content pane: written there through the cli when Lua asked
    /// for server-side delivery and this pane has one, otherwise left to Lua as before.
    fn emit_key(&mut self, event: Event, raw: &[u8]) -> io::Result<()> {
        if self.server_keys
            && !raw.is_empty()
            && let Some(cli) = self.cli.as_ref()
        {
            match cli.forward_key(raw) {
                Ok(pane) => {
                    self.log.log(format!("key delivered to pane {pane}"));
                    return self.emit(&event.delivered());
                }
                Err(err) => self.log.log(format!("key forward: {err}")),
            }
        }
        self.emit(&event)
    }

    /// While the menu is open it consumes every key: navigation, first-letter jump, edit buffer.
    fn menu_key(&mut self, name: &str, mods: vtabs_protocol::Mods) -> io::Result<bool> {
        let resolved = match (self.menu_outcome(), self.sync.menu.as_ref()) {
            (Outcome::Open(placed), Some(msg)) => {
                let view = MenuView {
                    level: placed.level,
                    items: &msg.items,
                    hits: &placed.hits,
                    follow_pointer: self.menu_cfg().follow_pointer,
                };
                interaction::menu_key(&view, &self.menu_ui, &self.ui, name, mods)
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
        let Some(cfg) = self.sync.config.as_ref() else {
            return MenuCfg::default();
        };
        let padding = cfg.render.padding;
        MenuCfg {
            padding_left: padding.left,
            padding_right: padding.right,
            want_width: cfg.popover.width,
            ellipsis: cfg.ellipsis.clone(),
            follow_pointer: cfg.popover.follow_pointer,
        }
    }

    /// What the stored menu message asks for, against this pane's size.
    fn menu_outcome(&self) -> Outcome {
        let (Some(msg), Some(theme)) = (self.effective_menu(), self.sync.resolved_theme.as_ref())
        else {
            return Outcome::Closed;
        };
        let header = self
            .sync
            .model
            .as_ref()
            .zip(self.sync.config.as_ref())
            .and_then(|(model, cfg)| vtabs_engine::enrich::menu_header(&msg, model, cfg));
        menu::plan(
            &msg,
            header.as_ref(),
            &self.menu_ui,
            &self.menu_cfg(),
            theme,
            self.dims(),
        )
    }

    fn effective_menu(&self) -> Option<payload::MenuMsg> {
        self.sync.menu.clone()
    }

    fn scene(&self) -> (Enriched, Outcome) {
        let (cfg, theme, model) = self.state();
        let mut e = enrich(cfg, theme, model, self.dims(), &self.ui);
        let effective_menu = self.effective_menu();
        let outcome = match effective_menu.as_ref() {
            Some(msg) => {
                let header = vtabs_engine::enrich::menu_header(msg, model, cfg);
                menu::plan(
                    msg,
                    header.as_ref(),
                    &self.menu_ui,
                    &self.menu_cfg(),
                    &e.view.theme,
                    self.dims(),
                )
            }
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

    fn state(&self) -> (&EngineConfig, &Theme, &payload::SidebarModel) {
        let model = self.sync.model.as_ref().expect("dressed");
        (
            self.sync.config.as_ref().expect("dressed"),
            self.sync.resolved_theme.as_ref().expect("dressed"),
            model,
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
        let (painted, popover, outcome, selected, rail_reserve) = {
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
            (painted, e.popover, outcome, selected, e.rail_reserve)
        };
        self.popover = popover;
        if let Some(selected) = selected {
            self.menu_ui.selected = selected;
        }
        self.refuse(&outcome)?;
        self.report_rail_reserve(rail_reserve)?;
        self.paint(painted, shown.as_ref())
    }

    fn report_rail_reserve(&mut self, cols: Option<i64>) -> io::Result<()> {
        let Some(cols) = cols else { return Ok(()) };
        if self.last_rail_reserve == Some(cols) {
            return Ok(());
        }
        self.emit(&Event::intent(Intent::SetRailReserve { cols }))?;
        self.last_rail_reserve = Some(cols);
        Ok(())
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
            self.write_paint(&bytes)?;
        }
        Ok(())
    }

    fn write_paint(&mut self, bytes: &str) -> io::Result<()> {
        self.write(bytes.as_bytes())?;
        self.metrics.terminal_paints = self.metrics.terminal_paints.saturating_add(1);
        self.metrics.paint_bytes = self.metrics.paint_bytes.saturating_add(bytes.len() as u64);
        self.log_paint();
        Ok(())
    }

    fn log_paint(&mut self) {
        self.log.log(self.paint_log_line());
    }

    fn paint_log_line(&self) -> String {
        let (cols, rows) = self.size;
        format!(
            "paint {cols}x{rows} totals={{commits={},paints={},bytes={}}}",
            self.metrics.commits, self.metrics.terminal_paints, self.metrics.paint_bytes,
        )
    }

    /// An open level that cannot be placed is Lua's to unwind; send the event once per publication.
    fn refuse(&mut self, outcome: &Outcome) -> io::Result<()> {
        let Outcome::Refused { why, level } = outcome else {
            return Ok(());
        };
        if self.menu_refused {
            return Ok(());
        }
        self.menu_refused = true;
        let id = self.sync.menu.as_ref().and_then(|m| m.target);
        self.emit(&Event::MenuRefused {
            why: Some(why),
            id,
            level: Some(level.name()),
        })
    }

    /// Hover goes stale on its own clock, so a pointer that left the pane stops lighting a row.
    fn arm_hover(&mut self) {
        let ms = self.sync.config.as_ref().map_or(0, |c| c.hover_timeout_ms);
        self.hover_deadline = (ms > 0 && self.ui.hover.is_some())
            .then(|| Instant::now() + std::time::Duration::from_millis(ms + 1));
    }

    pub fn next_hover(&self) -> Option<Instant> {
        self.hover_deadline
    }

    pub fn tick_hover(&mut self, now: Instant) -> io::Result<()> {
        if self.hover_deadline.is_some_and(|at| now >= at) {
            self.hover_deadline = None;
            let ms = self.sync.config.as_ref().map_or(0, |c| c.hover_timeout_ms);
            if self.ui.expire_hover(self.now_ms(), ms) {
                self.repaint()?;
            }
        }
        Ok(())
    }

    pub fn next_hook_deadline(&self) -> Option<Instant> {
        self.sync
            .pending
            .as_ref()
            .and_then(|pending| pending.hook_deadline)
    }

    /// Commits a deterministic no-hook fallback if the sole host-hook request gets no answer.
    /// Requests are not replayed: an uncorrelated duplicate answer could otherwise satisfy a later
    /// transaction.
    pub fn tick_hooks(&mut self, now: Instant) -> io::Result<()> {
        let due = self
            .sync
            .pending
            .as_ref()
            .is_some_and(|pending| pending.hook_deadline.is_some_and(|at| now >= at));
        if !due {
            return Ok(());
        }
        let what = if self
            .sync
            .pending
            .as_ref()
            .is_some_and(|pending| pending.space_hook_requested.is_some())
        {
            "space_route_hook"
        } else {
            "theme_hook"
        };
        let applied = self.fallback_pending_hook(what, "timeout")?;
        if matches!(applied, Applied::Repaint) {
            self.settle_scroll();
            self.repaint()?;
        }
        Ok(())
    }

    fn fallback_pending_hook(
        &mut self,
        what: &'static str,
        reason: &'static str,
    ) -> io::Result<Applied> {
        if self.sync.pending.is_none() {
            return Ok(Applied::Continue);
        }
        if reason == "timeout" {
            match what {
                "space_route_hook" => self.sync.skip_space_hook = true,
                "theme_hook" => self.sync.skip_theme_hook = true,
                _ => {}
            }
        }
        self.emit(&Event::dropped(what, reason))?;
        let requested = self
            .sync
            .pending
            .as_ref()
            .and_then(|pending| pending.space_hook_requested.clone());
        if let Some(requested) = requested {
            let answers = requested
                .into_iter()
                .map(|tab_id| payload::SpaceRouteHookAnswer {
                    tab_id,
                    space: None,
                })
                .collect::<Vec<_>>();
            let plan = {
                let pending = self.sync.pending.as_ref().expect("matching pending sync");
                let (Some(input), Some(theme)) = (
                    pending.sections.spaces.as_ref(),
                    pending.sections.theme.as_ref(),
                ) else {
                    self.sync.pending.take();
                    return Ok(Applied::Continue);
                };
                spaces::plan(input, &theme.scheme, Some(&answers))
            };
            let SpacesPlan::Resolved(resolution) = plan else {
                self.sync.pending.take();
                return Ok(Applied::Continue);
            };
            let pending = self.sync.pending.as_mut().expect("matching pending sync");
            pending.space_hook_requested = None;
            pending.hook_deadline = None;
            Self::apply_space_resolution(&mut pending.sections, *resolution);
            return self.continue_sync();
        }
        let Some(mut pending) = self.sync.pending.take() else {
            return Ok(Applied::Continue);
        };
        pending.hook_requested = false;
        pending.hook_deadline = None;
        self.publish_pending(pending, None)
    }

    fn run(&mut self, cmd: Command) -> io::Result<bool> {
        match self.apply(cmd)? {
            Applied::Quit => Ok(false),
            Applied::Continue => Ok(true),
            Applied::Repaint => {
                self.settle_scroll();
                self.repaint()?;
                Ok(true)
            }
        }
    }

    /// The optimistic wheel override retires once the model comes back carrying it.
    fn settle_scroll(&mut self) {
        let landed = self
            .sync
            .model
            .as_ref()
            .and_then(|m| m.scroll)
            .is_some_and(|s| s.user && Some(s.top) == self.ui.scroll);
        if landed {
            self.ui.scroll = None;
        }
    }

    /// Sections may only mutate the pending transaction between Begin and Commit.
    fn stage(&mut self, seen: u8, update: impl FnOnce(&mut Sections)) {
        let Some(pending) = self.sync.pending.as_mut() else {
            return;
        };
        if pending.valid && !pending.awaiting_hook() {
            update(&mut pending.sections);
            pending.seen |= seen;
        }
    }

    fn ignores_section(&self) -> bool {
        self.sync.discarding
            || self
                .sync
                .pending
                .as_ref()
                .is_none_or(PendingSync::awaiting_hook)
    }

    fn invalidate_pending(&mut self) {
        if let Some(pending) = self.sync.pending.as_mut() {
            pending.valid = false;
        }
    }

    fn resolve_sections(
        sections: &Sections,
        hook: Option<&payload::ThemeOverrides>,
    ) -> Option<Theme> {
        let raw = sections.theme.as_ref()?;
        if sections.model.is_none() && sections.settings.is_none() {
            return None;
        }
        let mut layered = raw.overrides.clone();
        if let Some(spaces) = &sections.space_resolution {
            layered = theme::overlay(&layered, &spaces.theme_overrides);
        }
        if let Some(hook) = hook {
            layered = theme::overlay(&layered, hook);
        }
        Some(theme::resolve(&layered, &raw.scheme, raw.private))
    }

    fn apply_space_resolution(sections: &mut Sections, resolution: payload::SpaceResolution) {
        if let (Some(spaces), Some(model)) = (&sections.spaces, sections.model.as_mut()) {
            let visible = resolution
                .visible_tab_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            model.tabs = spaces
                .tabs
                .iter()
                .filter(|fact| visible.contains(&fact.tab.id))
                .map(|fact| fact.tab.clone())
                .collect();
            model.space = resolution.active.clone();
            model.spaces = resolution
                .summary
                .iter()
                .map(|space| payload::SpaceItem {
                    id: space.id.clone(),
                    name: space.name.clone(),
                    icon: space.icon.clone(),
                    unseen: space.unseen,
                })
                .collect();
        }
        sections.space_resolution = Some(resolution);
    }

    fn valid_space_answer(requested: &[i64], routes: &[payload::SpaceRouteHookAnswer]) -> bool {
        if requested.len() != routes.len() {
            return false;
        }
        let expected = requested.iter().copied().collect::<BTreeSet<_>>();
        let received = routes
            .iter()
            .map(|answer| answer.tab_id)
            .collect::<BTreeSet<_>>();
        expected.len() == requested.len() && received.len() == routes.len() && expected == received
    }

    fn valid_spaces_input(input: &payload::SpacesMsg) -> bool {
        if input.tabs.len() > MODEL_MAX_TABS
            || input.dynamics.len() > MODEL_MAX_SPACES
            || input.last_tabs.len() > MODEL_MAX_SPACES
        {
            return false;
        }
        let tabs = input
            .tabs
            .iter()
            .map(|fact| fact.tab.id)
            .collect::<BTreeSet<_>>();
        let dynamics = input
            .dynamics
            .iter()
            .map(|space| space.id.as_str())
            .collect::<BTreeSet<_>>();
        let last_tabs = input
            .last_tabs
            .iter()
            .map(|entry| entry.space_id.as_str())
            .collect::<BTreeSet<_>>();
        tabs.len() == input.tabs.len()
            && dynamics.len() == input.dynamics.len()
            && last_tabs.len() == input.last_tabs.len()
    }

    fn report_committed_theme(&mut self) -> io::Result<()> {
        let Some(effective) = self.sync.resolved_theme.as_ref() else {
            return Ok(());
        };
        let reported = effective.clone();
        if self.last_reported_theme.as_ref() == Some(&reported) {
            return Ok(());
        }
        self.emit(&Event::ThemeResolved {
            theme: reported.clone(),
        })?;
        self.last_reported_theme = Some(reported);
        Ok(())
    }

    fn publish_pending(
        &mut self,
        mut pending: PendingSync,
        hook: Option<&payload::ThemeOverrides>,
    ) -> io::Result<Applied> {
        if let (Some(model), Some(theme)) = (
            pending.sections.model.as_mut(),
            pending.sections.theme.as_ref(),
        ) {
            model.private = theme.private;
        }
        let Some(effective) = Self::resolve_sections(&pending.sections, hook) else {
            return Ok(Applied::Continue);
        };
        pending.sections.resolved_theme = Some(effective);

        let previous_motion = self.hover_highlight();
        self.sync.committed = pending.sections;
        self.metrics.commits = self.metrics.commits.saturating_add(1);
        self.menu_refused = false;
        let next_motion = self.hover_highlight();
        if previous_motion != next_motion {
            self.write(if next_motion {
                MOTION_ON.as_bytes()
            } else {
                MOTION_OFF.as_bytes()
            })?;
        }
        if let Some(menu) = self.sync.menu.as_ref() {
            self.menu_ui.adopt(menu);
        }
        if let Some(spaces) = self.sync.space_resolution.clone() {
            let window_id = self
                .sync
                .spaces
                .as_ref()
                .map(|input| input.window_id)
                .unwrap_or_default();
            self.emit(&Event::SpacesResolved {
                window_id,
                resolution: Box::new(spaces),
            })?;
        }
        self.report_committed_theme()?;
        Ok(Applied::Repaint)
    }

    fn begin_sync(&mut self) -> io::Result<()> {
        if self.sync.discarding
            || self
                .sync
                .pending
                .as_ref()
                .is_some_and(PendingSync::awaiting_hook)
        {
            self.sync.discarding = true;
            self.emit(&Event::dropped("sync", "busy"))?;
            return Ok(());
        }
        // A repeated Begin before Commit restarts staging from committed state. Later
        // transactions may replace only changed sections; the resulting state remains complete.
        self.sync.pending = Some(PendingSync {
            sections: self.sync.committed.clone(),
            seen: 0,
            valid: true,
            space_hook_requested: None,
            hook_requested: false,
            hook_deadline: None,
        });
        Ok(())
    }

    fn continue_sync(&mut self) -> io::Result<Applied> {
        let needs_plan = self
            .sync
            .pending
            .as_ref()
            .is_some_and(|pending| pending.sections.space_resolution.is_none());
        if needs_plan {
            let plan = {
                let pending = self.sync.pending.as_ref().expect("matching pending sync");
                let Some(input) = pending.sections.spaces.as_ref() else {
                    self.sync.pending.take();
                    return Ok(Applied::Continue);
                };
                let Some(theme) = pending.sections.theme.as_ref() else {
                    self.sync.pending.take();
                    return Ok(Applied::Continue);
                };
                spaces::plan(input, &theme.scheme, None)
            };
            match plan {
                SpacesPlan::NeedsHooks { tabs } => {
                    if self.sync.skip_space_hook {
                        let answers = tabs
                            .iter()
                            .map(|tab| payload::SpaceRouteHookAnswer {
                                tab_id: tab.tab_id,
                                space: None,
                            })
                            .collect::<Vec<_>>();
                        let resolution = {
                            let pending =
                                self.sync.pending.as_ref().expect("matching pending sync");
                            let input = pending
                                .sections
                                .spaces
                                .as_ref()
                                .expect("validated spaces section");
                            let theme = pending
                                .sections
                                .theme
                                .as_ref()
                                .expect("validated theme section");
                            let SpacesPlan::Resolved(resolution) =
                                spaces::plan(input, &theme.scheme, Some(&answers))
                            else {
                                self.sync.pending.take();
                                return Ok(Applied::Continue);
                            };
                            resolution
                        };
                        let pending = self.sync.pending.as_mut().expect("matching pending sync");
                        Self::apply_space_resolution(&mut pending.sections, *resolution);
                    } else {
                        let window_id = self
                            .sync
                            .pending
                            .as_ref()
                            .and_then(|pending| pending.sections.spaces.as_ref())
                            .map(|input| input.window_id)
                            .unwrap_or_default();
                        let requested = tabs.iter().map(|tab| tab.tab_id).collect();
                        let event = Event::SpaceRouteHookRequest { window_id, tabs };
                        let pending = self.sync.pending.as_mut().expect("matching pending sync");
                        pending.space_hook_requested = Some(requested);
                        pending.hook_deadline = Some(Instant::now() + HOOK_TIMEOUT);
                        self.emit(&event)?;
                        return Ok(Applied::Continue);
                    }
                }
                SpacesPlan::Resolved(resolution) => {
                    let pending = self.sync.pending.as_mut().expect("matching pending sync");
                    Self::apply_space_resolution(&mut pending.sections, *resolution);
                }
            }
        }

        let pending = self.sync.pending.as_ref().expect("matching pending sync");
        if pending.hook_requested {
            return Ok(Applied::Continue);
        }
        let needs_hook = pending
            .sections
            .theme
            .as_ref()
            .is_some_and(|theme| theme.hook);
        if needs_hook {
            if self.sync.skip_theme_hook {
                let pending = self.sync.pending.take().expect("matching pending sync");
                return self.publish_pending(pending, None);
            }
            let Some(base) = Self::resolve_sections(&pending.sections, None) else {
                self.sync.pending.take();
                return Ok(Applied::Continue);
            };
            let event = Event::ThemeHookRequest { theme: base };
            let pending = self.sync.pending.as_mut().expect("matching pending sync");
            pending.hook_requested = true;
            pending.hook_deadline = Some(Instant::now() + HOOK_TIMEOUT);
            self.emit(&event)?;
            return Ok(Applied::Continue);
        }
        let pending = self.sync.pending.take().expect("matching pending sync");
        self.publish_pending(pending, None)
    }

    fn commit_sync(&mut self) -> io::Result<Applied> {
        if self.sync.discarding {
            self.sync.discarding = false;
            return Ok(Applied::Continue);
        }
        let Some(pending) = self.sync.pending.as_ref() else {
            return Ok(Applied::Continue);
        };
        if !pending.valid
            || pending.seen == 0
            || pending.sections.config.is_none()
            || pending.sections.theme.is_none()
            || pending.sections.spaces.is_none()
            || (pending.sections.model.is_none() && pending.sections.settings.is_none())
            || (pending.sections.model.is_some() && pending.sections.menu.is_none())
        {
            self.sync.pending.take();
            return Ok(Applied::Continue);
        }
        if pending.awaiting_hook() {
            return Ok(Applied::Continue);
        }
        self.continue_sync()
    }

    fn space_hook_result(
        &mut self,
        routes: Vec<payload::SpaceRouteHookAnswer>,
    ) -> io::Result<Applied> {
        let Some(requested) = self
            .sync
            .pending
            .as_ref()
            .filter(|pending| pending.valid)
            .and_then(|pending| pending.space_hook_requested.clone())
        else {
            return Ok(Applied::Continue);
        };
        if !Self::valid_space_answer(&requested, &routes) {
            return self.fallback_pending_hook("space_route_hook_result", "invalid");
        }
        let plan = {
            let pending = self.sync.pending.as_ref().expect("matching pending sync");
            let (Some(input), Some(theme)) = (
                pending.sections.spaces.as_ref(),
                pending.sections.theme.as_ref(),
            ) else {
                self.invalidate_pending();
                return Ok(Applied::Continue);
            };
            spaces::plan(input, &theme.scheme, Some(&routes))
        };
        let SpacesPlan::Resolved(resolution) = plan else {
            return self.fallback_pending_hook("space_route_hook_result", "incomplete");
        };
        let pending = self.sync.pending.as_mut().expect("matching pending sync");
        pending.space_hook_requested = None;
        pending.hook_deadline = None;
        Self::apply_space_resolution(&mut pending.sections, *resolution);
        self.continue_sync()
    }

    fn theme_hook_result(&mut self, overrides: payload::ThemeOverrides) -> io::Result<Applied> {
        let matches = self
            .sync
            .pending
            .as_ref()
            .is_some_and(|pending| pending.valid && pending.hook_requested);
        if !matches {
            return Ok(Applied::Continue);
        }
        if !theme::valid_overrides(&overrides) {
            return self.fallback_pending_hook("theme_hook_result", "invalid");
        }
        let pending = self.sync.pending.take().expect("matching pending sync");
        self.publish_pending(pending, Some(&overrides))
    }

    fn reset_for_auth(&mut self) -> io::Result<()> {
        let motion_was_on = self.hover_highlight();
        self.sync = SyncState::default();
        self.ui = UiState::default();
        self.menu_ui = MenuState::default();
        self.settings_ui = SettingsUi::default();
        self.popover = None;
        self.menu_refused = false;
        self.hover_deadline = None;
        self.fx = None;
        self.last_rows = None;
        self.shown_is_final = false;
        self.needs_clear = false;
        self.last_reported_theme = None;
        self.last_rail_reserve = None;
        self.transport = Transport::Off;
        self.server_keys = false;
        if !motion_was_on {
            self.write(MOTION_ON.as_bytes())?;
        }
        self.write(CLEAR_SCREEN.as_bytes())
    }

    fn apply(&mut self, cmd: Command) -> io::Result<Applied> {
        match cmd {
            Command::Clear => {
                self.needs_clear = true;
                return Ok(Applied::Repaint);
            }
            Command::Ping { n } => self.emit(&Event::Pong { echo: n })?,
            Command::Auth { token, keys } => {
                self.write(set_user_var(TOKEN_VAR, &token).as_bytes())?;
                let server_keys = keys.as_deref() == Some("server");
                // A new token is a new Lua process: the mux kept this backend but the plugin lost
                // its process state, and only `ready` restores it.
                // Re-announcing on the same token would ping-pong, since Lua re-auths on ready.
                let renewed = self.token.as_deref() != Some(token.as_str());
                if renewed {
                    self.token = Some(token);
                    self.reset_for_auth()?;
                }
                self.server_keys = server_keys;
                if renewed {
                    self.announce_ready()?;
                }
            }
            Command::Begin => self.begin_sync()?,
            Command::Commit => return self.commit_sync(),
            Command::ThemeHookResult { overrides } => {
                return self.theme_hook_result(*overrides);
            }
            Command::SpaceRouteHookResult { routes } => {
                return self.space_hook_result(routes);
            }
            Command::Config(msg) => {
                if self.ignores_section() {
                    return Ok(Applied::Continue);
                }
                let Ok(msg) = EngineConfig::try_from(*msg) else {
                    self.invalidate_pending();
                    self.emit(&Event::dropped("config", "invalid"))?;
                    return Ok(Applied::Continue);
                };
                self.stage(SEEN_CONFIG, |sections| sections.config = Some(msg));
                return Ok(Applied::Continue);
            }
            Command::Theme(msg) => {
                if self.ignores_section() {
                    return Ok(Applied::Continue);
                }
                self.stage(SEEN_THEME, |sections| {
                    sections.theme = Some(*msg);
                    sections.resolved_theme = None;
                    // Automatic space accents are selected from this raw palette.
                    sections.space_resolution = None;
                });
                return Ok(Applied::Continue);
            }
            Command::Spaces(msg) => {
                if self.ignores_section() {
                    return Ok(Applied::Continue);
                }
                if !Self::valid_spaces_input(&msg) {
                    self.invalidate_pending();
                    self.emit(&Event::dropped("spaces", "bounds"))?;
                    return Ok(Applied::Continue);
                }
                self.stage(SEEN_SPACES, |sections| {
                    sections.spaces = Some(*msg);
                    sections.space_resolution = None;
                });
                return Ok(Applied::Continue);
            }
            Command::Model(msg) => {
                if self.ignores_section() {
                    return Ok(Applied::Continue);
                }
                self.stage(SEEN_MODEL, |sections| {
                    sections.model = Some(sidebar_model(*msg));
                    sections.settings = None;
                    sections.settings_document = None;
                    if let Some(resolution) = sections.space_resolution.clone() {
                        Self::apply_space_resolution(sections, resolution);
                    }
                });
                return Ok(Applied::Continue);
            }
            Command::Settings(msg) => {
                if self.ignores_section() {
                    return Ok(Applied::Continue);
                }
                let Some((presentation, document)) = settings_input(*msg) else {
                    self.invalidate_pending();
                    self.emit(&Event::dropped("settings", "invalid"))?;
                    return Ok(Applied::Continue);
                };
                if presentation.fields.len() > MODEL_MAX_FIELDS {
                    self.invalidate_pending();
                    self.emit(&Event::dropped("settings", "bounds"))?;
                    return Ok(Applied::Continue);
                }
                self.stage(SEEN_SETTINGS, |sections| {
                    sections.model = None;
                    sections.menu = None;
                    sections.settings = Some(presentation);
                    sections.settings_document = Some(document);
                });
                return Ok(Applied::Continue);
            }
            Command::Menu(msg) => {
                if self.ignores_section() {
                    return Ok(Applied::Continue);
                }
                if msg.items.len() > MENU_MAX_ITEMS {
                    self.invalidate_pending();
                    self.emit(&Event::dropped("menu", "bounds"))?;
                    return Ok(Applied::Continue);
                }
                self.stage(SEEN_MENU, |sections| sections.menu = Some(*msg));
                return Ok(Applied::Continue);
            }
            Command::Fx(msg) => self.start_fx(&msg)?,
            Command::Notice(msg) => self.log.log(format!(
                "notice {} {}",
                msg.level.as_deref().unwrap_or("info"),
                msg.text
            )),
            Command::Kill { pane } => {
                let done = self
                    .cli
                    .as_ref()
                    .ok_or_else(|| "no cli here".to_string())
                    .and_then(|cli| cli.kill_pane(pane));
                self.report("kill", done.map(|()| String::new()))?;
            }
            Command::Rescue { band, position } => {
                let done = match position.as_str() {
                    "left" | "right" => self
                        .cli
                        .as_ref()
                        .ok_or_else(|| "no cli here".to_string())
                        .and_then(|cli| cli.rescue(i64::from(band), position == "right")),
                    _ => Err(format!("rescue: invalid position {position}")),
                };
                self.report("rescue", done.map(|n| n.to_string()))?;
            }
            Command::Adjust {
                target,
                min_content,
            } => {
                let done = self
                    .cli
                    .as_ref()
                    .ok_or_else(|| "no cli here".to_string())
                    .and_then(|cli| cli.adjust(target, min_content));
                let done = done.map(|()| String::new());
                // The split moved under this pane before the cli answered: the pane adopts the
                // size now and says so, rather than leaving Lua to wait for a SIGWINCH.
                let cols = if done.is_ok() {
                    self.poll_size()?;
                    Some(self.size.0)
                } else {
                    None
                };
                self.report_cols("adjust", done, cols)?;
            }
            Command::TransportProbe { .. } => {}
            Command::TransportBarrier { session } => self.transport_barrier(session)?,
            Command::TransportStop { session } => {
                if !self.transport_stop(session)? {
                    return Ok(Applied::Quit);
                }
            }
            Command::Quit => {
                self.transport = Transport::Off;
                return Ok(Applied::Quit);
            }
        }
        Ok(Applied::Continue)
    }

    /// `ready`, offering a fresh inbox session whenever a root was given and can be prepared.
    pub fn announce_ready(&mut self) -> io::Result<()> {
        let pane = self
            .cli
            .as_ref()
            .map(Cli::own_pane)
            .ok_or_else(|| io::Error::other("WEZTERM_PANE is required"))?;
        self.transport = Transport::Off;
        if let Some(offer) = self.inbox.as_ref() {
            match Inbox::create(&offer.root) {
                Ok(inbox) => {
                    self.log.log(format!("inbox {} offered", inbox.session()));
                    self.transport = Transport::Negotiating { inbox };
                }
                Err(err) => self.log.log(format!("inbox: {err}")),
            }
        }
        let inbox = self.transport.session().map(str::to_owned);
        self.emit(&Event::ready_at(self.size.0, self.size.1, pane, inbox))
    }

    /// The probe is the only record of message 1, framed with this session's token, naming it.
    fn probe_ok(&self, messages: &[Message], session: &str) -> bool {
        let Some((1, bytes)) = messages.first() else {
            return false;
        };
        let mut lines = bytes.split_inclusive(|b| *b == b'\n');
        let (Some(line), None) = (lines.next(), lines.next()) else {
            return false;
        };
        matches!(
            decode_control_line(line),
            Some(Input::Control {
                token,
                command: Command::TransportProbe { session: named },
            }) if Some(token.as_str()) == self.token.as_deref() && named == session
        )
    }

    /// The last stdin frame of a negotiation: one scan decides whether the probe made it.
    fn transport_barrier(&mut self, session: String) -> io::Result<()> {
        let outcome = match &self.transport {
            Transport::Off => Err("state"),
            Transport::Negotiating { inbox } | Transport::Active { inbox, .. }
                if inbox.session() != session =>
            {
                Err("session")
            }
            Transport::Active { .. } => Ok(false),
            Transport::Negotiating { inbox } => {
                if self.probe_ok(&inbox.scan(), &session) {
                    Ok(true)
                } else {
                    Err("probe")
                }
            }
        };
        match outcome {
            Ok(true) => {
                let Transport::Negotiating { inbox } = std::mem::take(&mut self.transport) else {
                    return Ok(());
                };
                inbox.remove(1);
                if let Some(offer) = self.inbox.as_ref() {
                    inbox.watch(offer.wake.clone(), true);
                }
                self.transport = Transport::Active {
                    inbox,
                    last_seq: 1,
                    gap_since: None,
                };
                self.log.log(format!("inbox {session} active"));
                self.emit(&Event::TransportReady { session })
            }
            Ok(false) => self.emit(&Event::TransportReady { session }),
            Err(why) => {
                if why == "probe" {
                    self.transport = Transport::Off;
                }
                self.log.log(format!("inbox {session} refused: {why}"));
                self.emit(&Event::TransportRefused { session, why })
            }
        }
    }

    /// Lua is back on stdin: whatever the inbox still holds is applied in order, then it goes.
    fn transport_stop(&mut self, session: String) -> io::Result<bool> {
        if self.transport.session() != Some(session.as_str()) {
            return Ok(true);
        }
        let messages = match &self.transport {
            Transport::Active { inbox, .. } => inbox.scan(),
            _ => Vec::new(),
        };
        let alive = self.apply_messages(&session, messages, usize::MAX)?;
        self.transport = Transport::Off;
        self.log.log(format!("inbox {session} stopped"));
        Ok(alive)
    }

    /// A batch the reader thread scanned; only the active session's count.
    pub fn inbox_batch(&mut self, batch: Batch) -> io::Result<bool> {
        if !matches!(&self.transport, Transport::Active { inbox, .. } if inbox.session() == batch.session)
        {
            return Ok(true);
        }
        self.apply_messages(&batch.session, batch.messages, 0)
    }

    /// Applies `messages` in sequence from where `session` left off. A hole holds everything
    /// behind it, unless `skip` missing numbers may still be written off as lost, one `dropped`
    /// each. False once a record asked the backend to quit.
    fn apply_messages(
        &mut self,
        session: &str,
        messages: Vec<Message>,
        mut skip: usize,
    ) -> io::Result<bool> {
        for (seq, bytes) in messages {
            let step = loop {
                let Transport::Active {
                    inbox,
                    last_seq,
                    gap_since,
                } = &mut self.transport
                else {
                    return Ok(true);
                };
                if inbox.session() != session {
                    return Ok(true);
                }
                if seq <= *last_seq {
                    inbox.remove(seq);
                    break Step::Duplicate;
                }
                if seq == *last_seq + 1 {
                    // Consumed before it runs: a stop or auth inside must not replay this file.
                    inbox.remove(seq);
                    *last_seq = seq;
                    *gap_since = None;
                    break Step::Apply;
                }
                if skip == 0 {
                    gap_since.get_or_insert(Instant::now());
                    break Step::Hold;
                }
                skip -= 1;
                *last_seq += 1;
                *gap_since = None;
                let lost = *last_seq;
                self.emit(&Event::dropped_message(lost))?;
            };
            match step {
                Step::Hold => return Ok(true),
                Step::Duplicate => continue,
                Step::Apply => {}
            }
            for line in bytes.split_inclusive(|b| *b == b'\n') {
                if let Some(input) = decode_control_line(line)
                    && !self.handle(input)?
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    pub fn next_transport(&self) -> Option<Instant> {
        match &self.transport {
            Transport::Active {
                gap_since: Some(at),
                ..
            } => Some(*at + GAP_GRACE),
            _ => None,
        }
    }

    /// Once a hole outlived its grace, one scan decides: the file arrived, or that seq is lost.
    pub fn tick_transport(&mut self, now: Instant) -> io::Result<bool> {
        if !self.next_transport().is_some_and(|at| now >= at) {
            return Ok(true);
        }
        let Transport::Active {
            inbox, gap_since, ..
        } = &mut self.transport
        else {
            return Ok(true);
        };
        *gap_since = None;
        let session = inbox.session().to_owned();
        let messages = inbox.scan();
        self.apply_messages(&session, messages, 1)
    }

    /// One `cli` event per server-side verb: the count or an empty detail on success, the error otherwise.
    fn report(&mut self, op: &'static str, done: Result<String, String>) -> io::Result<()> {
        self.report_cols(op, done, None)
    }

    fn report_cols(
        &mut self,
        op: &'static str,
        done: Result<String, String>,
        cols: Option<u16>,
    ) -> io::Result<()> {
        self.log.log(format!("cli {op}: {done:?}"));
        let (ok, detail) = match done {
            Ok(detail) => (true, detail),
            Err(detail) => (false, detail),
        };
        self.emit(&Event::Cli {
            op,
            ok,
            detail,
            cols,
        })
    }

    pub fn next_fx(&self) -> Option<Instant> {
        self.fx.as_ref().map(|run| run.next_at)
    }

    /// A fade over the frame the pane shows: the phase's rows rise out of the page colour.
    fn start_fx(&mut self, msg: &payload::FxMsg) -> io::Result<()> {
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
        self.write_paint(&first)?;
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
        self.write_paint(&bytes)?;
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

    /// One frame of a resize burst: the size is noted, and adopted by `tick_resize` once no
    /// further frame has landed for `RESIZE_DEBOUNCE`, or `RESIZE_MAX_WAIT` after the first.
    pub fn note_resize(&mut self, now: Instant) {
        let Some(size) = (self.probe)() else {
            return;
        };
        if size == self.size {
            self.resize = None;
            return;
        }
        let first = self.resize.map_or(now, |pending| pending.first);
        self.resize = Some(PendingResize {
            size,
            first,
            due: (now + RESIZE_DEBOUNCE).min(first + RESIZE_MAX_WAIT),
        });
    }

    pub fn next_resize(&self) -> Option<Instant> {
        self.resize.map(|pending| pending.due)
    }

    /// Adopts the burst's last size once it is due; the terminal is asked again, in case a frame
    /// landed since the note.
    pub fn tick_resize(&mut self, now: Instant) -> io::Result<()> {
        if self.next_resize().is_some_and(|due| now >= due) {
            self.resize = None;
            self.poll_size()?;
        }
        Ok(())
    }

    /// Asks the terminal; `true` when the size moved and the pane needs a fresh frame. The pixel
    /// size rides along: a dpi change moves it without moving a cell, and the strip follows it.
    fn sync_size(&mut self) -> io::Result<bool> {
        let pixels = (self.pixel_probe)().map(|(w, h)| (u32::from(w), u32::from(h)));
        let repixeled = pixels != self.ui.pixels;
        self.ui.pixels = pixels;
        match (self.probe)() {
            Some(size) if size != self.size => self.adopt_size(size).map(|()| true),
            _ => Ok(repixeled),
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
        self.resize = None;
        self.needs_clear = true;
        self.shown_is_final = false;
        self.fx = None;
        self.emit(&Event::Resize { cols: w, rows: h })
    }
}

fn protocol_setting_change(value: Option<Value>) -> io::Result<SettingsChange> {
    match value {
        Some(value) => Ok(SettingsChange::Set {
            value: serde_json::to_value(value).map_err(io::Error::other)?,
        }),
        None => Ok(SettingsChange::Remove),
    }
}

/// `model.ordered`: pinned first, then the rest, both in the order Lua sent them.
fn ordered_ids(model: &payload::SidebarModel) -> Vec<i64> {
    let mut ids: Vec<i64> = model
        .tabs
        .iter()
        .filter(|t| t.pinned)
        .map(|t| t.id)
        .collect();
    ids.extend(model.tabs.iter().filter(|t| !t.pinned).map(|t| t.id));
    ids
}

fn space_ids(model: &payload::SidebarModel) -> Vec<&str> {
    model.spaces.iter().map(|s| s.id.as_str()).collect()
}

fn knobs<'a>(
    cfg: &'a EngineConfig,
    model: &'a payload::SidebarModel,
    view: &'a vtabs_engine::scene::RenderInput,
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
        position: view.cfg.position,
        double_click_ms: cfg.double_click_ms,
        tear_off: cfg.tear_off,
        wheel: cfg.wheel,
        context: cfg.context,
        hover_mode: view.cfg.hover,
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

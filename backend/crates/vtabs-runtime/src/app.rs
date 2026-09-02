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
    ApplyMode, DocumentAction, DocumentEffect, RawSettings, SettingsDocument, Widget,
};
use vtabs_engine::settings::presentation::{
    PresentationField, PresentationGroup, PresentationLock, SettingsPresentation,
};
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
    Command, Event, Intent, Modifier, SettingsApplyMode, SettingsChange, SettingsPatch, v2,
};

use crate::cli::Cli;
use crate::input::Input;
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
    /// Negotiated per client; older plugins receive the centralized `do` downgrade.
    pub client_typed_intents: bool,
    /// Only clients advertising this can complete the generation-bound host-hook handshake.
    pub client_theme_hooks: bool,
    /// Only clients advertising this send full window facts and understand Rust's space results.
    pub client_spaces_policy: bool,
    /// Last generation/effective-theme pair reported to Lua. Legacy unversioned updates still
    /// dedupe equal answers, while every atomic generation gets an answer of its own.
    pub last_reported_theme: Option<(Option<u64>, Theme)>,
    /// Last Rust-computed traffic-light reserve reported to this Lua process.
    pub last_rail_reserve: Option<i64>,
    /// The server's own `wezterm cli`, where this pane has one to act for.
    pub cli: Option<Cli>,
    pub metrics: RuntimeMetrics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeMetrics {
    pub committed_generations: u64,
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

/// The four independently encoded sections that become visible together in atomic mode.
#[derive(Clone, Default)]
pub struct V2Sections {
    pub config: Option<EngineConfig>,
    /// Raw theme input. `resolved_theme` is the only theme rendering consumes.
    pub theme: Option<v2::ThemeMsg>,
    pub resolved_theme: Option<Theme>,
    /// Raw full-window facts and the last Rust-owned projection derived from them.
    pub spaces: Option<v2::SpacesMsg>,
    pub space_resolution: Option<v2::SpaceResolution>,
    /// Sidebar model only. A legacy protocol settings model is adapted into `settings` at ingress.
    pub model: Option<v2::ModelMsg>,
    pub settings: Option<SettingsPresentation>,
    pub settings_rev: Option<u64>,
    /// Present only for the Rust-owned raw settings projection. A legacy model has a presentation
    /// but no document, so it retains its historical intent round trip.
    pub settings_document: Option<SettingsDocument>,
    pub menu: Option<v2::MenuMsg>,
}

const SEEN_CONFIG: u8 = 1 << 0;
const SEEN_THEME: u8 = 1 << 1;
const SEEN_MODEL: u8 = 1 << 2;
const SEEN_MENU: u8 = 1 << 3;
const SEEN_SPACES: u8 = 1 << 4;
const HOOK_TIMEOUT: Duration = Duration::from_millis(500);
const HOOK_RETRIES: u8 = 1;

struct PendingSync {
    generation: u64,
    sections: V2Sections,
    seen: u8,
    valid: bool,
    /// Exact tab ids in the one route-hook batch awaiting a host answer.
    space_hook_requested: Option<Vec<i64>>,
    /// The base-theme hook follows space resolution and is the last publication barrier.
    hook_requested: bool,
    /// Exact request replayed once if the host callback/result delivery is lost.
    hook_event: Option<Event>,
    hook_deadline: Option<Instant>,
    hook_retries: u8,
}

impl PendingSync {
    fn awaiting_hook(&self) -> bool {
        self.space_hook_requested.is_some() || self.hook_requested
    }
}

/// Committed state plus, for atomic-capable clients, at most one unpublished generation.
#[derive(Default)]
pub struct V2State {
    committed: V2Sections,
    committed_generation: Option<u64>,
    pending: Option<PendingSync>,
    /// A rejected Begin still owns the untagged sections up to its matching Commit; quarantine that
    /// batch so it cannot spill into a newer pending generation.
    discarding_generation: Option<u64>,
    /// Once Begin has been accepted, bare sections cannot accidentally fall back to legacy mode.
    atomic: bool,
}

impl std::ops::Deref for V2State {
    type Target = V2Sections;

    fn deref(&self) -> &Self::Target {
        &self.committed
    }
}

impl std::ops::DerefMut for V2State {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.committed
    }
}

enum Applied {
    Continue,
    Repaint,
    Quit,
}

fn settings_input(msg: v2::SettingsMsg) -> Option<(u64, SettingsPresentation, SettingsDocument)> {
    let rev = msg.rev;
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
    Some((rev, presentation, document))
}

/// The sole adapter for the old pre-rendered settings DTO. It is never stored or consumed inside
/// the engine, and raw settings documents never make the reverse trip through protocol types.
fn legacy_settings(model: v2::SettingsModel) -> SettingsPresentation {
    let fields = model
        .fields
        .into_iter()
        .map(|field| PresentationField {
            path: SettingPath::from_dotted(&field.key),
            label: field.label,
            group: field.group,
            widget: Widget::from_legacy_name(&field.widget),
            value_text: field.value_text,
            changed: field.changed,
            locked: field
                .locked
                .map(|lock| PresentationLock { text: lock.text }),
            depth: usize::try_from(field.depth).unwrap_or_default(),
            help: field.help,
            editing: field.editing.map(|editing| editing.buffer),
            armed: field.armed,
        })
        .collect();
    let groups = model
        .groups
        .into_iter()
        .map(|group| PresentationGroup {
            id: group.id,
            label: group.label,
        })
        .collect();
    SettingsPresentation {
        fields,
        groups,
        caveat: model.caveat,
        version: model.version,
    }
}

fn adopt_model(sections: &mut V2Sections, msg: v2::ModelMsg) {
    let rev = msg.rev;
    match msg.screen {
        v2::ModelScreen::Sidebar(model) => {
            sections.model = Some(v2::ModelMsg {
                rev,
                screen: v2::ModelScreen::Sidebar(model),
            });
            sections.settings = None;
            sections.settings_rev = None;
        }
        v2::ModelScreen::Settings(model) => {
            sections.model = None;
            sections.settings = Some(legacy_settings(model));
            sections.settings_rev = Some(rev);
        }
    }
    sections.settings_document = None;
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
        let downgraded = match event {
            Event::Intent { intent } if !self.client_typed_intents => Some(intent.downgrade()),
            _ => None,
        };
        let event = downgraded.as_ref().unwrap_or(event);
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
        self.v2.config.is_some()
            && self.v2.resolved_theme.is_some()
            && (self.v2.model.is_some() || self.v2.settings.is_some())
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
                    self.invalidate_pending();
                    self.emit(&Event::Dropped { what, reason })?;
                }
            }
            Input::Key { name, mods, raw } => {
                if self.dressed() {
                    self.key(&name, mods, &raw)?;
                } else {
                    self.emit(&Event::key(name, mods, &raw))?;
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
            }
            #[cfg(test)]
            Input::Command(command) => return self.run(command),
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
        let resolved = match (self.menu_outcome(), self.v2.menu.as_ref()) {
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

    /// Runs settings semantics locally when this client supplied a canonical settings document.
    /// `false` leaves non-settings and legacy-model intents on their compatibility path to Lua.
    fn apply_document_intent(&mut self, intent: &Intent) -> io::Result<bool> {
        let Some(document) = self.v2.settings_document.as_ref() else {
            return Ok(false);
        };
        let settings_rev = self
            .v2
            .settings_rev
            .expect("a canonical settings document always has a source revision");
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
            .v2
            .settings_document
            .as_ref()
            .expect("document checked above")
            .clone();
        let effect = self
            .v2
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
                self.v2.settings_document = Some(previous_document);
                self.refresh_settings_presentation();
                self.emit(&Event::Dropped {
                    what: "settings",
                    reason: "size",
                })?;
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
                    settings_rev,
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
        let Some(document) = self.v2.settings_document.as_ref() else {
            return;
        };
        let Some(current) = self.v2.settings.as_ref() else {
            return;
        };
        self.v2.settings = Some(document.presentation(current.version.clone()));
    }

    /// The settings widget's input, from the same three messages the sidebar reads.
    fn settings_view(&self) -> Option<SettingsView<'_>> {
        let (cfg, theme, presentation) = (
            self.v2.config.as_ref()?,
            self.v2.resolved_theme.as_ref()?,
            self.v2.settings.as_ref()?,
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
        let Some(cfg) = self.v2.config.as_ref() else {
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
        let (Some(msg), Some(theme)) = (self.effective_menu(), self.v2.resolved_theme.as_ref())
        else {
            return Outcome::Closed;
        };
        menu::plan(&msg, &self.menu_ui, &self.menu_cfg(), theme, self.dims())
    }

    fn effective_menu(&self) -> Option<v2::MenuMsg> {
        let mut msg = self.v2.menu.clone()?;
        if msg.header.is_none()
            && let (Some(cfg), Some(model)) = (
                self.v2.config.as_ref(),
                self.v2.model.as_ref().and_then(v2::ModelMsg::sidebar),
            )
        {
            msg.header = vtabs_engine::enrich::menu_header(&msg, model, cfg);
        }
        Some(msg)
    }

    fn scene(&self) -> (Enriched, Outcome) {
        let (cfg, theme, model) = self.state();
        let mut e = enrich(cfg, theme, model, self.dims(), &self.ui);
        let effective_menu = self.effective_menu();
        let outcome = match effective_menu.as_ref() {
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

    fn state(&self) -> (&EngineConfig, &Theme, &v2::SidebarModel) {
        let model = self.v2.model.as_ref().expect("dressed");
        (
            self.v2.config.as_ref().expect("dressed"),
            self.v2.resolved_theme.as_ref().expect("dressed"),
            model.sidebar().expect("sidebar state"),
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
        let rev = self
            .v2
            .model
            .as_ref()
            .map(|model| model.rev)
            .or(self.v2.settings_rev)
            .unwrap_or(0);
        let (cols, rows) = self.size;
        format!(
            "paint {cols}x{rows} rev {rev} totals={{commits={},paints={},bytes={}}}",
            self.metrics.committed_generations,
            self.metrics.terminal_paints,
            self.metrics.paint_bytes,
        )
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

    pub fn next_hook_deadline(&self) -> Option<Instant> {
        self.v2
            .pending
            .as_ref()
            .and_then(|pending| pending.hook_deadline)
    }

    /// Replays one lost host-hook request, then commits a deterministic no-hook fallback. The Lua
    /// side caches answers per generation, so replay never runs user code twice.
    pub fn tick_hooks(&mut self, now: Instant) -> io::Result<()> {
        let due = self
            .v2
            .pending
            .as_ref()
            .is_some_and(|pending| pending.hook_deadline.is_some_and(|at| now >= at));
        if !due {
            return Ok(());
        }
        let retry = self.v2.pending.as_ref().is_some_and(|pending| {
            pending.hook_retries < HOOK_RETRIES && pending.hook_event.is_some()
        });
        if retry {
            let event = {
                let pending = self
                    .v2
                    .pending
                    .as_mut()
                    .expect("due hook has pending state");
                pending.hook_retries += 1;
                pending.hook_deadline = Some(now + HOOK_TIMEOUT);
                pending.hook_event.clone().expect("retry checked an event")
            };
            self.emit(&event)?;
            return Ok(());
        }
        let what = if self
            .v2
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
        let Some(generation) = self.v2.pending.as_ref().map(|pending| pending.generation) else {
            return Ok(Applied::Continue);
        };
        self.emit(&Event::Dropped { what, reason })?;
        let requested = self
            .v2
            .pending
            .as_ref()
            .and_then(|pending| pending.space_hook_requested.clone());
        if let Some(requested) = requested {
            let answers = requested
                .into_iter()
                .map(|tab_id| v2::SpaceRouteHookAnswer {
                    tab_id,
                    space: None,
                })
                .collect::<Vec<_>>();
            let plan = {
                let pending = self.v2.pending.as_ref().expect("matching pending sync");
                let (Some(input), Some(theme)) = (
                    pending.sections.spaces.as_ref(),
                    pending.sections.theme.as_ref(),
                ) else {
                    self.v2.pending.take();
                    return Ok(Applied::Continue);
                };
                spaces::plan(input, &theme.scheme, Some(&answers))
            };
            let SpacesPlan::Resolved(resolution) = plan else {
                self.v2.pending.take();
                return Ok(Applied::Continue);
            };
            let pending = self.v2.pending.as_mut().expect("matching pending sync");
            pending.space_hook_requested = None;
            pending.hook_event = None;
            pending.hook_deadline = None;
            pending.hook_retries = 0;
            Self::apply_space_resolution(&mut pending.sections, *resolution);
            return self.continue_sync(generation);
        }
        let Some(mut pending) = self.v2.pending.take() else {
            return Ok(Applied::Continue);
        };
        pending.hook_requested = false;
        pending.hook_event = None;
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
            .v2
            .model
            .as_ref()
            .and_then(v2::ModelMsg::sidebar)
            .and_then(|m| m.scroll)
            .is_some_and(|s| s.user && Some(s.top) == self.ui.scroll);
        if landed {
            self.ui.scroll = None;
        }
    }

    /// Transactional clients may only mutate the pending clone between Begin and Commit.
    fn stage(&mut self, seen: u8, update: impl FnOnce(&mut V2Sections)) {
        let Some(pending) = self.v2.pending.as_mut() else {
            return;
        };
        if pending.valid && !pending.awaiting_hook() {
            update(&mut pending.sections);
            pending.seen |= seen;
        }
    }

    fn ignores_atomic_section(&self) -> bool {
        self.v2.atomic
            && (self.v2.discarding_generation.is_some()
                || self
                    .v2
                    .pending
                    .as_ref()
                    .is_none_or(PendingSync::awaiting_hook))
    }

    fn invalidate_pending(&mut self) {
        if let Some(pending) = self.v2.pending.as_mut() {
            pending.valid = false;
        }
    }

    fn resolve_sections(sections: &V2Sections, hook: Option<&v2::ThemeOverrides>) -> Option<Theme> {
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
        Some(theme::resolve(
            &layered,
            &raw.scheme,
            raw.private
                .unwrap_or_else(|| sections.model.as_ref().is_some_and(v2::ModelMsg::private)),
        ))
    }

    fn apply_space_resolution(sections: &mut V2Sections, resolution: v2::SpaceResolution) {
        if let (Some(spaces), Some(model)) = (&sections.spaces, sections.model.as_mut()) {
            let visible = resolution
                .visible_tab_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if let v2::ModelScreen::Sidebar(sidebar) = &mut model.screen {
                sidebar.tabs = spaces
                    .tabs
                    .iter()
                    .filter(|fact| visible.contains(&fact.tab.id))
                    .map(|fact| fact.tab.clone())
                    .collect();
                sidebar.space = resolution.active.clone();
                sidebar.spaces = resolution
                    .summary
                    .iter()
                    .map(|space| v2::SpaceItem {
                        id: space.id.clone(),
                        name: space.name.clone(),
                        icon: space.icon.clone(),
                        unseen: space.unseen,
                    })
                    .collect();
            }
        }
        sections.space_resolution = Some(resolution);
    }

    fn valid_space_answer(requested: &[i64], routes: &[v2::SpaceRouteHookAnswer]) -> bool {
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

    fn valid_spaces_input(input: &v2::SpacesMsg) -> bool {
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

    fn report_committed_theme(&mut self, generation: Option<u64>) -> io::Result<()> {
        let Some(effective) = self.v2.resolved_theme.as_ref() else {
            return Ok(());
        };
        let reported = (generation, effective.clone());
        if self.last_reported_theme.as_ref() == Some(&reported) {
            return Ok(());
        }
        self.emit(&Event::ThemeResolved {
            generation,
            theme: reported.1.clone(),
        })?;
        self.last_reported_theme = Some(reported);
        Ok(())
    }

    /// Legacy sections still resolve in Rust; they simply cannot pause for an atomic host hook.
    fn refresh_legacy_theme(&mut self) -> io::Result<()> {
        self.v2.resolved_theme = Self::resolve_sections(&self.v2.committed, None);
        self.report_committed_theme(None)
    }

    fn publish_pending(
        &mut self,
        mut pending: PendingSync,
        hook: Option<&v2::ThemeOverrides>,
    ) -> io::Result<Applied> {
        let Some(effective) = Self::resolve_sections(&pending.sections, hook) else {
            return Ok(Applied::Continue);
        };
        pending.sections.resolved_theme = Some(effective);

        let previous_motion = self.hover_highlight();
        let generation = pending.generation;
        self.v2.committed = pending.sections;
        self.v2.committed_generation = Some(generation);
        self.metrics.committed_generations = self.metrics.committed_generations.saturating_add(1);
        let next_motion = self.hover_highlight();
        if previous_motion != next_motion {
            self.write(if next_motion {
                MOTION_ON.as_bytes()
            } else {
                MOTION_OFF.as_bytes()
            })?;
        }
        if let Some(menu) = self.v2.menu.as_ref() {
            self.menu_ui.adopt(menu);
        }
        if let Some(spaces) = self.v2.space_resolution.clone() {
            let window_id = self
                .v2
                .spaces
                .as_ref()
                .map(|input| input.window_id)
                .unwrap_or_default();
            self.emit(&Event::SpacesResolved {
                generation,
                window_id,
                resolution: Box::new(spaces),
            })?;
        }
        self.report_committed_theme(Some(generation))?;
        Ok(Applied::Repaint)
    }

    fn begin_sync(&mut self, generation: u64) {
        let stale_committed = self
            .v2
            .committed_generation
            .is_some_and(|current| generation <= current);
        let rejected_pending = self.v2.pending.as_ref().is_some_and(|pending| {
            generation < pending.generation
                || (generation == pending.generation && pending.awaiting_hook())
        });
        if stale_committed || rejected_pending {
            self.v2.discarding_generation = Some(generation);
            return;
        }
        // Lua retries the same generation when an atomic write reports failure. While still
        // building, replay starts from committed state again instead of keeping a partial prefix.
        self.v2.atomic = true;
        self.v2.discarding_generation = None;
        self.v2.pending = Some(PendingSync {
            generation,
            sections: self.v2.committed.clone(),
            seen: 0,
            valid: true,
            space_hook_requested: None,
            hook_requested: false,
            hook_event: None,
            hook_deadline: None,
            hook_retries: 0,
        });
    }

    fn continue_sync(&mut self, generation: u64) -> io::Result<Applied> {
        if self.client_spaces_policy {
            let needs_plan = self
                .v2
                .pending
                .as_ref()
                .is_some_and(|pending| pending.sections.space_resolution.is_none());
            if needs_plan {
                let plan = {
                    let pending = self.v2.pending.as_ref().expect("matching pending sync");
                    let Some(input) = pending.sections.spaces.as_ref() else {
                        self.v2.pending.take();
                        return Ok(Applied::Continue);
                    };
                    let Some(theme) = pending.sections.theme.as_ref() else {
                        self.v2.pending.take();
                        return Ok(Applied::Continue);
                    };
                    spaces::plan(input, &theme.scheme, None)
                };
                match plan {
                    SpacesPlan::NeedsHooks { tabs } => {
                        let window_id = self
                            .v2
                            .pending
                            .as_ref()
                            .and_then(|pending| pending.sections.spaces.as_ref())
                            .map(|input| input.window_id)
                            .unwrap_or_default();
                        let requested = tabs.iter().map(|tab| tab.tab_id).collect();
                        let event = Event::SpaceRouteHookRequest {
                            generation,
                            window_id,
                            tabs,
                        };
                        let pending = self.v2.pending.as_mut().expect("matching pending sync");
                        pending.space_hook_requested = Some(requested);
                        pending.hook_event = Some(event.clone());
                        pending.hook_deadline = Some(Instant::now() + HOOK_TIMEOUT);
                        pending.hook_retries = 0;
                        self.emit(&event)?;
                        return Ok(Applied::Continue);
                    }
                    SpacesPlan::Resolved(resolution) => {
                        let pending = self.v2.pending.as_mut().expect("matching pending sync");
                        Self::apply_space_resolution(&mut pending.sections, *resolution);
                    }
                }
            }
        }

        let pending = self.v2.pending.as_ref().expect("matching pending sync");
        if pending.hook_requested {
            return Ok(Applied::Continue);
        }
        let needs_hook = self.client_theme_hooks
            && pending
                .sections
                .theme
                .as_ref()
                .is_some_and(|theme| theme.hook);
        if needs_hook {
            let Some(base) = Self::resolve_sections(&pending.sections, None) else {
                self.v2.pending.take();
                return Ok(Applied::Continue);
            };
            let event = Event::ThemeHookRequest {
                generation,
                theme: base,
            };
            let pending = self.v2.pending.as_mut().expect("matching pending sync");
            pending.hook_requested = true;
            pending.hook_event = Some(event.clone());
            pending.hook_deadline = Some(Instant::now() + HOOK_TIMEOUT);
            pending.hook_retries = 0;
            self.emit(&event)?;
            return Ok(Applied::Continue);
        }
        let pending = self.v2.pending.take().expect("matching pending sync");
        self.publish_pending(pending, None)
    }

    fn commit_sync(&mut self, generation: u64) -> io::Result<Applied> {
        if let Some(discarded) = self.v2.discarding_generation {
            if discarded == generation {
                self.v2.discarding_generation = None;
            }
            return Ok(Applied::Continue);
        }
        if self
            .v2
            .committed_generation
            .is_some_and(|current| generation <= current)
        {
            return Ok(Applied::Continue);
        }
        let Some(pending) = self.v2.pending.as_ref() else {
            return Ok(Applied::Continue);
        };
        if pending.generation != generation {
            return Ok(Applied::Continue);
        }
        if !pending.valid
            || pending.seen == 0
            || pending.sections.config.is_none()
            || pending.sections.theme.is_none()
            || (self.client_spaces_policy && pending.sections.spaces.is_none())
            || (pending.sections.model.is_none() && pending.sections.settings.is_none())
        {
            self.v2.pending.take();
            return Ok(Applied::Continue);
        }
        if pending.awaiting_hook() {
            return Ok(Applied::Continue);
        }
        self.continue_sync(generation)
    }

    fn space_hook_result(
        &mut self,
        generation: u64,
        routes: Vec<v2::SpaceRouteHookAnswer>,
    ) -> io::Result<Applied> {
        let Some(requested) = self
            .v2
            .pending
            .as_ref()
            .filter(|pending| pending.generation == generation && pending.valid)
            .and_then(|pending| pending.space_hook_requested.clone())
        else {
            return Ok(Applied::Continue);
        };
        if !Self::valid_space_answer(&requested, &routes) {
            return self.fallback_pending_hook("space_route_hook_result", "invalid");
        }
        let plan = {
            let pending = self.v2.pending.as_ref().expect("matching pending sync");
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
        let pending = self.v2.pending.as_mut().expect("matching pending sync");
        pending.space_hook_requested = None;
        pending.hook_event = None;
        pending.hook_deadline = None;
        pending.hook_retries = 0;
        Self::apply_space_resolution(&mut pending.sections, *resolution);
        self.continue_sync(generation)
    }

    fn theme_hook_result(
        &mut self,
        generation: u64,
        overrides: v2::ThemeOverrides,
    ) -> io::Result<Applied> {
        let matches = self.v2.pending.as_ref().is_some_and(|pending| {
            pending.generation == generation && pending.valid && pending.hook_requested
        });
        if !matches {
            return Ok(Applied::Continue);
        }
        if !theme::valid_overrides(&overrides) {
            return self.fallback_pending_hook("theme_hook_result", "invalid");
        }
        let pending = self.v2.pending.take().expect("matching pending sync");
        self.publish_pending(pending, Some(&overrides))
    }

    fn reset_for_auth(&mut self) -> io::Result<()> {
        let motion_was_on = self.hover_highlight();
        self.v2 = V2State::default();
        self.ui = UiState::default();
        self.menu_ui = MenuState::default();
        self.settings_ui = SettingsUi::default();
        self.popover = None;
        self.noted_menu = None;
        self.hover_deadline = None;
        self.fx = None;
        self.last_rows = None;
        self.shown_is_final = false;
        self.needs_clear = false;
        self.client_typed_intents = false;
        self.client_theme_hooks = false;
        self.client_spaces_policy = false;
        self.last_reported_theme = None;
        self.last_rail_reserve = None;
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
            Command::Auth { token, caps } => {
                self.write(set_user_var(TOKEN_VAR, &token).as_bytes())?;
                let typed_intents = caps.iter().any(|cap| cap == "typed_intents");
                let theme_hooks = caps.iter().any(|cap| cap == "theme_hooks");
                let spaces_policy = caps.iter().any(|cap| cap == "spaces_policy");
                // A new token is a new Lua process: the mux kept this backend but the plugin lost
                // `store.proto`/`store.paints` with its old state, and only `ready` restores them.
                // Re-announcing on the same token would ping-pong, since Lua re-auths on ready.
                if self.token.as_deref() != Some(token.as_str()) {
                    self.token = Some(token);
                    self.reset_for_auth()?;
                    self.client_typed_intents = typed_intents;
                    self.client_theme_hooks = theme_hooks;
                    self.client_spaces_policy = spaces_policy;
                    self.emit(&Event::ready(self.size.0, self.size.1))?;
                } else {
                    self.client_typed_intents = typed_intents;
                    self.client_theme_hooks = theme_hooks;
                    self.client_spaces_policy = spaces_policy;
                }
            }
            Command::Begin { generation } => self.begin_sync(generation),
            Command::Commit { generation } => return self.commit_sync(generation),
            Command::ThemeHookResult {
                generation,
                overrides,
            } => return self.theme_hook_result(generation, *overrides),
            Command::SpaceRouteHookResult { generation, routes } => {
                return self.space_hook_result(generation, routes);
            }
            Command::Config(msg) => {
                if self.ignores_atomic_section() {
                    return Ok(Applied::Continue);
                }
                let Ok(msg) = EngineConfig::try_from(*msg) else {
                    self.invalidate_pending();
                    self.emit(&Event::Dropped {
                        what: "config",
                        reason: "invalid",
                    })?;
                    return Ok(if self.v2.atomic {
                        Applied::Continue
                    } else {
                        Applied::Repaint
                    });
                };
                if self.v2.atomic {
                    self.stage(SEEN_CONFIG, |sections| sections.config = Some(msg));
                    return Ok(Applied::Continue);
                }
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
                self.v2.config = Some(msg);
                return Ok(Applied::Repaint);
            }
            Command::Theme(msg) => {
                if self.ignores_atomic_section() {
                    return Ok(Applied::Continue);
                }
                if self.v2.atomic {
                    self.stage(SEEN_THEME, |sections| {
                        sections.theme = Some(*msg);
                        sections.resolved_theme = None;
                        // Automatic space accents are selected from this raw palette.
                        sections.space_resolution = None;
                    });
                    return Ok(Applied::Continue);
                }
                self.v2.theme = Some(*msg);
                self.refresh_legacy_theme()?;
                return Ok(Applied::Repaint);
            }
            Command::Spaces(msg) => {
                if self.ignores_atomic_section() {
                    return Ok(Applied::Continue);
                }
                if !self.v2.atomic || !self.client_spaces_policy {
                    self.invalidate_pending();
                    self.emit(&Event::Dropped {
                        what: "spaces",
                        reason: "unsupported",
                    })?;
                    return Ok(Applied::Continue);
                }
                if !Self::valid_spaces_input(&msg) {
                    self.invalidate_pending();
                    self.emit(&Event::Dropped {
                        what: "spaces",
                        reason: "bounds",
                    })?;
                    return Ok(Applied::Continue);
                }
                self.stage(SEEN_SPACES, |sections| {
                    sections.spaces = Some(*msg);
                    sections.space_resolution = None;
                });
                return Ok(Applied::Continue);
            }
            Command::Model(msg) => {
                if self.ignores_atomic_section() {
                    return Ok(Applied::Continue);
                }
                if msg.tab_count() > MODEL_MAX_TABS
                    || msg.field_count() > MODEL_MAX_FIELDS
                    || msg.space_count() > MODEL_MAX_SPACES
                {
                    self.invalidate_pending();
                    self.emit(&Event::Dropped {
                        what: "model",
                        reason: "bounds",
                    })?;
                    return Ok(if self.v2.atomic {
                        Applied::Continue
                    } else {
                        Applied::Repaint
                    });
                } else if self.v2.atomic {
                    self.stage(SEEN_MODEL, |sections| {
                        adopt_model(sections, *msg);
                        if let Some(resolution) = sections.space_resolution.clone() {
                            Self::apply_space_resolution(sections, resolution);
                        }
                    });
                    return Ok(Applied::Continue);
                } else {
                    adopt_model(&mut self.v2.committed, *msg);
                }
                self.refresh_legacy_theme()?;
                return Ok(Applied::Repaint);
            }
            Command::Settings(msg) => {
                if self.ignores_atomic_section() {
                    return Ok(Applied::Continue);
                }
                let Some((rev, presentation, document)) = settings_input(*msg) else {
                    self.invalidate_pending();
                    self.emit(&Event::Dropped {
                        what: "settings",
                        reason: "invalid",
                    })?;
                    return Ok(if self.v2.atomic {
                        Applied::Continue
                    } else {
                        Applied::Repaint
                    });
                };
                if presentation.fields.len() > MODEL_MAX_FIELDS {
                    self.invalidate_pending();
                    self.emit(&Event::Dropped {
                        what: "settings",
                        reason: "bounds",
                    })?;
                    return Ok(if self.v2.atomic {
                        Applied::Continue
                    } else {
                        Applied::Repaint
                    });
                } else if self.v2.atomic {
                    self.stage(SEEN_MODEL, |sections| {
                        sections.model = None;
                        sections.settings = Some(presentation);
                        sections.settings_rev = Some(rev);
                        sections.settings_document = Some(document);
                    });
                    return Ok(Applied::Continue);
                } else {
                    self.v2.model = None;
                    self.v2.settings = Some(presentation);
                    self.v2.settings_rev = Some(rev);
                    self.v2.settings_document = Some(document);
                }
                self.refresh_legacy_theme()?;
                return Ok(Applied::Repaint);
            }
            Command::Menu(msg) => {
                if self.ignores_atomic_section() {
                    return Ok(Applied::Continue);
                }
                if msg.items.len() > MENU_MAX_ITEMS {
                    self.invalidate_pending();
                    self.emit(&Event::Dropped {
                        what: "menu",
                        reason: "bounds",
                    })?;
                    return Ok(if self.v2.atomic {
                        Applied::Continue
                    } else {
                        Applied::Repaint
                    });
                } else if self.v2.atomic {
                    self.stage(SEEN_MENU, |sections| sections.menu = Some(*msg));
                    return Ok(Applied::Continue);
                } else {
                    self.menu_ui.adopt(&msg);
                    self.v2.menu = Some(*msg);
                }
                return Ok(Applied::Repaint);
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
            Command::Quit => return Ok(Applied::Quit),
        }
        Ok(Applied::Continue)
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

fn protocol_setting_change(value: Option<Value>) -> io::Result<SettingsChange> {
    match value {
        Some(value) => Ok(SettingsChange::Set {
            value: serde_json::to_value(value).map_err(io::Error::other)?,
        }),
        None => Ok(SettingsChange::Remove),
    }
}

/// `model.ordered`: pinned first, then the rest, both in the order Lua sent them.
fn ordered_ids(model: &v2::SidebarModel) -> Vec<i64> {
    let mut ids: Vec<i64> = model
        .tabs
        .iter()
        .filter(|t| t.pinned)
        .map(|t| t.id)
        .collect();
    ids.extend(model.tabs.iter().filter(|t| !t.pinned).map(|t| t.id));
    ids
}

fn space_ids(model: &v2::SidebarModel) -> Vec<&str> {
    model.spaces.iter().map(|s| s.id.as_str()).collect()
}

fn knobs<'a>(
    cfg: &'a EngineConfig,
    model: &'a v2::SidebarModel,
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

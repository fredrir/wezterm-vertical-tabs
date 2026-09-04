use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigMsg {
    #[serde(default)]
    pub rail_width: u32,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub icons: bool,
    #[serde(default)]
    pub icon_map: BTreeMap<String, String>,
    #[serde(default)]
    pub meta: Option<String>,
    /// Joins `proc` and the directory on the meta line; wire.lua has always sent it.
    #[serde(default)]
    pub meta_sep: Option<String>,
    #[serde(default)]
    pub glyphs: GlyphFlags,
    #[serde(default)]
    pub double_click_ms: u64,
    #[serde(default)]
    pub tear_off: bool,
    #[serde(default)]
    pub wheel: Option<String>,
    #[serde(default)]
    pub context: Option<ContextSpec>,
    #[serde(default)]
    pub hover_timeout_ms: u64,
    /// Off: no row lights under the pointer, and the runtime stops asking for motion reports.
    #[serde(default = "yes")]
    pub hover_highlight: bool,
    /// `cfg.ellipsis`: the menu's own truncation marker, which is not the `ellipsis` glyph.
    #[serde(default)]
    pub ellipsis: Option<String>,
    #[serde(default)]
    pub popover: Option<PopoverSection>,
    #[serde(default)]
    pub render: Option<RenderSection>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PopoverSection {
    /// `"auto"` or a column count, represented as the exact wire union.
    #[serde(default)]
    pub width: Option<PopoverWidth>,
    #[serde(default = "yes")]
    pub follow_pointer: bool,
    #[serde(default)]
    pub overflow: Option<String>,
}

impl PopoverSection {
    /// The width the user asked for, or None for `"auto"`.
    pub fn fixed_width(&self) -> Option<i64> {
        match self.width.as_ref() {
            Some(PopoverWidth::Fixed(width)) => Some(*width),
            Some(PopoverWidth::Auto(_)) | None => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PopoverWidth {
    Fixed(i64),
    Auto(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ContextSpec {
    Name(String),
    Enabled(bool),
}

impl Default for PopoverSection {
    fn default() -> Self {
        PopoverSection {
            width: None,
            follow_pointer: true,
            overflow: None,
        }
    }
}

fn yes() -> bool {
    true
}

/// The renderer-facing config surface, normalised by Lua before it crosses the wire.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSection {
    /// False when the meta line is off; the mode string ConfigMsg.meta carries what it shows.
    #[serde(default)]
    pub meta: bool,
    pub padding: PaddingSpec,
    #[serde(default)]
    pub frame: bool,
    #[serde(default)]
    pub tab_height: Option<String>,
    #[serde(default)]
    pub row_gap: i64,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(default)]
    pub pinned_style: Option<String>,
    #[serde(default)]
    pub close_button: Option<String>,
    #[serde(default)]
    pub show_index: bool,
    #[serde(default)]
    pub scroll_indicator: Option<String>,
    #[serde(default)]
    pub new_tab_button: bool,
    #[serde(default)]
    pub new_tab_label: Option<String>,
    #[serde(default)]
    pub hover: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaddingSpec {
    #[serde(default)]
    pub left: i64,
    #[serde(default)]
    pub right: i64,
    #[serde(default)]
    pub top: i64,
    #[serde(default)]
    pub bottom: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlyphFlags {
    #[serde(default)]
    pub custom_block: bool,
    #[serde(default)]
    pub east_asian_wide: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeMsg {
    #[serde(default)]
    pub scheme: Scheme,
    /// Raw user/space overrides. Rust is the only place that resolves these against the scheme.
    #[serde(default)]
    pub overrides: ThemeOverrides,
    /// Window-global private state, shared by sidebar and settings panes.
    pub private: bool,
    /// Ask the backend to round-trip its resolved base through `hooks.theme` before commit.
    #[serde(default)]
    pub hook: bool,
}

/// The complete theme override surface accepted at the wire boundary.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeOverrides {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub elevation: Option<f64>,
    pub accent: Option<String>,
    pub private_accent: Option<String>,
    pub hover_bg: Option<String>,
    pub hover_fg: Option<String>,
    pub active_bg: Option<String>,
    pub active_fg: Option<String>,
    pub focus_bg: Option<String>,
    pub meta_fg: Option<String>,
    pub dim: Option<String>,
    pub title_idle: Option<String>,
    pub title_active: Option<String>,
    pub pinned_fg: Option<String>,
    pub separator: Option<String>,
    pub border: Option<String>,
    pub border_idle: Option<String>,
    pub ghost_border_hover: Option<String>,
    pub new_tab_fg: Option<String>,
    pub close_fg: Option<String>,
    pub close_hover_fg: Option<String>,
    pub unseen_fg: Option<String>,
    pub drag_bg: Option<String>,
    pub drag_fg: Option<String>,
    pub scroll_fg: Option<String>,
    pub scroll_idle_fg: Option<String>,
    pub surface_raised: Option<String>,
    pub scrim: Option<f64>,
    pub disabled_fg: Option<String>,
    pub popover_sel_bg: Option<String>,
    pub popover_sel_fg: Option<String>,
    pub popover_sel_hint: Option<String>,
}

/// Machine-readable theme field metadata shared with the Lua bridge. Serialization tests compare
/// this manifest to the DTO so adding a field cannot silently skip validation or normalization.
pub const THEME_COLOR_FIELDS: &[&str] = &[
    "fg",
    "bg",
    "accent",
    "private_accent",
    "hover_bg",
    "hover_fg",
    "active_bg",
    "active_fg",
    "focus_bg",
    "meta_fg",
    "dim",
    "title_idle",
    "title_active",
    "pinned_fg",
    "separator",
    "border",
    "border_idle",
    "ghost_border_hover",
    "new_tab_fg",
    "close_fg",
    "close_hover_fg",
    "unseen_fg",
    "drag_bg",
    "drag_fg",
    "scroll_fg",
    "scroll_idle_fg",
    "surface_raised",
    "disabled_fg",
    "popover_sel_bg",
    "popover_sel_fg",
    "popover_sel_hint",
];
pub const THEME_FRACTION_FIELDS: &[&str] = &["elevation", "scrim"];

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scheme {
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub foreground: Option<String>,
    #[serde(default)]
    pub cursor_bg: Option<String>,
    #[serde(default)]
    pub active_tab_bg: Option<String>,
    #[serde(default)]
    pub ansi: Vec<String>,
    /// Bright ANSI colours are kept separate because automatic space accents preserve the host
    /// palette's normal-then-bright slot order.
    #[serde(default)]
    pub brights: Vec<String>,
}

/// The complete effective theme produced by Rust. RGB values deliberately stay triples: the hook
/// receives the same representation the renderer consumes, without a second colour parser.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedTheme {
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub title_idle: Color,
    pub meta_fg: Color,
    pub active_bg: Color,
    pub active_fg: Color,
    pub hover_bg: Color,
    pub hover_fg: Color,
    pub focus_bg: Color,
    pub pinned_fg: Color,
    pub separator: Color,
    pub border: Color,
    pub border_idle: Color,
    pub ghost_border_hover: Color,
    pub new_tab_fg: Color,
    pub close_fg: Color,
    pub close_hover_fg: Color,
    pub unseen_fg: Color,
    pub private_accent: Color,
    pub drag_bg: Color,
    pub drag_fg: Color,
    pub scroll_fg: Color,
    pub scroll_idle_fg: Color,
    pub title_active: Color,
    pub title_active_contrast: f64,
    pub content_bg: Color,
    pub surface_raised: Color,
    pub scrim: f64,
    pub disabled_fg: Color,
    pub popover_sel_bg: Color,
    pub popover_sel_fg: Color,
    pub popover_sel_hint: Color,
}

/// Per-pane UI state supplied by Lua. The window tab census and space state arrive in `SpacesMsg`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMsg {
    /// The window's collapse mode is Lua state; the pane only knows its width.
    #[serde(default)]
    pub rail: bool,
    #[serde(default)]
    pub active: Option<i64>,
    #[serde(default)]
    pub focus: Option<FocusState>,
    #[serde(default)]
    pub scroll: Option<ScrollState>,
    #[serde(default)]
    pub drag: Option<DragState>,
    #[serde(default)]
    pub strip: Option<StripState>,
    #[serde(default)]
    pub footer: Vec<FooterItem>,
}

/// Renderer model assembled by Rust from `ModelMsg`, `SpacesMsg`, and `ThemeMsg`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SidebarModel {
    /// The window's collapse mode is Lua state; the pane only knows its width.
    pub rail: bool,
    pub active: Option<i64>,
    pub focus: Option<FocusState>,
    pub scroll: Option<ScrollState>,
    pub drag: Option<DragState>,
    pub strip: Option<StripState>,
    pub footer: Vec<FooterItem>,
    pub tabs: Vec<TabRecord>,
    /// Rust copies the authoritative value from `ThemeMsg` while assembling this model.
    pub private: bool,
    /// The active space's id, Lua's to decide; each entry's highlight is derived from it here.
    pub space: Option<String>,
    pub spaces: Vec<SpaceItem>,
}

/// The host facts Rust needs to own the live settings document. Values are deliberately JSON at
/// this transport boundary; `vtabs-engine` converts them to its canonical typed value tree and is
/// the sole owner of schema validation and mutation semantics.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsMsg {
    pub values: serde_json::Value,
    #[serde(default)]
    pub explicit: Vec<Vec<String>>,
    #[serde(default)]
    pub host_values: Vec<String>,
    #[serde(default)]
    pub opaque: Vec<Vec<String>>,
    #[serde(default)]
    pub key_defaults: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub is_macos: bool,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusState {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub index: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollState {
    #[serde(default)]
    pub top: i64,
    #[serde(default)]
    pub user: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DragState {
    pub id: i64,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub slot: Option<i64>,
    #[serde(default)]
    pub outside: bool,
    pub origin: DragOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DragOrigin {
    pub x: i64,
    pub y: i64,
    pub at: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StripState {
    /// The window's dpi, the one measurement a pane cannot take for itself.
    #[serde(default)]
    pub dpi: Option<f64>,
    #[serde(default)]
    pub chrome: Option<ChromeFacts>,
    #[serde(default)]
    pub buttons: Vec<StripButton>,
}

/// Host chrome facts Rust combines with pane metrics and render configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeFacts {
    #[serde(default)]
    pub is_mac: bool,
    #[serde(default)]
    pub integrated_buttons: bool,
    #[serde(default)]
    pub native_button_style: bool,
    #[serde(default)]
    pub preview: bool,
    #[serde(default)]
    pub is_full_screen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StripButton {
    pub id: String,
    #[serde(default)]
    pub icon: Option<String>,
}

/// One switcher entry; the tab count travels on the menu's `spaces` level, not here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceItem {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub unseen: bool,
}

/// One complete, immutable window census for Rust's stateless spaces planner. `definitions` stays
/// raw at the protocol boundary so one malformed entry or field can be dropped with a precise
/// warning while the other entries remain usable.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpacesMsg {
    pub window_id: i64,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub hook: bool,
    #[serde(default)]
    pub definitions: Vec<serde_json::Value>,
    #[serde(default)]
    pub tabs: Vec<SpaceTabFact>,
    #[serde(default)]
    pub active_tab: Option<i64>,
    #[serde(default)]
    pub active_space: Option<String>,
    /// The active `(tab, space)` pair observed on the previous poll. It makes following a trigger,
    /// rather than an invariant, so a deliberately selected empty space remains selected.
    #[serde(default)]
    pub follow: Option<SpaceFollow>,
    #[serde(default)]
    pub last_tabs: Vec<SpaceLastTab>,
    /// Dynamic ids and their first-sight order are explicit input, keeping the engine pure and the
    /// eventual persistence/ownership choice outside this protocol.
    #[serde(default)]
    pub dynamics: Vec<DynamicSpace>,
}

/// Renderer data plus the routing state that the existing sidebar model deliberately omits.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceTabFact {
    pub tab: TabRecord,
    pub remote: bool,
    pub space: Option<String>,
    pub manual: bool,
    /// Opaque planner-produced stamp accepted back on the next poll.
    pub fingerprint: Option<String>,
}

/// Strict wire representation for the deliberately flat tab census entry. A derived flattened
/// `TabRecord` cannot be combined with `deny_unknown_fields`, so this private DTO enumerates the
/// complete accepted key set before rebuilding the public renderer-plus-routing shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceTabFactWire {
    id: i64,
    index: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    pane_title: Option<String>,
    #[serde(default, rename = "override")]
    override_title: Option<String>,
    #[serde(default)]
    proc: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    unseen: bool,
    #[serde(default)]
    settings: bool,
    #[serde(default)]
    remote: bool,
    #[serde(default)]
    space: Option<String>,
    #[serde(default)]
    manual: bool,
    #[serde(default)]
    fingerprint: Option<String>,
}

impl<'de> Deserialize<'de> for SpaceTabFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SpaceTabFactWire::deserialize(deserializer)?;
        Ok(Self {
            tab: TabRecord {
                id: wire.id,
                index: wire.index,
                title: wire.title,
                pane_title: wire.pane_title,
                override_title: wire.override_title,
                proc: wire.proc,
                icon: wire.icon,
                cwd: wire.cwd,
                host: wire.host,
                user: wire.user,
                domain: wire.domain,
                pinned: wire.pinned,
                unseen: wire.unseen,
                settings: wire.settings,
            },
            remote: wire.remote,
            space: wire.space,
            manual: wire.manual,
            fingerprint: wire.fingerprint,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceFollow {
    pub tab_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceLastTab {
    pub space_id: String,
    pub tab_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicSpace {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    pub seq: u64,
}

/// The exact Lua hook argument, emitted only for tabs whose routing fingerprint changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpaceRouteHookFact {
    pub tab_id: i64,
    pub window_id: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    pub remote: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
    pub manual: bool,
}

/// An explicit row with no `space` is the hook's nil/no-op answer; omitting the row means the
/// batch is incomplete and must not be published.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceRouteHookAnswer {
    pub tab_id: i64,
    #[serde(default)]
    pub space: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpaceAssignment {
    pub tab_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
    pub manual: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpaceSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub count: usize,
    pub unseen: bool,
}

/// Stable machine-readable warning codes let Lua retain its warn-once presentation without
/// parsing English emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpaceWarning {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// A complete planner result. The same value supplies host state, the renderer, and active theme.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpaceResolution {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    pub assignments: Vec<SpaceAssignment>,
    pub dynamics: Vec<DynamicSpace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow: Option<SpaceFollow>,
    pub last_tabs: Vec<SpaceLastTab>,
    pub summary: Vec<SpaceSummary>,
    pub visible_tab_ids: Vec<i64>,
    pub theme_overrides: ThemeOverrides,
    pub warnings: Vec<SpaceWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FooterItem {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub fg: Option<Color>,
    #[serde(default)]
    pub bg: Option<Color>,
    #[serde(default)]
    pub icon_fg: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabRecord {
    pub id: i64,
    pub index: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pane_title: Option<String>,
    #[serde(default, rename = "override")]
    pub override_title: Option<String>,
    #[serde(default)]
    pub proc: Option<String>,
    /// A user `icon_map` match resolved by Lua, where the user's native pattern semantics live.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub unseen: bool,
    #[serde(default)]
    pub settings: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuMsg {
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub selected: Option<i64>,
    #[serde(default)]
    pub anchor: Option<MenuAnchor>,
    #[serde(default)]
    pub target: Option<i64>,
    /// Tab whose Rust-resolved title/meta heads this level. Usually `target`; confirmation may
    /// name the first tab that will actually close instead.
    #[serde(default)]
    pub subject: Option<i64>,
    /// Tabs affected by a confirmation, including `subject`.
    #[serde(default)]
    pub victims: Option<usize>,
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

/// `col` is absent when the menu was opened from a key binding, which knows no column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuAnchor {
    #[serde(default)]
    pub row: i64,
    #[serde(default)]
    pub col: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub danger: bool,
    #[serde(default)]
    pub confirm: Option<MenuConfirm>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuConfirm {
    pub q: String,
    pub yes: String,
    pub no: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FxMsg {
    pub phase: String,
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoticeMsg {
    #[serde(default)]
    pub level: Option<String>,
    pub text: String,
}

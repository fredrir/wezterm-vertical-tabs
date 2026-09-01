//! v2 command payloads (§2.3): idempotent, full within their domain, JSON lines on the pty.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConfigMsg {
    pub rev: u64,
    pub desired_width: u32,
    #[serde(default)]
    pub rail_width: u32,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub collapsed: Option<String>,
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
    pub unseen: bool,
    #[serde(default)]
    pub glyphs: GlyphFlags,
    #[serde(default)]
    pub animate: bool,
    #[serde(default)]
    pub double_click_ms: u64,
    #[serde(default)]
    pub tear_off: bool,
    #[serde(default)]
    pub wheel: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub hover_timeout_ms: u64,
    /// `cfg.ellipsis`: the menu's own truncation marker, which is not the `ellipsis` glyph.
    #[serde(default)]
    pub ellipsis: Option<String>,
    #[serde(default)]
    pub popover: Option<PopoverSection>,
    #[serde(default)]
    pub render: Option<RenderSection>,
    #[serde(default)]
    pub mac: Option<MacFacts>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PopoverSection {
    /// `"auto"` or a column count, which is why this stays untyped on the wire.
    #[serde(default)]
    pub width: Option<serde_json::Value>,
    #[serde(default = "yes")]
    pub follow_pointer: bool,
    #[serde(default)]
    pub overflow: Option<String>,
}

impl PopoverSection {
    /// The width the user asked for, or None for `"auto"`.
    pub fn fixed_width(&self) -> Option<i64> {
        self.width.as_ref().and_then(serde_json::Value::as_i64)
    }
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

/// The renderer-facing config surface, normalised by Lua exactly as the scene fixtures are.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
pub struct MacFacts {
    #[serde(default)]
    pub integrated_buttons: bool,
    #[serde(default)]
    pub native_button_style: bool,
    #[serde(default)]
    pub preview: bool,
    #[serde(default)]
    pub is_full_screen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct GlyphFlags {
    #[serde(default)]
    pub custom_block: bool,
    #[serde(default)]
    pub east_asian_wide: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ThemeMsg {
    pub rev: u64,
    #[serde(default)]
    pub scheme: Scheme,
    /// `hooks.theme` output, pre-resolved by Lua to hex.
    #[serde(default)]
    pub overrides: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub elevation: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Scheme {
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub foreground: Option<String>,
    #[serde(default)]
    pub cursor_bg: Option<String>,
    #[serde(default)]
    pub selection_bg: Option<String>,
    #[serde(default)]
    pub active_tab_bg: Option<String>,
    #[serde(default)]
    pub ansi: Vec<String>,
    #[serde(default)]
    pub brights: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelMsg {
    pub rev: u64,
    #[serde(default)]
    pub screen: Option<String>,
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
    #[serde(default)]
    pub tabs: Vec<TabRecord>,
    #[serde(default)]
    pub private: bool,
    /// `screen == "settings"` only: the form, in order, with everything a row shows pre-rendered.
    #[serde(default)]
    pub fields: Vec<SettingsField>,
    #[serde(default)]
    pub groups: Vec<SettingsGroup>,
    #[serde(default)]
    pub caveat: Option<Vec<String>>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub preview: Option<SettingsPreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SettingsGroup {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct SettingsLock {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct SettingsEditing {
    #[serde(default)]
    pub buffer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SettingsField {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub widget: String,
    #[serde(default)]
    pub value_text: String,
    #[serde(default)]
    pub changed: bool,
    #[serde(default)]
    pub locked: Option<SettingsLock>,
    #[serde(default)]
    pub depth: i64,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub editing: Option<SettingsEditing>,
    #[serde(default)]
    pub armed: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SettingsPreview {
    pub render: RenderSection,
    #[serde(default)]
    pub icons: bool,
    #[serde(default)]
    pub tabs: Vec<PreviewTab>,
    /// `layout.resolved_actions(merged)`; empty falls back to the shipped default cluster.
    #[serde(default)]
    pub strip: Vec<StripButton>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PreviewTab {
    pub id: i64,
    pub index: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub meta: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub icon: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct FocusState {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub index: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct ScrollState {
    #[serde(default)]
    pub top: i64,
    #[serde(default)]
    pub user: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct DragState {
    pub id: i64,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub slot: Option<i64>,
    #[serde(default)]
    pub outside: bool,
    /// The press point; the press may have happened in another backend process (§1.4).
    pub origin: DragOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct DragOrigin {
    pub x: i64,
    pub y: i64,
    pub at: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct StripState {
    #[serde(default)]
    pub rows: i64,
    #[serde(default)]
    pub cols: i64,
    #[serde(default)]
    pub toggle_row: Option<i64>,
    #[serde(default)]
    pub cell_w: Option<f64>,
    #[serde(default)]
    pub buttons: Vec<StripButton>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StripButton {
    pub id: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct FooterItem {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub fg: Option<[u8; 3]>,
    #[serde(default)]
    pub bg: Option<[u8; 3]>,
    #[serde(default)]
    pub icon_fg: Option<[u8; 3]>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
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
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub panes: i64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub unseen: bool,
    #[serde(default)]
    pub zoomed: bool,
    #[serde(default)]
    pub settings: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MenuMsg {
    pub rev: u64,
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
    #[serde(default)]
    pub header: Option<MenuHeader>,
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

/// `col` is absent when the menu was opened from a key binding, which knows no column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct MenuAnchor {
    #[serde(default)]
    pub row: i64,
    #[serde(default)]
    pub col: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct MenuHeader {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub meta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
pub struct MenuConfirm {
    pub q: String,
    pub yes: String,
    pub no: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FxMsg {
    pub phase: String,
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NoticeMsg {
    #[serde(default)]
    pub level: Option<String>,
    pub text: String,
}

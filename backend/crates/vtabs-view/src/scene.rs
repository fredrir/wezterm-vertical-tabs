//! The renderer's input: the v1 view shape, with Lua-owned resolution (buttons, hooks, theme)
//! already applied. Scene fixtures under plugin/tests/golden/scenes/ deserialize into this.

use std::collections::BTreeMap;

use serde::Deserialize;

pub type Rgb = [u8; 3];

#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub tab_id: i64,
    pub index: i64,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub is_private: bool,
    pub title: String,
    #[serde(default)]
    pub meta: Option<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub has_unseen: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Padding {
    #[serde(default)]
    pub left: i64,
    #[serde(default)]
    pub right: i64,
    #[serde(default)]
    pub top: i64,
    #[serde(default)]
    pub bottom: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenderCfg {
    pub padding: Padding,
    #[serde(default)]
    pub frame: bool,
    pub position: String,
    #[serde(default)]
    pub new_tab_button: bool,
    #[serde(default)]
    pub new_tab_label: String,
    #[serde(default)]
    pub row_gap: i64,
    pub separator: String,
    pub tab_height: String,
    /// False when the meta line is disabled (`cfg.meta == false` in Lua).
    #[serde(default = "yes")]
    pub meta: bool,
    #[serde(default)]
    pub meta_sep: Option<String>,
    #[serde(default)]
    pub show_index: bool,
    #[serde(default)]
    pub icons: bool,
    pub close_button: String,
    pub hover: String,
    pub pinned_style: String,
    pub scroll_indicator: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Toggle {
    pub row: i64,
    #[serde(default)]
    pub x: i64,
    #[serde(default)]
    pub x1: i64,
    #[serde(default)]
    pub x2: i64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Strip {
    #[serde(default)]
    pub rows: i64,
    #[serde(default)]
    pub cols: i64,
    #[serde(default)]
    pub toggle_row: Option<i64>,
    #[serde(default)]
    pub cell_w: Option<f64>,
    #[serde(default)]
    pub toggle: Option<Toggle>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripButton {
    pub id: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Hover {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Drag {
    pub tab_id: i64,
    #[serde(default)]
    pub over_index: Option<i64>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub outside: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FooterEntry {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub fg: Option<Rgb>,
    #[serde(default)]
    pub bg: Option<Rgb>,
    #[serde(default)]
    pub icon_fg: Option<Rgb>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PopSpan {
    #[serde(default = "one")]
    pub x: i64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub fg: Option<Rgb>,
    #[serde(default)]
    pub bold: bool,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PopRow {
    #[serde(default)]
    pub bg: Option<Rgb>,
    #[serde(default)]
    pub fg: Option<Rgb>,
    #[serde(default)]
    pub spans: Vec<PopSpan>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PopoverRect {
    #[serde(default = "one")]
    pub x: i64,
    #[serde(default = "one")]
    pub y: i64,
    #[serde(default)]
    pub w: Option<i64>,
    #[serde(default)]
    pub h: i64,
    #[serde(default)]
    pub scrim: f64,
    #[serde(default)]
    pub bg: Option<Rgb>,
    #[serde(default)]
    pub rows: Vec<PopRow>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenderInput {
    pub cols: i64,
    pub rows: i64,
    #[serde(default)]
    pub rail: bool,
    pub items: Vec<Item>,
    pub theme: vtabs_theme::Theme,
    pub cfg: RenderCfg,
    pub glyphs: BTreeMap<String, String>,
    #[serde(default)]
    pub strip: Option<Strip>,
    #[serde(default)]
    pub strip_buttons: Vec<StripButton>,
    #[serde(default)]
    pub hover: Option<Hover>,
    #[serde(default)]
    pub drag: Option<Drag>,
    #[serde(default)]
    pub scroll: i64,
    #[serde(default)]
    pub focus_index: Option<i64>,
    #[serde(default)]
    pub ensure_visible: Option<i64>,
    #[serde(default)]
    pub footer: Vec<FooterEntry>,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub user_scrolled: bool,
    #[serde(default)]
    pub popover: Option<PopoverRect>,
}

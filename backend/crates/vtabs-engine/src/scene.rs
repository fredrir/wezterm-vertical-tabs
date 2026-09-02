use std::collections::BTreeMap;

use crate::color::Color;
use crate::config::RenderConfig;
pub use crate::config::{Padding, RenderConfig as RenderCfg};
pub use vtabs_protocol::v2::{FooterItem as FooterEntry, StripButton};

#[derive(Debug, Clone)]
pub struct Item {
    pub tab_id: i64,
    pub index: i64,
    pub is_active: bool,
    pub is_pinned: bool,
    pub is_private: bool,
    pub title: String,
    pub meta: Option<String>,
    pub icon: String,
    pub has_unseen: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Toggle {
    pub row: i64,
    pub x: i64,
    pub x1: i64,
    pub x2: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct Strip {
    pub rows: i64,
    pub cols: i64,
    pub toggle_row: Option<i64>,
    pub cell_w: Option<f64>,
    pub toggle: Option<Toggle>,
}

#[derive(Debug, Clone, Copy)]
pub struct Hover {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub tab_id: i64,
    pub over_index: Option<i64>,
    pub active: bool,
    pub outside: bool,
}

/// One entry of the space switcher at the foot; `is_active` is derived from `model.space`.
#[derive(Debug, Clone)]
pub struct SpaceEntry {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub is_active: bool,
    pub has_unseen: bool,
}

#[derive(Debug, Clone)]
pub struct PopSpan {
    pub x: i64,
    pub text: String,
    pub fg: Option<Color>,
    pub bold: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PopRow {
    pub bg: Option<Color>,
    pub fg: Option<Color>,
    pub spans: Vec<PopSpan>,
}

#[derive(Debug, Clone)]
pub struct PopoverRect {
    pub x: i64,
    pub y: i64,
    pub w: Option<i64>,
    pub h: i64,
    pub scrim: f64,
    pub bg: Option<Color>,
    pub rows: Vec<PopRow>,
}

#[derive(Debug, Clone)]
pub struct RenderInput {
    pub cols: i64,
    pub rows: i64,
    pub rail: bool,
    pub items: Vec<Item>,
    pub theme: crate::theme::Theme,
    pub cfg: RenderConfig,
    pub glyphs: BTreeMap<String, String>,
    pub strip: Option<Strip>,
    pub strip_buttons: Vec<StripButton>,
    pub hover: Option<Hover>,
    pub drag: Option<Drag>,
    pub scroll: i64,
    pub focus_index: Option<i64>,
    pub ensure_visible: Option<i64>,
    pub footer: Vec<FooterEntry>,
    pub spaces: Vec<SpaceEntry>,
    pub private: bool,
    pub user_scrolled: bool,
    pub popover: Option<PopoverRect>,
}

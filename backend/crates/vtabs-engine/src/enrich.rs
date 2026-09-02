//! The v2 command state plus this pane's own dimensions and pointer state, composed into the
//! renderer's input. This is the single owner of title/meta fallback and the resolution formerly
//! done per window in Lua: icons, glyphs, theme and the strip's geometry.

use std::collections::BTreeMap;

use crate::config::{EngineConfig, MetaMode};
use crate::geom::{Dims, StripOpts, strip_geometry};
use crate::theme::Theme;
use crate::{basename, icons, sanitize, ui::UiState};
use vtabs_protocol::v2::{MenuHeader, MenuMsg, SidebarModel, SpaceItem, TabRecord, ThemeMsg};

use crate::glyphs;
use crate::scene::{Drag, Hover, Item, RenderInput, SpaceEntry, Strip};

const SHELLS: &[&str] = &[
    "bash",
    "cmd.exe",
    "fish",
    "nu",
    "powershell.exe",
    "pwsh.exe",
    "sh",
    "zsh",
];
const REMOTE: &[&str] = &["mosh", "mosh-client", "ssh", "ssh.exe"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopoverHits {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    /// One entry per row, top to bottom: the row's id and whether it refuses clicks.
    pub rows: Vec<(Option<String>, bool)>,
}

impl PopoverHits {
    /// The row under `y`, or None when `y` is off the rect entirely (a scrim row).
    pub fn row_at(&self, y: i64) -> Option<&(Option<String>, bool)> {
        let i = y - self.y;
        if i >= 0 && i < self.rows.len() as i64 {
            self.rows.get(i as usize)
        } else {
            None
        }
    }

    /// `hit.in_card` for the rect: the columns the menu actually owns.
    pub fn inside(&self, x: i64) -> bool {
        x >= self.x && x < self.x + self.w
    }

    pub fn covers(&self, y: i64) -> bool {
        y >= self.y && y < self.y + self.h
    }
}

pub struct Enriched {
    pub view: RenderInput,
    pub popover: Option<PopoverHits>,
    /// Rust's computed traffic-light reserve, reported to Lua only as a host width effect.
    pub rail_reserve: Option<i64>,
}

fn some(s: Option<&String>) -> Option<&str> {
    s.map(String::as_str).filter(|v| !v.is_empty())
}

/// The separator only earns its place between two present parts.
fn join(prefix: Option<&str>, tail: Option<&str>, sep: &str) -> Option<String> {
    match (prefix, tail) {
        (Some(a), Some(b)) => Some(format!("{a}{sep}{b}")),
        (a, b) => a.or(b).map(str::to_string),
    }
}

/// The card's second line. `cwd` arrives already tilde'd from Lua.
fn meta_from(tab: &TabRecord, mode: MetaMode, sep: &str) -> Option<String> {
    if mode == MetaMode::Off {
        return None;
    }
    let dir = some(tab.cwd.as_ref());
    let process = some(tab.proc.as_ref());
    if mode == MetaMode::Cwd {
        return dir.map(str::to_string);
    }
    if mode == MetaMode::Process {
        return process.map(str::to_string);
    }
    let Some(process) = process else {
        // A mux pane reports no process, so name where it is instead.
        let domain = some(tab.domain.as_ref()).filter(|d| *d != "local");
        return join(domain, dir, sep);
    };
    if REMOTE.contains(&process) {
        let Some(host) = some(tab.host.as_ref()) else {
            return Some(process.to_string());
        };
        return Some(match some(tab.user.as_ref()) {
            Some(user) => format!("{user}@{host}"),
            None => host.to_string(),
        });
    }
    if SHELLS.contains(&process) {
        return dir.map(str::to_string);
    }
    join(Some(process), dir.map(basename), sep)
}

/// `override → title → pane_title → "tab N"`; Lua sends the three sources raw so Rust owns this.
pub(crate) fn title_of(tab: &TabRecord) -> String {
    some(tab.override_title.as_ref())
        .or(Some(tab.title.as_str()).filter(|t| !t.is_empty()))
        .or(some(tab.pane_title.as_ref()))
        .map(str::to_string)
        .unwrap_or_else(|| format!("tab {}", tab.id))
}

/// Derives plugin menu headers from the same raw tab record as the card. A supplied header remains
/// the legacy escape hatch; current Lua sends only ids/counts and owns no title/meta fallback.
pub fn menu_header(msg: &MenuMsg, model: &SidebarModel, cfg: &EngineConfig) -> Option<MenuHeader> {
    if let Some(header) = &msg.header {
        return Some(header.clone());
    }
    let tab = msg
        .subject
        .or(msg.target)
        .and_then(|id| model.tabs.iter().find(|tab| tab.id == id));
    let title = tab.map(title_of).unwrap_or_else(|| "tab".to_owned());
    match msg.level.as_deref() {
        Some("confirm") => {
            let victims = msg.victims.unwrap_or(1).max(1);
            Some(MenuHeader {
                title: format!("Close {title}?"),
                meta: (victims > 1).then(|| {
                    format!(
                        "and {} other{}",
                        victims - 1,
                        if victims == 2 { "" } else { "s" }
                    )
                }),
            })
        }
        Some("rename") => Some(MenuHeader {
            title: "Rename tab".to_owned(),
            meta: None,
        }),
        Some("spaces") => Some(MenuHeader {
            title: "Move to space".to_owned(),
            meta: Some(title),
        }),
        _ => Some(MenuHeader {
            title,
            meta: tab.and_then(|tab| {
                meta_from(
                    tab,
                    cfg.meta_mode,
                    cfg.render.meta_sep.as_deref().unwrap_or(" "),
                )
            }),
        }),
    }
}

/// The switcher shows one glyph per space: the icon as sent, else the name's initial, else a dot.
fn space_icon(space: &SpaceItem) -> String {
    let icon = sanitize(space.icon.as_deref().unwrap_or("").as_bytes());
    if !icon.trim().is_empty() {
        return icon;
    }
    sanitize(space.name.as_bytes())
        .chars()
        .find(|c| !c.is_whitespace())
        .map(|c| c.to_string())
        .unwrap_or_else(|| "·".to_string())
}

/// The window's resolved theme, so a caller outside the frame — the menu — shares one answer.
pub fn theme_of(msg: &ThemeMsg, private: bool) -> crate::theme::Theme {
    crate::theme::resolve(&msg.overrides, &msg.scheme, private)
}

/// The sole strip geometry calculation, from raw WezTerm pane metrics and host chrome facts.
fn strip_of(
    cfg: &EngineConfig,
    model: &SidebarModel,
    cols: i64,
    rows: i64,
) -> (Strip, Option<i64>) {
    let sent = model.strip.as_ref();
    let metrics_fact = sent.and_then(|strip| strip.metrics);
    let chrome_fact = sent.and_then(|strip| strip.chrome);
    let metrics = metrics_fact.unwrap_or_default();
    let chrome = chrome_fact.unwrap_or_default();
    let position_left = !cfg.render.position.is_right();
    let reserve_disabled = !position_left
        || chrome_fact.is_some_and(|facts| {
            !(facts.is_mac || facts.preview)
                || !facts.integrated_buttons
                || !facts.native_button_style
                || facts.is_full_screen
        });
    let toggle_button = sent.is_some_and(|s| s.buttons.iter().any(|b| b.id == "toggle"));
    let g = strip_geometry(
        Dims {
            cols: if metrics.cols > 0 {
                metrics.cols
            } else {
                cols.max(0) as u32
            },
            viewport_rows: if metrics.viewport_rows > 0 {
                metrics.viewport_rows
            } else {
                rows.max(0) as u32
            },
            pixel_width: metrics.pixel_width,
            pixel_height: metrics.pixel_height,
            dpi: metrics.dpi,
        },
        StripOpts {
            is_mac: chrome.is_mac || chrome.preview,
            integrated_buttons: chrome.integrated_buttons,
            native_button_style: chrome.native_button_style,
            preview: chrome.preview && !chrome.is_mac,
            is_full_screen: chrome.is_full_screen,
            position_left,
            rail: model.rail,
            rail_width: cfg.rail_width,
            padding_top: cfg.render.padding.top,
            toggle_button,
            card_x1: Some((cfg.render.padding.left.max(0) + 1) as u32),
        },
    );
    (
        Strip {
            rows: i64::from(g.rows),
            cols: i64::from(g.cols),
            toggle_row: Some(i64::from(g.toggle_row)),
            cell_w: g.cell_w,
            toggle: None,
        },
        if reserve_disabled {
            Some(0)
        } else if metrics_fact.is_some() && chrome_fact.is_some() {
            Some(i64::from(g.cols))
        } else {
            None
        },
    )
}

pub fn enrich(
    cfg: &EngineConfig,
    theme: &Theme,
    model: &SidebarModel,
    (cols, rows): (i64, i64),
    ui: &UiState,
) -> Enriched {
    let icon_set = icons::resolve(&cfg.icon_map);
    let resolved = glyphs::resolve(
        &icon_set.map,
        cfg.glyph_custom_block,
        cfg.glyph_east_asian_wide,
    );

    let items: Vec<Item> = model
        .tabs
        .iter()
        .map(|tab| Item {
            tab_id: tab.id,
            index: tab.index,
            is_active: model.active == Some(tab.id),
            is_pinned: tab.pinned,
            is_private: model.private,
            title: title_of(tab),
            meta: meta_from(
                tab,
                cfg.meta_mode,
                cfg.render.meta_sep.as_deref().unwrap_or(" "),
            ),
            icon: if cfg.render.icons && tab.settings {
                icon_set.map.get("settings").cloned().unwrap_or_default()
            } else if cfg.render.icons {
                tab.icon.clone().unwrap_or_else(|| {
                    icons::for_process(tab.proc.as_deref().unwrap_or(""), &icon_set).to_string()
                })
            } else {
                String::new()
            },
            has_unseen: tab.unseen,
        })
        .collect();

    let scroll = model.scroll.unwrap_or_default();
    let (strip, rail_reserve) = strip_of(cfg, model, cols, rows);
    // the wheel moves the list before the model round-trips, so the local override wins while it holds
    let user_scrolled = ui.user_scrolled || scroll.user;
    let view = RenderInput {
        cols,
        rows,
        rail: model.rail,
        items,
        theme: theme.clone(),
        cfg: cfg.render.clone(),
        glyphs: resolved.glyphs,
        strip: Some(strip),
        strip_buttons: model
            .strip
            .as_ref()
            .map(|s| s.buttons.clone())
            .unwrap_or_default(),
        hover: ui.hover.map(|h| Hover { x: h.x, y: h.y }),
        drag: drag_of(model, ui),
        scroll: ui.scroll.unwrap_or(scroll.top),
        focus_index: model.focus.filter(|f| f.on).map(|f| f.index),
        ensure_visible: (!user_scrolled).then_some(model.active).flatten(),
        footer: model.footer.clone(),
        spaces: model
            .spaces
            .iter()
            .map(|s| SpaceEntry {
                id: s.id.clone(),
                name: s.name.clone(),
                icon: space_icon(s),
                is_active: model.space.as_deref() == Some(s.id.as_str()),
                has_unseen: s.unseen,
            })
            .collect(),
        private: model.private,
        user_scrolled,
        popover: None,
    };
    Enriched {
        view,
        popover: None,
        rail_reserve,
    }
}

/// The mirrored drag wins; a press this process is still holding shows through until it does.
fn drag_of(model: &SidebarModel, ui: &UiState) -> Option<Drag> {
    if let Some(d) = model.drag {
        return Some(Drag {
            tab_id: d.id,
            over_index: d.slot,
            active: d.active,
            outside: d.outside,
        });
    }
    ui.drag.filter(|d| d.active).map(|d| Drag {
        tab_id: d.tab_id,
        over_index: None,
        active: true,
        outside: false,
    })
}

/// Glyph lookups the runtime needs outside a frame (the strip's own toggle icon, say).
pub fn glyph_map(cfg: &EngineConfig) -> BTreeMap<String, String> {
    let icon_set = icons::resolve(&cfg.icon_map);
    glyphs::resolve(
        &icon_set.map,
        cfg.glyph_custom_block,
        cfg.glyph_east_asian_wide,
    )
    .glyphs
}

#[cfg(test)]
#[path = "../tests/unit/enrich.rs"]
mod tests;

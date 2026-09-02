//! The v2 command state plus this pane's own dimensions and pointer state, composed into the
//! renderer's input. Ports model.lua's `meta_from` and title fallback, and the resolution
//! view.lua used to do per window: icons, glyphs, theme and the strip's geometry.

use std::collections::BTreeMap;

use vtabs_core::geom::{Dims, StripOpts, strip_geometry};
use vtabs_core::{basename, icons, sanitize, ui::UiState};
use vtabs_protocol::v2::{ConfigMsg, ModelMsg, RenderSection, SpaceItem, TabRecord, ThemeMsg};

use crate::glyphs;
use crate::scene::{
    Drag, FooterEntry, Hover, Item, Padding, RenderCfg, RenderInput, SpaceEntry, Strip, StripButton,
};

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
}

fn some(s: Option<&String>) -> Option<&str> {
    s.map(String::as_str).filter(|v| !v.is_empty())
}

/// model.lua's `join`: the separator only earns its place between two parts.
fn join(prefix: Option<&str>, tail: Option<&str>, sep: &str) -> Option<String> {
    match (prefix, tail) {
        (Some(a), Some(b)) => Some(format!("{a}{sep}{b}")),
        (a, b) => a.or(b).map(str::to_string),
    }
}

/// model.lua's `meta_from`: the card's second line. `cwd` arrives already tilde'd from Lua.
fn meta_from(tab: &TabRecord, mode: Option<&str>, sep: &str) -> Option<String> {
    let mode = mode?;
    let dir = some(tab.cwd.as_ref());
    let process = some(tab.proc.as_ref());
    if mode == "cwd" {
        return dir.map(str::to_string);
    }
    if mode == "process" {
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
fn title_of(tab: &TabRecord) -> String {
    some(tab.override_title.as_ref())
        .or(Some(tab.title.as_str()).filter(|t| !t.is_empty()))
        .or(some(tab.pane_title.as_ref()))
        .map(str::to_string)
        .unwrap_or_else(|| format!("tab {}", tab.id))
}

fn render_defaults() -> RenderSection {
    RenderSection {
        meta: true,
        padding: Default::default(),
        frame: false,
        tab_height: None,
        row_gap: 0,
        separator: None,
        pinned_style: None,
        close_button: None,
        show_index: false,
        scroll_indicator: None,
        new_tab_button: false,
        new_tab_label: None,
        hover: None,
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

fn text(value: &Option<String>, fallback: &str) -> String {
    value
        .as_deref()
        .filter(|v| !v.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn palette_of(msg: &ThemeMsg) -> vtabs_theme::Palette {
    vtabs_theme::Palette {
        background: msg.scheme.background.clone(),
        foreground: msg.scheme.foreground.clone(),
        cursor_bg: msg.scheme.cursor_bg.clone(),
        active_tab_bg: msg.scheme.active_tab_bg.clone(),
        ansi: msg.scheme.ansi.clone(),
    }
}

/// Lua pre-resolves every override to hex, so an unknown key is a user typo, not a shape we must
/// guess at: it is ignored rather than failing the frame.
fn user_theme(msg: &ThemeMsg) -> vtabs_theme::UserTheme {
    let mut t = vtabs_theme::UserTheme {
        elevation: msg.elevation,
        ..Default::default()
    };
    for (key, value) in &msg.overrides {
        let hex = value.as_str().map(str::to_string);
        let number = value.as_f64();
        match key.as_str() {
            "fg" => t.fg = hex,
            "bg" => t.bg = hex,
            "elevation" => t.elevation = number.or(t.elevation),
            "accent" => t.accent = hex,
            "private_accent" => t.private_accent = hex,
            "hover_bg" => t.hover_bg = hex,
            "hover_fg" => t.hover_fg = hex,
            "active_bg" => t.active_bg = hex,
            "active_fg" => t.active_fg = hex,
            "focus_bg" => t.focus_bg = hex,
            "meta_fg" => t.meta_fg = hex,
            "dim" => t.dim = hex,
            "title_idle" => t.title_idle = hex,
            "title_active" => t.title_active = hex,
            "active_title_fg" => t.active_title_fg = hex,
            "pinned_fg" => t.pinned_fg = hex,
            "separator" => t.separator = hex,
            "border" => t.border = hex,
            "border_idle" => t.border_idle = hex,
            "ghost_border_hover" => t.ghost_border_hover = hex,
            "new_tab_fg" => t.new_tab_fg = hex,
            "close_fg" => t.close_fg = hex,
            "close_hover_fg" => t.close_hover_fg = hex,
            "unseen_fg" => t.unseen_fg = hex,
            "drag_bg" => t.drag_bg = hex,
            "drag_fg" => t.drag_fg = hex,
            "scroll_fg" => t.scroll_fg = hex,
            "scroll_idle_fg" => t.scroll_idle_fg = hex,
            "surface_raised" => t.surface_raised = hex,
            "scrim" => t.scrim = number,
            "disabled_fg" => t.disabled_fg = hex,
            "popover_sel_bg" => t.popover_sel_bg = hex,
            "popover_sel_fg" => t.popover_sel_fg = hex,
            "popover_sel_hint" => t.popover_sel_hint = hex,
            _ => {}
        }
    }
    t
}

/// The window's resolved theme, so a caller outside the frame — the menu — shares one answer.
pub fn theme_of(msg: &ThemeMsg, private: bool) -> vtabs_theme::Theme {
    vtabs_theme::resolve(&user_theme(msg), &palette_of(msg), private)
}

/// The strip Lua measured, when it sent one; otherwise what this pane's own size implies.
/// A pane knows its cells but not its pixels, so the macOS traffic-light reserve can only come
/// from Lua — `toggle_row` present is the signal that it did.
fn strip_of(
    cfg: &ConfigMsg,
    model: &ModelMsg,
    render: &RenderSection,
    cols: i64,
    rows: i64,
) -> Strip {
    if let Some(sent) = model.strip.as_ref().filter(|s| s.toggle_row.is_some()) {
        return Strip {
            rows: sent.rows,
            cols: sent.cols,
            toggle_row: sent.toggle_row,
            cell_w: sent.cell_w,
            toggle: None,
        };
    }
    let mac = cfg.mac.unwrap_or_default();
    let position_left = cfg.position.as_deref() != Some("right");
    let toggle_button = model
        .strip
        .as_ref()
        .is_some_and(|s| s.buttons.iter().any(|b| b.id == "toggle"));
    let g = strip_geometry(
        Dims {
            cols: cols.max(0) as u32,
            viewport_rows: rows.max(0) as u32,
            ..Default::default()
        },
        StripOpts {
            is_mac: mac.integrated_buttons && mac.native_button_style,
            integrated_buttons: mac.integrated_buttons,
            native_button_style: mac.native_button_style,
            preview: mac.preview,
            is_full_screen: mac.is_full_screen,
            position_left,
            rail: model.rail,
            rail_width: cfg.rail_width,
            padding_top: render.padding.top,
            toggle_button,
            card_x1: Some((render.padding.left.max(0) + 1) as u32),
        },
    );
    Strip {
        rows: i64::from(g.rows),
        cols: i64::from(g.cols),
        toggle_row: Some(i64::from(g.toggle_row)),
        cell_w: g.cell_w,
        toggle: None,
    }
}

pub fn enrich(
    cfg: &ConfigMsg,
    theme: &ThemeMsg,
    model: &ModelMsg,
    (cols, rows): (i64, i64),
    ui: &UiState,
) -> Enriched {
    let render = cfg.render.clone().unwrap_or_else(render_defaults);
    let icon_set = icons::resolve(&cfg.icon_map);
    let resolved = glyphs::resolve(
        &icon_set.map,
        cfg.glyphs.custom_block,
        cfg.glyphs.east_asian_wide,
    );
    let theme = theme_of(theme, model.private);

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
                cfg.meta.as_deref(),
                cfg.meta_sep.as_deref().unwrap_or(" "),
            ),
            icon: if cfg.icons && tab.settings {
                icon_set.map.get("settings").cloned().unwrap_or_default()
            } else if cfg.icons {
                icons::for_process(tab.proc.as_deref().unwrap_or(""), &icon_set).to_string()
            } else {
                String::new()
            },
            has_unseen: tab.unseen,
        })
        .collect();

    let scroll = model.scroll.unwrap_or_default();
    // the wheel moves the list before the model round-trips, so the local override wins while it holds
    let user_scrolled = ui.user_scrolled || scroll.user;
    let view = RenderInput {
        cols,
        rows,
        rail: model.rail,
        items,
        theme,
        cfg: RenderCfg {
            padding: Padding {
                left: render.padding.left,
                right: render.padding.right,
                top: render.padding.top,
                bottom: render.padding.bottom,
            },
            frame: render.frame,
            position: text(&cfg.position, "left"),
            new_tab_button: render.new_tab_button,
            new_tab_label: text(&render.new_tab_label, "New tab"),
            row_gap: render.row_gap,
            separator: text(&render.separator, "gap"),
            tab_height: text(&render.tab_height, "card"),
            meta: render.meta,
            meta_sep: cfg.meta_sep.clone(),
            show_index: render.show_index,
            icons: cfg.icons,
            close_button: text(&render.close_button, "hover"),
            hover: text(&render.hover, "follow"),
            pinned_style: text(&render.pinned_style, "compact"),
            scroll_indicator: text(&render.scroll_indicator, "auto"),
        },
        glyphs: resolved.glyphs,
        strip: Some(strip_of(cfg, model, &render, cols, rows)),
        strip_buttons: model
            .strip
            .as_ref()
            .map(|s| {
                s.buttons
                    .iter()
                    .map(|b| StripButton {
                        id: b.id.clone(),
                        icon: b.icon.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        hover: ui.hover.map(|h| Hover { x: h.x, y: h.y }),
        drag: drag_of(model, ui),
        scroll: ui.scroll.unwrap_or(scroll.top),
        focus_index: model.focus.filter(|f| f.on).map(|f| f.index),
        ensure_visible: (!user_scrolled).then_some(model.active).flatten(),
        footer: model
            .footer
            .iter()
            .map(|f| FooterEntry {
                text: f.text.clone(),
                icon: f.icon.clone(),
                id: f.id.clone(),
                fg: f.fg,
                bg: f.bg,
                icon_fg: f.icon_fg,
            })
            .collect(),
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
    }
}

/// The mirrored drag wins; a press this process is still holding shows through until it does.
fn drag_of(model: &ModelMsg, ui: &UiState) -> Option<Drag> {
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
pub fn glyph_map(cfg: &ConfigMsg) -> BTreeMap<String, String> {
    let icon_set = icons::resolve(&cfg.icon_map);
    glyphs::resolve(
        &icon_set.map,
        cfg.glyphs.custom_block,
        cfg.glyphs.east_asian_wide,
    )
    .glyphs
}

#[cfg(test)]
#[path = "../tests/unit/enrich.rs"]
mod tests;

use std::collections::BTreeMap;

use crate::sanitize;
use crate::theme::Theme;

use crate::config::Position;
use crate::layout::{self, CardSpan, GhostShape, Grid, LItem, Part, RowKind, St};
use crate::scene::{PopoverRect, RenderCfg, RenderInput};
use crate::text;
use crate::{
    color::Color,
    frame::{self, Cell, Style},
};

type Glyphs = BTreeMap<String, String>;

fn glyph<'a>(glyphs: &'a Glyphs, key: &str) -> Option<&'a str> {
    glyphs.get(key).map(String::as_str)
}

fn ellipsis(glyphs: &Glyphs) -> &str {
    glyph(glyphs, "ellipsis").unwrap_or("…")
}

fn first_char(s: &str) -> Option<char> {
    s.chars().next()
}

struct Ctx<'a> {
    theme: &'a Theme,
    cfg: &'a RenderCfg,
    cols: i64,
    glyphs: &'a Glyphs,
    grid: Grid,
}

fn row_colors(item: &LItem, theme: &Theme, st: &St) -> (Color, Color) {
    if st.dragging {
        return (theme.drag_fg, theme.drag_bg);
    }
    if st.focused {
        return (theme.fg, theme.focus_bg);
    }
    if item.item.is_active {
        return (theme.active_fg, theme.active_bg);
    }
    if st.hovered {
        return (theme.hover_fg, theme.hover_bg);
    }
    (theme.title_idle, theme.bg)
}

const BAR_MIN_HUE: i64 = 24;

/// A title that ran out of room and came back as `fg` scores well on contrast and shows no hue at
/// all, so the bar is gated on distance from `fg`, not on contrast against the card.
fn needs_bar(theme: &Theme) -> bool {
    let (title, fg) = (theme.title_active, theme.fg);
    let delta = (0..3)
        .map(|i| (i64::from(title[i]) - i64::from(fg[i])).abs())
        .max()
        .unwrap_or(0);
    delta < BAR_MIN_HUE
}

fn accent_of(item: &LItem, theme: &Theme) -> Color {
    if item.item.is_private {
        theme.private_accent
    } else {
        theme.accent
    }
}

fn marker<'a>(
    item: &LItem,
    theme: &Theme,
    st: &St,
    glyphs: &'a Glyphs,
) -> (Option<&'a str>, Color) {
    let accent = accent_of(item, theme);
    if (item.item.is_active || st.dragging) && needs_bar(theme) {
        return (glyph(glyphs, "active"), accent);
    }
    if st.focused {
        return (glyph(glyphs, "focus"), accent);
    }
    if item.item.has_unseen {
        return (glyph(glyphs, "unseen"), theme.unseen_fg);
    }
    (Some(" "), theme.fg)
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// Byte offset of the last plain occurrence of `needle`, mirroring Lua's `find(.., true)` loop.
fn last_index_of(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let (hay, needle) = (hay.as_bytes(), needle.as_bytes());
    let mut found = None;
    for i in 0..hay.len().saturating_sub(needle.len() - 1) {
        if &hay[i..i + needle.len()] == needle {
            found = Some(i);
        }
    }
    found
}

/// Path-shaped meta keeps its tail: two siblings must not both collapse to their shared parent.
fn fit_meta(s: &str, budget: i64, glyphs: &Glyphs, sep: &str) -> String {
    let budget = budget.max(0) as usize;
    if text::width(s) <= budget {
        return s.to_string();
    }
    let (head, tail) = match last_index_of(s, sep) {
        Some(at) => {
            let cut = at + sep.len();
            // the separator may or may not carry its own spacing; the path starts at the first byte
            let lead: usize = s[cut..]
                .chars()
                .take_while(|c| is_space(*c))
                .map(char::len_utf8)
                .sum();
            (s[..cut + lead].to_string(), s[cut + lead..].to_string())
        }
        None => (String::new(), s.to_string()),
    };
    let room = budget as i64 - text::width(&head) as i64;
    let bytes = tail.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let path_like =
        tail.starts_with('~') || tail.starts_with('/') || drive || tail.starts_with("\\\\");
    if !path_like || room < 3 {
        return text::truncate(s, budget, ellipsis(glyphs));
    }
    head + &text::shorten_path(&tail, room as usize, ellipsis(glyphs))
}

/// Paints one row of a card; which sub-targets exist was decided in layout.
#[allow(clippy::too_many_arguments)]
fn card_row(
    item: &LItem,
    ctx: &Ctx,
    st: &St,
    part: Part,
    rows_in_card: i64,
    span: Option<CardSpan>,
    row_in_card: i64,
) -> Vec<Cell> {
    let (theme, cfg, glyphs, g) = (ctx.theme, ctx.cfg, ctx.glyphs, &ctx.grid);
    let mut cells = frame::new_line(ctx.cols, theme.bg, theme.fg);
    if part == Part::Gap {
        return cells;
    }
    let (fg, bg) = row_colors(item, theme, st);
    let icon_fg = if st.dragging { fg } else { theme.meta_fg };
    frame::fill(&mut cells, g.card_x1, g.card_x2, bg);

    // the content block is centred, so the title row is the middle one in both modes
    if part == Part::Title {
        let (mark, mark_fg) = marker(item, theme, st, glyphs);
        frame::put(
            &mut cells,
            g.gutter,
            mark.unwrap_or(""),
            &Style::fg(mark_fg),
            g.gutter,
        );
    } else if (item.item.is_active || st.dragging) && needs_bar(theme) {
        // the bar runs the card's full height: one cell is too little to carry the active state
        let active = glyph(glyphs, "active").unwrap_or("");
        frame::put(
            &mut cells,
            g.gutter,
            active,
            &Style::fg(accent_of(item, theme)),
            g.gutter,
        );
    }

    let icon = sanitize(item.item.icon.as_bytes());
    let icon_style = Style::fg(if item.item.is_private {
        theme.private_accent
    } else {
        icon_fg
    });

    if !layout::has_text(g) {
        if part == Part::Title && !item.item.icon.is_empty() {
            frame::put(&mut cells, g.icon_x, &icon, &icon_style, g.icon_x);
        }
        return cells;
    }
    let (title_x1, title_x2) = (g.title_x1.unwrap_or(0), g.title_x2.unwrap_or(0));

    if part == Part::Title {
        if cfg.icons && !item.item.icon.is_empty() {
            frame::put(&mut cells, g.icon_x, &icon, &icon_style, g.icon_x);
        }
        let mut body = sanitize(item.item.title.as_bytes());
        // with no meta line the index has nowhere else to go, so it rides the title
        if cfg.show_index && !cfg.meta {
            body = format!("{}{}{}", item.item.index, meta_sep(cfg, glyphs), body);
        }
        let budget = g.title_budget.unwrap_or(0).max(0) as usize;
        let title = text::truncate(&body, budget, ellipsis(glyphs));
        let title_fg = if item.item.is_active && !st.dragging {
            theme.title_active
        } else {
            fg
        };
        let style = Style {
            fg: title_fg,
            bg: None,
            bold: item.item.is_active,
        };
        frame::put(&mut cells, title_x1, &title, &style, title_x2);
        if let Some(span) = span {
            let key = if span.id == "pin" { "pinned" } else { "close" };
            let glyph_fg = if span.id == "pin" {
                theme.pinned_fg
            } else if st.hovered && st.pointer_x.is_some_and(|x| x >= span.x1 && x <= span.x2) {
                theme.close_hover_fg
            } else {
                theme.close_fg
            };
            let close_x = g.close_x.unwrap_or(0);
            let g_str = glyph(glyphs, key).unwrap_or("");
            frame::put(&mut cells, close_x, g_str, &Style::fg(glyph_fg), close_x);
        }
    } else if part == Part::Meta {
        let mut meta = sanitize(item.item.meta.as_deref().unwrap_or("").as_bytes());
        let sep = meta_sep(cfg, glyphs).to_string();
        if cfg.show_index {
            meta = format!("{}{}{}", item.item.index, sep, meta);
        }
        let meta_fg = if st.dragging {
            theme.drag_fg
        } else {
            theme.meta_fg
        };
        let fitted = fit_meta(&meta, g.meta_budget.unwrap_or(0), glyphs, &sep);
        let x1 = g.meta_x1.unwrap_or(0);
        frame::put(
            &mut cells,
            x1,
            &fitted,
            &Style::fg(meta_fg),
            g.meta_x2.unwrap_or(0),
        );
    }

    let chamfered = (st.hovered || st.focused) && !item.item.is_active && !st.dragging;
    if chamfered && rows_in_card >= 2 && glyph(glyphs, "corners") == Some("chamfer") {
        let ch = if row_in_card == 1 {
            glyph(glyphs, "chamfer_top")
        } else {
            None
        }
        .or(if row_in_card == rows_in_card {
            glyph(glyphs, "chamfer_bottom")
        } else {
            None
        });
        if let Some(ch) = ch {
            let x = g.card_x2;
            if x >= 1 && x <= ctx.cols {
                cells[(x - 1) as usize] = Cell::new(first_char(ch), bg, theme.bg);
            }
        }
    }
    cells
}

fn meta_sep<'a>(cfg: &'a RenderCfg, glyphs: &'a Glyphs) -> &'a str {
    match cfg.meta_sep.as_deref() {
        Some(sep) => sep,
        None => glyph(glyphs, "meta_sep").unwrap_or(""),
    }
}

/// A strip action's glyph: the user's icon, the mirrored toggle, or the icon map's entry for the id.
fn action_glyph(action: &layout::Action, cfg: &RenderCfg, glyphs: &Glyphs) -> String {
    if let Some(icon) = &action.icon {
        return sanitize(icon.as_bytes());
    }
    let g = |key: &str| glyph(glyphs, key).map(str::to_string);
    let fallback = || g("new_tab").unwrap_or_default();
    match action.id.as_str() {
        "toggle_sidebar" => {
            if cfg.position == Position::Right {
                g("toggle_right")
                    .or_else(|| g("toggle_left"))
                    .unwrap_or_default()
            } else {
                g("toggle_left").unwrap_or_default()
            }
        }
        "new_tab" => g("strip_new_tab")
            .or_else(|| g("new_tab"))
            .unwrap_or_default(),
        "open_settings" => g("settings").unwrap_or_else(fallback),
        id => g(id).unwrap_or_else(fallback),
    }
}

fn chrome_row(
    ctx: &Ctx,
    glyph_str: Option<&str>,
    glyph_x: i64,
    body: &str,
    text_fg: Color,
    glyph_fg: Option<Color>,
    bg: Option<Color>,
) -> Vec<Cell> {
    let (theme, g) = (ctx.theme, &ctx.grid);
    let mut cells = frame::new_line(ctx.cols, bg.unwrap_or(theme.bg), theme.fg);
    if let Some(s) = glyph_str.filter(|s| !s.is_empty()) {
        frame::put(
            &mut cells,
            glyph_x,
            s,
            &Style::fg(glyph_fg.unwrap_or(text_fg)),
            glyph_x,
        );
    }
    if !body.is_empty() && layout::has_text(g) {
        let x1 = g.title_x1.unwrap_or(0);
        let budget = (g.card_x2 - x1 + 1).max(0) as usize;
        let fitted = text::truncate(body, budget, ellipsis(ctx.glyphs));
        frame::put(&mut cells, x1, &fitted, &Style::fg(text_fg), g.card_x2);
    }
    cells
}

/// The only outlined element in the sidebar: that is what makes it read as "not a tab".
fn ghost_rows(ctx: &Ctx, hovered: bool) -> Vec<Vec<Cell>> {
    let (theme, glyphs, g) = (ctx.theme, ctx.glyphs, &ctx.grid);
    // hover moves the border's colour and the label's, and nothing else
    let border_fg = if hovered {
        theme.ghost_border_hover
    } else {
        theme.border_idle
    };
    let mut rows = Vec::new();
    for i in 1..=3 {
        let mut cells = frame::new_line(ctx.cols, theme.bg, theme.fg);
        let border = Style::fg(border_fg);
        if i == 2 {
            let dash_v = glyph(glyphs, "frame_dash_v").unwrap_or("");
            frame::put(&mut cells, g.card_x1, dash_v, &border, g.card_x1);
            let new_tab = glyph(glyphs, "new_tab").unwrap_or("");
            frame::put(
                &mut cells,
                g.icon_x,
                new_tab,
                &Style::fg(theme.accent),
                g.icon_x,
            );
            if layout::has_text(g) {
                let x1 = g.title_x1.unwrap_or(0);
                let budget = (g.card_x2 - 1 - x1).max(0) as usize;
                let label = text::truncate(&ctx.cfg.new_tab_label, budget, ellipsis(glyphs));
                let fg = if hovered { theme.fg } else { theme.new_tab_fg };
                frame::put(&mut cells, x1, &label, &Style::fg(fg), g.card_x2 - 1);
            }
            frame::put(&mut cells, g.card_x2, dash_v, &border, g.card_x2);
        } else {
            let left = glyph(glyphs, if i == 1 { "frame_tl" } else { "frame_bl" }).unwrap_or("");
            frame::put(&mut cells, g.card_x1, left, &border, g.card_x1);
            // the glyph is dashed inside its own cell, so a solid run still reads as a dash
            let dash = glyph(glyphs, "frame_dash").unwrap_or("");
            for x in (g.card_x1 + 1)..=(g.card_x2 - 1) {
                frame::put(&mut cells, x, dash, &border, x);
            }
            let right = glyph(glyphs, if i == 1 { "frame_tr" } else { "frame_br" }).unwrap_or("");
            frame::put(&mut cells, g.card_x2, right, &border, g.card_x2);
        }
        rows.push(cells);
    }
    rows
}

/// Paints the inner edge in the content's own colour and chamfers the page away from it.
fn frame_edge(painted: &mut [Option<Vec<Cell>>], cols: i64, theme: &Theme, glyphs: &Glyphs) {
    let content = theme.content_bg;
    let painted_rows: Vec<usize> = painted
        .iter()
        .enumerate()
        .filter(|(_, cells)| cells.is_some())
        .map(|(row, _)| row)
        .collect();
    let (first, last) = (painted_rows.first().copied(), painted_rows.last().copied());
    let chamfer = glyph(glyphs, "corners") == Some("chamfer");
    for (row, slot) in painted.iter_mut().enumerate() {
        let Some(cells) = slot.as_mut() else {
            continue;
        };
        if cols >= 1 && (cols as usize) <= cells.len() {
            cells[(cols - 1) as usize] = Cell::new(Some(' '), theme.fg, content);
        }
        let edge = cols - 1;
        if chamfer
            && edge >= 1
            && (edge as usize) <= cells.len()
            && (Some(row) == first || Some(row) == last)
        {
            let key = if Some(row) == first {
                "chamfer_top"
            } else {
                "chamfer_bottom"
            };
            let ch = glyph(glyphs, key).and_then(first_char);
            cells[(edge - 1) as usize] = Cell::new(ch, theme.bg, content);
        }
    }
}

/// Overlays a popover rect on a laid-out frame and scrims every row it does not own.
fn composite(
    painted: &mut [Option<Vec<Cell>>],
    cols: i64,
    rows: i64,
    theme: &Theme,
    rect: &PopoverRect,
) {
    let scrim = rect.scrim;
    let x1 = rect.x.max(1);
    let x2 = (x1 + rect.w.unwrap_or(cols) - 1).min(cols);
    let y1 = rect.y.max(1);
    let y2 = (y1 + rect.h - 1).min(rows);
    if x1 > cols || y1 > rows || x2 < x1 || y2 < y1 {
        return;
    }
    for (idx, slot) in painted.iter_mut().enumerate() {
        let row = idx as i64 + 1;
        let Some(cells) = slot.as_mut() else { continue };
        if (row < y1 || row > y2) && scrim > 0.0 {
            for cell in cells.iter_mut() {
                cell.scrim = scrim;
            }
        }
    }
    for i in 1..=(y2 - y1 + 1) {
        let row = y1 + i - 1;
        let Some(cells) = painted.get_mut((row - 1) as usize).and_then(Option::as_mut) else {
            continue;
        };
        let spec = rect.rows.get((i - 1) as usize);
        let bg = spec
            .and_then(|s| s.bg)
            .or(rect.bg)
            .unwrap_or(theme.active_bg);
        let fg = spec.and_then(|s| s.fg).unwrap_or(theme.fg);
        for x in x1..=x2 {
            cells[(x - 1) as usize] = Cell::new(Some(' '), fg, bg);
        }
        for span in spec.map(|s| s.spans.as_slice()).unwrap_or(&[]) {
            let style = Style {
                fg: span.fg.unwrap_or(fg),
                bg: Some(bg),
                bold: span.bold,
            };
            frame::put(cells, x1 + span.x - 1, &span.text, &style, x2);
        }
    }
}

/// A laid-out frame: the cells, the per-row fade, and the plan whose regions answer for them.
pub struct Frame<'a> {
    pub cells: Vec<Option<Vec<Cell>>>,
    pub fades: Vec<Option<f64>>,
    pub plan: layout::Plan<'a>,
}

/// Lays a frame out into cells without encoding it; the runtime turns these into ANSI.
pub fn frame_of(view: &RenderInput) -> Frame<'_> {
    let (cells, fades, plan) = cells(view);
    Frame { cells, fades, plan }
}

#[allow(clippy::type_complexity)]
fn cells(view: &RenderInput) -> (Vec<Option<Vec<Cell>>>, Vec<Option<f64>>, layout::Plan<'_>) {
    let (cfg, theme, cols) = (&view.cfg, &view.theme, view.cols);
    let glyphs = &view.glyphs;
    let plan = layout::plan(view);
    let ctx = Ctx {
        theme,
        cfg,
        cols,
        glyphs,
        grid: plan.grid,
    };
    let g = &ctx.grid;
    let n = view.rows.max(0) as usize;
    let mut painted: Vec<Option<Vec<Cell>>> = vec![None; n];
    let mut fades: Vec<Option<f64>> = vec![None; n];

    for row in 0..n {
        let Some(spec) = &plan.rows[row] else {
            continue;
        };
        let mut cells = match &spec.kind {
            RowKind::Strip {
                actions,
                lit_id,
                glyph: draw,
            } => {
                let mut cells = frame::new_line(cols, theme.bg, theme.fg);
                for action in actions {
                    let on = lit_id.as_deref() == Some(action.id.as_str());
                    if on {
                        frame::fill(&mut cells, action.x1, action.x2, theme.hover_bg);
                    }
                    if *draw {
                        let fg = if on { theme.accent } else { theme.dim };
                        let s = action_glyph(action, cfg, glyphs);
                        frame::put(&mut cells, action.x, &s, &Style::fg(fg), cols);
                    }
                }
                cells
            }
            RowKind::Space => frame::new_line(cols, theme.bg, theme.fg),
            RowKind::Header => chrome_row(
                &ctx,
                glyph(glyphs, "private"),
                g.icon_x,
                "Private",
                theme.private_accent,
                Some(theme.private_accent),
                None,
            ),
            RowKind::Separator => {
                let mut cells = frame::new_line(cols, theme.bg, theme.fg);
                let rule = glyph(glyphs, "rule").unwrap_or("");
                for x in g.card_x1..=g.card_x2 {
                    frame::put(&mut cells, x, rule, &Style::fg(theme.separator), x);
                }
                cells
            }
            RowKind::Card {
                item,
                part,
                rows_in_card,
                row_in_card,
                st,
                span,
            } => card_row(item, &ctx, st, *part, *rows_in_card, *span, *row_in_card),
            RowKind::Ghost {
                shape,
                index,
                hovered,
            } => match shape {
                GhostShape::Rail => {
                    let bg = if *hovered { theme.hover_bg } else { theme.bg };
                    let mut cells = frame::new_line(cols, bg, theme.fg);
                    let new_tab = glyph(glyphs, "new_tab").unwrap_or("");
                    frame::put(
                        &mut cells,
                        g.icon_x,
                        new_tab,
                        &Style::fg(theme.accent),
                        cols,
                    );
                    cells
                }
                GhostShape::Card => {
                    let rows = ghost_rows(&ctx, *hovered);
                    let nth = usize::try_from(*index - 1).ok();
                    match nth.and_then(|i| rows.into_iter().nth(i)) {
                        Some(cells) => cells,
                        None => continue,
                    }
                }
                GhostShape::Row => chrome_row(
                    &ctx,
                    glyph(glyphs, "new_tab"),
                    g.icon_x,
                    &cfg.new_tab_label,
                    if *hovered { theme.fg } else { theme.new_tab_fg },
                    Some(theme.accent),
                    Some(if *hovered { theme.hover_bg } else { theme.bg }),
                ),
            },
            RowKind::Footer { entry, hovered } => {
                let mut fg = entry.fg.unwrap_or(theme.meta_fg);
                let mut bg = entry.bg.unwrap_or(theme.bg);
                if *hovered {
                    fg = theme.hover_fg;
                    bg = theme.hover_bg;
                }
                chrome_row(
                    &ctx,
                    entry.icon.as_deref(),
                    g.icon_x,
                    &sanitize(entry.text.as_bytes()),
                    fg,
                    Some(entry.icon_fg.unwrap_or(fg)),
                    Some(bg),
                )
            }
            RowKind::Spaces { slots, lit_id } => {
                let mut cells = frame::new_line(cols, theme.bg, theme.fg);
                for slot in slots {
                    let lit = lit_id.as_deref() == Some(slot.id.as_str());
                    // the active slot wears the pill the active card does; hover is the strip's
                    let (fg, fill) = if slot.active {
                        (theme.accent, Some(theme.active_bg))
                    } else if lit {
                        (theme.fg, Some(theme.hover_bg))
                    } else if slot.unseen {
                        (theme.unseen_fg, None)
                    } else {
                        (theme.dim, None)
                    };
                    if let Some(bg) = fill {
                        frame::fill(&mut cells, slot.x1, slot.x2, bg);
                    }
                    let fg = if slot.cut {
                        frame::faded(fg, theme.bg, 0.5)
                    } else {
                        fg
                    };
                    frame::put(&mut cells, slot.x, &slot.icon, &Style::fg(fg), slot.x2);
                }
                cells
            }
        };
        if spec.thumb && cols >= 1 && (cols as usize) <= cells.len() {
            let fg = if spec.thumb_lit {
                theme.scroll_fg
            } else {
                theme.scroll_idle_fg
            };
            let ch = glyph(glyphs, "scroll").and_then(first_char);
            cells[(cols - 1) as usize] = Cell::new(ch, fg, theme.bg);
        }
        let chrome = matches!(
            spec.kind,
            RowKind::Strip { .. } | RowKind::Footer { .. } | RowKind::Spaces { .. }
        );
        if view.drag.is_some_and(|d| d.outside) && !chrome {
            let edge = if cfg.position == Position::Right {
                1
            } else {
                cols
            };
            if edge >= 1 && (edge as usize) <= cells.len() {
                cells[(edge - 1) as usize] = Cell::new(Some(' '), theme.fg, theme.accent);
            }
        }
        painted[row] = Some(cells);
        fades[row] = spec.fade;
    }

    if cfg.frame {
        frame_edge(&mut painted, cols, theme, glyphs);
    }
    if let Some(rect) = &view.popover {
        composite(&mut painted, cols, view.rows, theme, rect);
    }
    (painted, fades, plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use vtabs_protocol::payload::ConfigMsg;

    fn config(position: &str) -> RenderCfg {
        let message: ConfigMsg = serde_json::from_value(serde_json::json!({
            "position": position,
        }))
        .unwrap();
        EngineConfig::try_from(message).unwrap().render
    }

    fn action(id: &str) -> layout::Action {
        layout::Action {
            id: id.into(),
            icon: None,
            x: 1,
            x1: 1,
            x2: 1,
        }
    }

    #[test]
    fn canonical_strip_action_ids_select_their_glyphs() {
        let glyphs = Glyphs::from([
            ("toggle_left".into(), "L".into()),
            ("toggle_right".into(), "R".into()),
            ("settings".into(), "S".into()),
            ("new_tab".into(), "+".into()),
        ]);
        assert_eq!(
            action_glyph(&action("toggle_sidebar"), &config("left"), &glyphs),
            "L"
        );
        assert_eq!(
            action_glyph(&action("toggle_sidebar"), &config("right"), &glyphs),
            "R"
        );
        assert_eq!(
            action_glyph(&action("open_settings"), &config("left"), &glyphs),
            "S"
        );
    }
}

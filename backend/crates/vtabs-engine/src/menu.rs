use crate::theme::Theme;
use crate::{
    sanitize,
    strings::{grapheme_count, graphemes},
};
use vtabs_protocol::payload::{MenuItem, MenuMsg};

use crate::enrich::{MenuHeader, PopoverHits};
use crate::text;
use crate::{
    color::Color,
    scene::{PopRow, PopSpan, PopoverRect},
};

pub const MAX_TITLE_ROWS: usize = 3;
pub const MAX_HINT_COLS: i64 = 8;
pub const FRAME_ROWS: i64 = 2;
pub const MIN_ITEM_ROWS: i64 = 4;
pub const MIN_W: i64 = 16;
pub const MIN_RENDER_W: i64 = 4;
/// Interior columns a row spends on anything but its label: borders, marker, both margins, and
/// the gap a hint needs on top of that.
pub const LABEL_PAD: i64 = 5;
pub const HINT_PAD: i64 = 6;
/// Every row's text starts here, so header lines and item labels share one left margin.
pub const TEXT_REL: i64 = 4;
pub const MIN_ROWS: i64 = FRAME_ROWS + MIN_ITEM_ROWS;

const LABEL_ELLIPSIS: &str = "…";
const RENAME_ROWS: i64 = 5;

mod drop {
    pub const META: i64 = 1;
    pub const TITLE_EXTRA: i64 = 3;
    pub const SEPARATOR: i64 = 4;
    pub const TITLE: i64 = 5;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Root,
    Confirm,
    Rename,
    /// "Move to space ▸": plain rows like the root, but a sub-level, so Esc steps back to it.
    Spaces,
}

impl Level {
    pub fn of(msg: &MenuMsg) -> Level {
        match msg.level.as_deref() {
            Some("confirm") => Level::Confirm,
            Some("rename") => Level::Rename,
            Some("spaces") => Level::Spaces,
            _ => Level::Root,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Root => "root",
            Level::Confirm => "confirm",
            Level::Rename => "rename",
            Level::Spaces => "spaces",
        }
    }
}

/// The knobs popover.lua read off `config.get()`; the config message carries all of them.
#[derive(Debug, Clone)]
pub struct MenuCfg {
    pub padding_left: i64,
    pub padding_right: i64,
    /// `cfg.popover.width`; None is `"auto"`.
    pub want_width: Option<i64>,
    pub ellipsis: String,
    pub follow_pointer: bool,
}

impl Default for MenuCfg {
    fn default() -> Self {
        MenuCfg {
            padding_left: 0,
            padding_right: 0,
            want_width: None,
            ellipsis: "…".into(),
            follow_pointer: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuState {
    pub selected: i64,
    pub buffer: String,
    pub cursor: usize,
    pub armed: Option<String>,
    dismissed: bool,
    sig: Option<(String, Vec<MenuItem>)>,
    root_width: Option<i64>,
}

impl MenuState {
    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }

    pub fn is_dismissed(&self) -> bool {
        self.dismissed
    }

    /// `selected` is the message's only when the level or its items changed; otherwise the
    /// selection is Rust-local and a stale round-trip must not drag it back.
    pub fn adopt(&mut self, msg: &MenuMsg) {
        let level = Level::of(msg);
        let name = if msg.open { level.name() } else { "" };
        let sig = (name.to_string(), msg.items.clone());
        if self.sig.as_ref() != Some(&sig) {
            self.sig = Some(sig);
            self.selected = first_enabled(&msg.items, msg.selected.unwrap_or(1));
            self.armed = None;
            self.dismissed = false;
            if level == Level::Rename {
                self.buffer = sanitize(
                    msg.items
                        .first()
                        .and_then(|i| i.value.as_deref())
                        .unwrap_or("")
                        .as_bytes(),
                );
                self.cursor = grapheme_count(&self.buffer) + 1;
            }
        }
        if msg.open && level == Level::Root {
            self.root_width = Some(natural_width(&msg.items, &[]));
        }
    }
}

/// What `rect()` decided: the composited rect, the rows a click can name, and the selection the
/// clamp settled on.
#[derive(Debug, Clone)]
pub struct Placed {
    pub rect: PopoverRect,
    pub hits: PopoverHits,
    pub level: Level,
    pub selected: i64,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    /// No menu message, or one that says closed: nothing is drawn and nothing is consumed.
    Closed,
    Refused {
        why: &'static str,
        level: Level,
    },
    Open(Box<Placed>),
}

/// util.wrap: breaks on a space, else after a `/`, else hard, so a path keeps its boundaries.
pub fn wrap(text: &str, budget: i64, rows: usize, ellipsis: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut rest = sanitize(text.as_bytes());
    while !rest.is_empty() && lines.len() < rows {
        if text::width(&rest) as i64 <= budget {
            lines.push(rest);
            return lines;
        }
        let bytes = rest.as_bytes();
        let hi = usize::try_from(budget + 1).unwrap_or(0).min(bytes.len());
        let mut cut = None;
        let mut i = hi;
        while i >= 2 {
            match bytes[i - 2] {
                b' ' => {
                    cut = Some(i - 1);
                    break;
                }
                b'/' if cut.is_none() => cut = Some(i),
                _ => {}
            }
            i -= 1;
        }
        let hard = usize::try_from(budget + 1).unwrap_or(0);
        // A hard cut is a byte index Lua would split a sequence at; Rust snaps back to the char.
        let at = floor_boundary(&rest, cut.unwrap_or(hard).saturating_sub(1).min(rest.len()));
        lines.push(rest[..at].trim_end_matches(' ').to_string());
        rest = rest[at..].trim_start_matches(' ').to_string();
    }
    if !rest.is_empty()
        && let Some(last) = lines.last_mut()
    {
        *last = text::truncate(&format!("{last} {rest}"), cols(budget), ellipsis);
    }
    lines
}

fn floor_boundary(s: &str, at: usize) -> usize {
    let mut at = at.min(s.len());
    while at > 0 && !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn cols(n: i64) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// A `selected` that names a row that cannot act — the current space on the `spaces` level — would
/// leave the highlight on a dead row; the nearest enabled one takes it, forward first.
fn first_enabled(items: &[MenuItem], from: i64) -> i64 {
    let n = items.len() as i64;
    if from < 1 || from > n || !items[cols(from - 1)].disabled {
        return from;
    }
    let forward = move_by(items, from, 1);
    if forward != from {
        forward
    } else {
        move_by(items, from, -1)
    }
}

fn label_width(item: &MenuItem) -> i64 {
    let w = text::width(&item.label) as i64;
    match &item.hint {
        Some(hint) => w + text::width(hint) as i64 + HINT_PAD,
        None => w + LABEL_PAD,
    }
}

fn natural_width(items: &[MenuItem], head: &[String]) -> i64 {
    let mut natural = MIN_W;
    for item in items {
        natural = natural.max(label_width(item));
    }
    for line in head {
        natural = natural.max(text::width(line) as i64 + LABEL_PAD);
    }
    natural
}

pub fn width_for(cfg: &MenuCfg, cols: i64, items: &[MenuItem], head: &[String], floor: i64) -> i64 {
    let avail = cols - cfg.padding_left - cfg.padding_right;
    let natural = natural_width(items, head).max(floor);
    let want = cfg.want_width.unwrap_or(natural);
    want.min(avail).max(MIN_W.min(avail))
}

#[derive(Debug, Clone)]
struct HeadLine {
    text: String,
    meta_tone: bool,
    drop: i64,
}

/// Rule 1 then rule 2: below the anchor, else above it, else nil.
fn place(anchor: i64, height: i64, rows: i64) -> Option<i64> {
    if anchor + height <= rows {
        return Some(anchor + 1);
    }
    if anchor - height >= 1 {
        return Some(anchor - height);
    }
    None
}

fn scroll_for(index: i64, visible: i64, count: i64) -> i64 {
    if count <= visible {
        return 0;
    }
    (index - visible).min(count - visible).max(0)
}

/// Removes the lowest-priority lines until `keep` remain.
fn drop_to(lines: &[HeadLine], keep: usize) -> Vec<HeadLine> {
    let mut out = lines.to_vec();
    while out.len() > keep {
        let at = out
            .iter()
            .enumerate()
            .min_by_key(|(_, line)| line.drop)
            .map(|(i, _)| i);
        match at {
            Some(i) => out.remove(i),
            None => break,
        };
    }
    out
}

fn header(
    menu_header: Option<&MenuHeader>,
    level: Level,
    budget: i64,
    ellipsis: &str,
) -> Vec<HeadLine> {
    let (title, meta) = match menu_header {
        Some(h) => (h.title.as_str(), h.meta.as_deref()),
        None => ("tab", None),
    };
    let mut lines: Vec<HeadLine> = Vec::new();
    if level == Level::Confirm {
        // The qualifier goes before the question does: "and 3 others" alone asks nothing.
        for (n, ask) in [Some(title), meta].into_iter().flatten().enumerate() {
            for (i, line) in wrap(ask, budget, MAX_TITLE_ROWS, ellipsis)
                .into_iter()
                .enumerate()
            {
                let first = n == 0 && i == 0;
                lines.push(HeadLine {
                    text: line,
                    meta_tone: false,
                    drop: if first {
                        drop::TITLE
                    } else {
                        drop::TITLE_EXTRA
                    },
                });
            }
        }
        lines.push(HeadLine {
            text: String::new(),
            meta_tone: true,
            drop: drop::SEPARATOR,
        });
        return lines;
    }
    let title = if title.is_empty() { "tab" } else { title };
    for (i, line) in wrap(title, budget, MAX_TITLE_ROWS, ellipsis)
        .into_iter()
        .enumerate()
    {
        lines.push(HeadLine {
            text: line,
            meta_tone: false,
            drop: if i == 0 {
                drop::TITLE
            } else {
                drop::TITLE_EXTRA
            },
        });
    }
    if let Some(meta) = meta.filter(|m| !m.is_empty()) {
        lines.push(HeadLine {
            text: text::truncate(meta, cols(budget), ellipsis),
            meta_tone: true,
            drop: drop::META,
        });
    }
    if matches!(level, Level::Root | Level::Spaces) && !lines.is_empty() {
        lines.push(HeadLine {
            text: String::new(),
            meta_tone: true,
            drop: drop::SEPARATOR,
        });
    }
    lines
}

struct Layout {
    a: i64,
    lines: Vec<HeadLine>,
    visible: i64,
    scroll: i64,
}

fn layout(full: &[HeadLine], count: i64, index: i64, anchor_row: i64, rows: i64) -> Layout {
    let anchor = anchor_row.clamp(0, rows);
    for keep in (0..=full.len()).rev() {
        let lines = drop_to(full, keep);
        let height = FRAME_ROWS + lines.len() as i64 + count;
        if height <= rows
            && let Some(a) = place(anchor, height, rows)
        {
            return Layout {
                a,
                lines,
                visible: count,
                scroll: 0,
            };
        }
    }
    // Rules 4 and 5: no header, scroll the list, and take the pane when even that will not fit.
    let visible = count.min(rows - FRAME_ROWS).max(1);
    let a = place(anchor, FRAME_ROWS + visible, rows).unwrap_or(1);
    Layout {
        a,
        lines: Vec::new(),
        visible,
        scroll: scroll_for(index, visible, count),
    }
}

fn span(x: i64, text: impl Into<String>, fg: Color) -> PopSpan {
    PopSpan {
        x,
        text: text.into(),
        fg: Some(fg),
        bold: false,
    }
}

fn frame_row(w: i64, left: &str, fill: &str, right: &str, theme: &Theme) -> PopRow {
    let body = fill.repeat(cols(w - 2));
    PopRow {
        bg: None,
        fg: None,
        spans: vec![span(1, format!("{left}{body}{right}"), theme.border)],
    }
}

fn text_row(text: &str, tone: Color, w: i64, theme: &Theme, ellipsis: &str) -> PopRow {
    PopRow {
        bg: None,
        fg: None,
        spans: vec![
            span(1, "│", theme.border),
            span(w, "│", theme.border),
            span(
                3,
                text::truncate(&sanitize(text.as_bytes()), cols(w - 4), ellipsis),
                tone,
            ),
        ],
    }
}

/// Right-aligns `hint` so it ends at the interior's last column.
fn item_row(entry: &MenuItem, w: i64, selected: bool, theme: &Theme) -> PopRow {
    let txt_x2 = w - 2;
    // The selected row is an accent fill; its ink covers the destructive tint too — red on
    // accent is unreadable.
    let (fg, hint_fg) = if selected {
        (theme.popover_sel_fg, theme.popover_sel_hint)
    } else if entry.disabled {
        (theme.disabled_fg, theme.disabled_fg)
    } else {
        (theme.fg, theme.disabled_fg)
    };
    let mut spans = vec![
        span(1, "│", theme.border),
        span(w, "│", theme.border),
        span(
            TEXT_REL,
            text::truncate(
                &sanitize(entry.label.as_bytes()),
                cols(txt_x2 - TEXT_REL + 1),
                LABEL_ELLIPSIS,
            ),
            fg,
        ),
    ];
    if selected {
        // An accent bar on an accent field would be invisible, so the marker takes the ink colour.
        spans.push(span(2, "▎", fg));
    }
    if let Some(hint) = entry.hint.as_ref() {
        let hint = sanitize(hint.as_bytes());
        spans.push(span(txt_x2 - text::width(&hint) as i64 + 1, hint, hint_fg));
    }
    PopRow {
        bg: selected.then_some(theme.popover_sel_bg),
        fg: None,
        spans,
    }
}

fn rename_rows(state: &MenuState, w: i64, theme: &Theme, ellipsis: &str) -> Vec<PopRow> {
    let budget = cols(w - 4);
    let cleaned = sanitize(state.buffer.as_bytes());
    let clusters: Vec<&str> = graphemes(&cleaned).collect();
    let cursor = state.cursor.clamp(1, clusters.len() + 1) - 1;
    let under = clusters.get(cursor).copied().unwrap_or(" ");
    let cursor_width = text::width(under).max(1);

    // Keep the cursor and the complete grapheme under it inside the field, then use any remaining
    // columns for the suffix on either side. Grapheme indices are editing positions, never cells.
    let mut from = cursor;
    let mut before_width = 0;
    while from > 0 {
        let previous = text::width(clusters[from - 1]);
        if previous + before_width + cursor_width > budget {
            break;
        }
        from -= 1;
        before_width += previous;
    }
    let mut shown = String::new();
    let mut shown_width = 0;
    for cluster in &clusters[from..] {
        let cluster_width = text::width(cluster);
        if shown_width + cluster_width > budget {
            break;
        }
        shown.push_str(cluster);
        shown_width += cluster_width;
    }
    vec![
        text_row("Rename tab", theme.fg, w, theme, ellipsis),
        text_row("", theme.meta_fg, w, theme, ellipsis),
        PopRow {
            bg: None,
            fg: None,
            spans: vec![
                span(1, "│", theme.border),
                span(w, "│", theme.border),
                span(3, shown, theme.fg),
                span(3 + before_width as i64, under, theme.bg),
            ],
        },
        text_row("", theme.meta_fg, w, theme, ellipsis),
        text_row("⏎ save   esc cancel", theme.meta_fg, w, theme, ellipsis),
    ]
}

fn head_texts(menu_header: Option<&MenuHeader>, level: Level) -> Vec<String> {
    if level != Level::Confirm {
        return Vec::new();
    }
    let Some(h) = menu_header else {
        return Vec::new();
    };
    let mut out = vec![h.title.clone()];
    out.extend(h.meta.clone());
    out
}

/// The rect `composite` overlays, plus the rows a click can name: popover.lua's `rect()`.
pub fn plan(
    msg: &MenuMsg,
    menu_header: Option<&MenuHeader>,
    state: &MenuState,
    cfg: &MenuCfg,
    theme: &Theme,
    (cols_n, rows): (i64, i64),
) -> Outcome {
    if !msg.open {
        return Outcome::Closed;
    }
    if state.dismissed {
        return Outcome::Closed;
    }
    let level = Level::of(msg);
    let head = head_texts(menu_header, level);
    let floor = match level {
        Level::Rename | Level::Spaces => state.root_width.unwrap_or(MIN_W),
        _ => MIN_W,
    };
    let w = width_for(cfg, cols_n, &msg.items, &head, floor);
    if w < MIN_RENDER_W {
        return Outcome::Refused {
            why: "width",
            level,
        };
    }
    if rows < FRAME_ROWS + 1 {
        return Outcome::Refused { why: "rows", level };
    }

    let first_col = cfg.padding_left + 1;
    let anchor = msg.anchor.unwrap_or_default();
    let anchor_col = anchor.col.unwrap_or(first_col);
    let x = anchor_col
        .min(cols_n - cfg.padding_right - w + 1)
        .max(first_col);

    let count = msg.items.len() as i64;
    let mut selected = state.selected;
    let mut body: Vec<PopRow> = Vec::new();
    let mut ids: Vec<(Option<String>, bool)> = Vec::new();
    let a;
    if level == Level::Rename {
        let content = rename_rows(state, w, theme, &cfg.ellipsis);
        a = (anchor.row + 1).clamp(1, (rows - RENAME_ROWS - 1).max(1));
        ids.extend(content.iter().map(|_| (None, false)));
        body = content;
    } else {
        let full = header(menu_header, level, w - LABEL_PAD, &cfg.ellipsis);
        let placed = layout(&full, count, selected, anchor.row, rows);
        a = placed.a;
        for line in &placed.lines {
            let tone = if line.meta_tone {
                theme.meta_fg
            } else {
                theme.fg
            };
            body.push(text_row(&line.text, tone, w, theme, &cfg.ellipsis));
            ids.push((None, false));
        }
        selected = selected.min(count).max(1);
        for i in 1..=placed.visible {
            let nth = i + placed.scroll;
            let Some(entry) = msg.items.get(cols(nth - 1)) else {
                continue;
            };
            body.push(item_row(entry, w, nth == selected, theme));
            ids.push((Some(entry.id.clone()), entry.disabled));
        }
    }

    let mut out = vec![frame_row(w, "╭", "─", "╮", theme)];
    let mut hit_ids = vec![(None, false)];
    out.append(&mut body);
    hit_ids.append(&mut ids);
    out.push(frame_row(w, "╰", "─", "╯", theme));
    hit_ids.push((None, false));

    let h = out.len() as i64;
    let y = a.min((rows - h + 1).max(1)).max(1);
    Outcome::Open(Box::new(Placed {
        rect: PopoverRect {
            x,
            y,
            w: Some(w),
            h,
            scrim: theme.scrim,
            bg: Some(theme.surface_raised),
            rows: out,
        },
        hits: PopoverHits {
            x,
            y,
            w,
            h,
            rows: hit_ids,
        },
        level,
        selected,
    }))
}

/// `M.move`: moves the selection by `delta`, skipping disabled items and stopping at the ends.
pub fn move_by(items: &[MenuItem], from: i64, delta: i64) -> i64 {
    let n = items.len() as i64;
    let mut i = from;
    for _ in 0..n {
        i += delta;
        if i < 1 || i > n {
            return from;
        }
        if !items[cols(i - 1)].disabled {
            return i;
        }
    }
    from
}

/// `M.jump`: first-letter jump to the next enabled item starting with `ch`.
pub fn jump(items: &[MenuItem], from: i64, ch: char) -> Option<i64> {
    let n = items.len() as i64;
    if n == 0 {
        return None;
    }
    let want = ch.to_lowercase().next()?;
    for step in 1..=n {
        let i = (from + step - 1).rem_euclid(n) + 1;
        let item = &items[cols(i - 1)];
        let head = item
            .label
            .chars()
            .next()
            .and_then(|c| c.to_lowercase().next());
        if !item.disabled && head == Some(want) {
            return Some(i);
        }
    }
    None
}

pub fn point_at(
    items: &[MenuItem],
    level: Level,
    follow_pointer: bool,
    from: i64,
    id: Option<&str>,
) -> Option<i64> {
    if !follow_pointer || level == Level::Confirm {
        return None;
    }
    let id = id?;
    let at = items.iter().position(|item| item.id == id)? as i64 + 1;
    let item = &items[cols(at - 1)];
    (!item.disabled && from != at).then_some(at)
}

/// popover.lua's `edit`, applied to the buffer Rust owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Commit,
    Cancel,
    Consumed,
}

pub fn edit(state: &mut MenuState, key: &str, ctrl: bool) -> Edit {
    let mut clusters: Vec<String> = graphemes(&state.buffer).map(str::to_owned).collect();
    let at = state.cursor.clamp(1, clusters.len() + 1);
    let end = clusters.len() + 1;
    match key {
        "enter" => return Edit::Commit,
        "escape" => return Edit::Cancel,
        "c" if ctrl => return Edit::Cancel,
        "u" if ctrl => rebuild(state, Vec::new(), 1),
        "a" if ctrl => state.cursor = 1,
        "e" if ctrl => state.cursor = end,
        "k" if ctrl => {
            clusters.truncate(at - 1);
            rebuild(state, clusters, at);
        }
        "w" if ctrl => {
            let mut i = at - 1;
            while i > 0 && clusters[i - 1] == " " {
                i -= 1;
            }
            while i > 0 && clusters[i - 1] != " " {
                i -= 1;
            }
            clusters.drain(i..at - 1);
            rebuild(state, clusters, i + 1);
        }
        "backspace" if at > 1 => {
            clusters.remove(at - 2);
            rebuild(state, clusters, at - 1);
        }
        "delete" if at <= clusters.len() => {
            clusters.remove(at - 1);
            rebuild(state, clusters, at);
        }
        "left" => state.cursor = (at - 1).max(1),
        "right" => state.cursor = (at + 1).min(end),
        "home" => state.cursor = 1,
        "end" => state.cursor = end,
        _ => {
            let one = (!ctrl
                && grapheme_count(key) == 1
                && sanitize(key.as_bytes()) == key
                && clusters.len() < 256)
                .then(|| key.to_string());
            if let Some(cluster) = one {
                clusters.insert(at - 1, cluster);
                rebuild(state, clusters, at + 1);
            }
        }
    }
    Edit::Consumed
}

fn rebuild(state: &mut MenuState, list: Vec<String>, cursor: usize) {
    let end = list.len() + 1;
    state.buffer = list.concat();
    state.cursor = cursor.clamp(1, end);
}

#[cfg(test)]
#[path = "../tests/unit/menu.rs"]
mod tests;

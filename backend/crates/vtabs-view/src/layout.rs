//! Port of plugin/vtabs/layout.lua: everything about a frame that does not depend on colour,
//! including the `hits` map that names what each row and column answers for.

use vtabs_core::geom::BUTTON_PITCH_PT;

use crate::scene::{FooterEntry, Item, RenderCfg, RenderInput, SpaceEntry, Strip, StripButton};

/// Column grid of §1.1; every landmark derives from `cols` and `padding`.
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub card_x1: i64,
    pub card_x2: i64,
    pub gutter: i64,
    pub icon_x: i64,
    pub close_x: Option<i64>,
    pub title_x1: Option<i64>,
    pub title_x2: Option<i64>,
    pub meta_x1: Option<i64>,
    pub meta_x2: Option<i64>,
    pub close_x1: Option<i64>,
    pub close_x2: Option<i64>,
    pub title_budget: Option<i64>,
    pub meta_budget: Option<i64>,
}

pub fn grid(cfg: &RenderCfg, cols: i64) -> Grid {
    let pad_l = cfg.padding.left.max(0);
    let pad_r = cfg.padding.right.max(0);
    let card_x1 = pad_l + 1;
    let card_x2 = (cols - pad_r - i64::from(cfg.frame)).max(card_x1);
    let gutter = card_x1;
    let icon_x = gutter + 2;
    let title_x1 = icon_x + 2;
    let close_x = card_x2 - 1;
    let title_x2 = close_x - 2;
    let meta_x2 = close_x - 1;
    Grid {
        card_x1,
        card_x2,
        gutter,
        icon_x,
        close_x: Some(close_x),
        title_x1: Some(title_x1),
        title_x2: Some(title_x2),
        meta_x1: Some(title_x1),
        meta_x2: Some(meta_x2),
        close_x1: Some(close_x - 1),
        close_x2: Some(close_x + 1),
        title_budget: Some((title_x2 - title_x1 + 1).max(0)),
        meta_budget: Some((meta_x2 - title_x1 + 1).max(0)),
    }
}

/// Collapsed: one icon column, no title, no close, no meta. The nils are the point.
pub fn rail_grid(cols: i64) -> Grid {
    Grid {
        card_x1: 1,
        card_x2: cols,
        gutter: 1,
        icon_x: (f64::from(cols as i32) / 2.0).ceil() as i64,
        close_x: None,
        title_x1: None,
        title_x2: None,
        meta_x1: None,
        meta_x2: None,
        close_x1: None,
        close_x2: None,
        title_budget: None,
        meta_budget: None,
    }
}

/// Whether the grid has a text column at all; a nil budget means the glyph is all there is room for.
pub fn has_text(g: &Grid) -> bool {
    g.title_budget.is_some()
}

const ACTION_STRIDE: i64 = 3;

/// Columns between one action glyph and the next. The lights are 20 pt apart whatever the font is,
/// so a fixed cell count drifts out of step with them; two is the floor a hit span can live in.
fn action_stride(strip: Option<&Strip>) -> i64 {
    match strip.and_then(|s| s.cell_w) {
        Some(w) if w > 0.0 => 2.max((BUTTON_PITCH_PT / w + 0.5).floor() as i64),
        _ => ACTION_STRIDE,
    }
}

#[derive(Debug, Clone)]
pub struct Action {
    pub id: String,
    pub icon: Option<String>,
    pub x: i64,
    pub x1: i64,
    pub x2: i64,
}

/// Action glyphs and their hit spans, left to right on the lights' own row. `buttons` is what
/// layout.lua's `resolved_actions(cfg)` would have returned; Lua resolves it before the scene ships.
pub fn strip_actions(
    buttons: &[StripButton],
    cfg: &RenderCfg,
    strip: Option<&Strip>,
    g: &Grid,
    rail: bool,
    cols: i64,
) -> (Vec<Action>, Option<i64>) {
    let row = strip.and_then(|s| s.toggle_row.or_else(|| s.toggle.map(|t| t.row)));
    let (Some(row), false) = (row, buttons.is_empty()) else {
        return (Vec::new(), None);
    };
    let strip = strip.expect("a toggle row implies a strip");
    let reserve = strip.cols;
    let stride = action_stride(Some(strip));
    let mut list: Vec<&StripButton> = buttons.iter().collect();
    let n = list.len() as i64;
    let base = if rail {
        let width = g.card_x2;
        // kept as layout.lua spells it: last glyph's own column, then one clear of the edge
        #[allow(clippy::int_plus_one)]
        let fits = reserve == 0 && width >= 9 && 2 + stride * (n - 1) + 1 <= width - 1;
        if fits {
            2
        } else {
            list.truncate(1);
            (f64::from(width as i32) / 2.0).ceil() as i64
        }
    } else if reserve > 0 {
        // two clear of the last light, whatever the cell width made the reserve
        reserve + 2
    } else if cfg.position == "right" {
        g.card_x1
    } else {
        // no lights to sit beside: the trailing edge is the toolbar convention
        g.card_x2 - 1 - stride * (n - 1)
    };
    let mut out = Vec::new();
    for (i, action) in list.iter().enumerate() {
        let x = base + stride * i as i64;
        // the span is exactly one stride wide, so neighbours stay contiguous with no dead cell
        let x1 = x - (stride - 1) / 2;
        let x2 = x1 + stride - 1;
        if x1 >= 1 && x2 <= cols {
            out.push(Action {
                id: action.id.clone(),
                icon: action.icon.clone(),
                x,
                x1,
                x2,
            });
        }
    }
    (out, Some(row))
}

/// One switcher slot: the glyph, its column, its click span, and the space a click on it targets.
#[derive(Debug, Clone)]
pub struct SpaceSlot {
    pub id: String,
    pub icon: String,
    pub x: i64,
    pub x1: i64,
    pub x2: i64,
    pub active: bool,
    pub unseen: bool,
    /// The list continues past this end of the window, so the glyph paints faded.
    pub cut: bool,
}

/// The space `delta` steps from the active one, or None at either end: the switcher never wraps.
pub fn space_neighbour(ids: &[&str], active: Option<usize>, delta: i64) -> Option<String> {
    let at = active? as i64 + delta;
    if at < 0 || at >= ids.len() as i64 {
        return None;
    }
    Some(ids[at as usize].to_string())
}

/// One row of space icons at the foot, plus a blank row above it when the pane can spare one; the
/// list keeps the same 3-row floor the ghost card respects.
fn spaces_rows(room: i64, n: usize) -> i64 {
    if n == 0 {
        0
    } else if room >= 5 {
        2
    } else if room >= 3 {
        1
    } else {
        0
    }
}

/// The switcher's slots, centred in the card span on the strip's own pitch. Past the width, a
/// window of slots around the active one, its cut ends faded; a lone slot is a cycle button.
fn space_slots(spaces: &[SpaceEntry], g: &Grid, stride: i64, cols: i64) -> Vec<SpaceSlot> {
    let n = spaces.len() as i64;
    if n == 0 || stride < 1 {
        return Vec::new();
    }
    let card_w = g.card_x2 - g.card_x1 + 1;
    let max_n = ((card_w - 3) / stride + 1).clamp(1, n);
    let active = spaces.iter().position(|s| s.is_active);
    let from = if max_n >= n {
        0
    } else {
        (active.unwrap_or(0) as i64 - max_n / 2).clamp(0, n - max_n)
    };
    let first_x1 = g.card_x1 + (card_w - max_n * stride) / 2;
    let mut out = Vec::new();
    for i in 0..max_n {
        let at = (from + i) as usize;
        let space = &spaces[at];
        let x1 = first_x1 + stride * i;
        let x2 = x1 + stride - 1;
        if x1 < 1 || x2 > cols {
            continue;
        }
        let target = if max_n == 1 && n > 1 {
            spaces[(at + 1) % n as usize].id.clone()
        } else {
            space.id.clone()
        };
        out.push(SpaceSlot {
            id: target,
            icon: space.icon.clone(),
            x: x1 + (stride - 1) / 2,
            x1,
            x2,
            active: space.is_active,
            unseen: space.has_unseen,
            cut: (i == 0 && from > 0) || (i == max_n - 1 && from + max_n < n),
        });
    }
    out
}

/// A list item as the plan sees it: drag may reseat a copy at another slot with a different pinning.
#[derive(Debug, Clone, Copy)]
pub struct LItem<'a> {
    pub item: &'a Item,
    pub is_pinned: bool,
    pub armed_pinned: Option<bool>,
}

impl<'a> LItem<'a> {
    fn of(item: &'a Item) -> Self {
        LItem {
            item,
            is_pinned: item.is_pinned,
            armed_pinned: None,
        }
    }
}

/// Reorders items so the dragged one sits at the hovered slot in the pinned-first sequence.
pub fn apply_drag<'a>(items: &'a [Item], drag: Option<&crate::scene::Drag>) -> Vec<LItem<'a>> {
    let all: Vec<LItem<'a>> = items.iter().map(LItem::of).collect();
    let Some(drag) = drag.filter(|d| d.over_index.is_some() && d.active) else {
        return all;
    };
    let over_index = drag.over_index.expect("filtered above");
    let mut dragged = None;
    let mut rest = Vec::new();
    for item in &all {
        if item.item.tab_id == drag.tab_id {
            dragged = Some(*item);
        } else {
            rest.push(*item);
        }
    }
    let Some(dragged) = dragged else { return all };
    let (pinned, unpinned): (Vec<_>, Vec<_>) = rest.iter().partition(|i| i.is_pinned);
    let mut ghost = dragged;
    ghost.is_pinned = over_index <= pinned.len() as i64 + i64::from(dragged.is_pinned);
    ghost.armed_pinned = Some(dragged.is_pinned);
    let target = 1.max(over_index.min(rest.len() as i64 + 1));
    let mut sequence: Vec<LItem<'a>> = pinned.into_iter().chain(unpinned).copied().collect();
    sequence.insert((target - 1) as usize, ghost);
    sequence
}

/// Ghost card height that leaves the list at least 3 rows (2 for the one-row form).
pub fn new_tab_rows(cfg: &RenderCfg, rows: i64, strip_rows: i64, footer_n: i64) -> i64 {
    if !cfg.new_tab_button {
        return 0;
    }
    if rows - strip_rows - 3 - footer_n >= 3 {
        return 3;
    }
    if rows - strip_rows - 1 - footer_n >= 2 {
        return 1;
    }
    0
}

#[derive(Debug, Clone, Copy)]
pub struct St {
    pub hovered: bool,
    pub dragging: bool,
    pub focused: bool,
    pub pointer_x: Option<i64>,
}

fn shows_close(item: &LItem, cfg: &RenderCfg, st: &St) -> bool {
    if cfg.close_button == "never" || st.dragging {
        return false;
    }
    if cfg.close_button == "always" || cfg.hover == "press" {
        return true;
    }
    st.hovered || item.item.is_active
}

#[derive(Debug, Clone, Copy)]
pub struct CardSpan {
    pub id: &'static str,
    pub x1: i64,
    pub x2: i64,
}

/// Sub-target on a card row: the close button, or the pin toggle on a dense pinned entry.
fn card_span(item: &LItem, cfg: &RenderCfg, st: &St, g: &Grid, part: Part) -> Option<CardSpan> {
    g.close_x?;
    let (x1, x2) = (g.close_x1?, g.close_x2?);
    let pin_only = item.is_pinned && cfg.pinned_style != "full";
    if part == Part::Title && pin_only {
        if cfg.pinned_style == "compact" || st.hovered {
            return Some(CardSpan { id: "pin", x1, x2 });
        }
        return None;
    }
    if pin_only || !shows_close(item, cfg, st) {
        return None;
    }
    if part == Part::Title || part == Part::Meta {
        return Some(CardSpan {
            id: "close",
            x1,
            x2,
        });
    }
    None
}

/// Blank card rows above and below the content block, by `tab_height`.
fn pads_for(tab_height: &str) -> i64 {
    match tab_height {
        "row" => 0,
        "tall" => 2,
        _ => 1,
    }
}

/// `tab_height` decides the pads, `meta` decides the content lines.
fn card_rows(item: &LItem, cfg: &RenderCfg) -> (i64, bool, i64, i64) {
    let dense = item.is_pinned && cfg.pinned_style != "full";
    let armed_dense = item.armed_pinned == Some(true) && cfg.pinned_style != "full";
    let one_row = match item.armed_pinned {
        Some(_) => armed_dense,
        None => dense,
    };
    if one_row {
        return (1, dense, 0, 1);
    }
    let pads = pads_for(&cfg.tab_height);
    let content = if cfg.meta { 2 } else { 1 };
    (2 * pads + content, dense, pads, content)
}

/// Rows an unpinned tab owns, gap included: the shortest travel that can land on another slot.
pub fn slot_rows(cfg: &RenderCfg) -> i64 {
    let pads = pads_for(&cfg.tab_height);
    let content = if cfg.meta { 2 } else { 1 };
    2 * pads + content + cfg.row_gap.max(0)
}

/// The card row the title and icon sit on: the middle of the pad/content/pad block.
pub fn icon_row(rows_in_card: i64) -> i64 {
    (f64::from(rows_in_card.max(1) as i32) / 2.0).ceil() as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Pad,
    Title,
    Meta,
    Gap,
}

#[derive(Debug, Clone, Copy)]
enum Entry<'a> {
    Header,
    Space,
    Separator,
    Tab {
        item: LItem<'a>,
        slot: i64,
        part: Part,
        rows_in_card: i64,
        row_in_card: i64,
    },
}

fn at<'e, 'a>(entries: &'e [Entry<'a>], i: i64) -> Option<&'e Entry<'a>> {
    if i >= 1 {
        entries.get((i - 1) as usize)
    } else {
        None
    }
}

// a gap row paints nothing, so fading it would leave the cut edge looking solid
fn paints(entries: &[Entry], i: i64) -> bool {
    !matches!(
        at(entries, i),
        None | Some(Entry::Space)
            | Some(Entry::Tab {
                part: Part::Gap,
                ..
            })
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhostShape {
    Card,
    Rail,
    Row,
}

#[derive(Debug, Clone)]
pub enum RowKind<'a> {
    Strip {
        actions: Vec<Action>,
        lit_id: Option<String>,
        glyph: bool,
    },
    Space,
    Header,
    Separator,
    Card {
        item: LItem<'a>,
        part: Part,
        rows_in_card: i64,
        row_in_card: i64,
        st: St,
        span: Option<CardSpan>,
    },
    Ghost {
        shape: GhostShape,
        index: i64,
        hovered: bool,
    },
    Footer {
        entry: &'a FooterEntry,
        hovered: bool,
    },
    Spaces {
        slots: Vec<SpaceSlot>,
        lit_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct RowSpec<'a> {
    pub kind: RowKind<'a>,
    pub fade: Option<f64>,
    pub thumb: bool,
    pub thumb_lit: bool,
}

impl<'a> RowSpec<'a> {
    fn plain(kind: RowKind<'a>) -> Self {
        RowSpec {
            kind,
            fade: None,
            thumb: false,
            thumb_lit: false,
        }
    }
}

/// Port of hit.lua's `KIND`: what a row answers for when the pointer lands on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    Space,
    Separator,
    Strip,
    Action,
    Tab,
    NewTab,
    Footer,
    Spaces,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub id: String,
    pub x1: i64,
    pub x2: i64,
}

/// One row of the v1 `hits` map. `index` is the footer entry's place in the model's own list,
/// which is how the wire addresses id-less footer closures.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub kind: RegionKind,
    pub id: Option<i64>,
    pub slot: Option<i64>,
    pub part: Option<Part>,
    pub x1: Option<i64>,
    pub x2: Option<i64>,
    pub pinned: bool,
    pub index: Option<i64>,
    pub spans: Vec<Span>,
}

impl Region {
    fn of(kind: RegionKind) -> Self {
        Region {
            kind,
            id: None,
            slot: None,
            part: None,
            x1: None,
            x2: None,
            pinned: false,
            index: None,
            spans: Vec::new(),
        }
    }

    /// `hit.in_card`: true when `x` is on the row's card surface.
    pub fn in_card(&self, x: i64) -> bool {
        matches!((self.x1, self.x2), (Some(a), Some(b)) if x >= a && x <= b)
    }

    /// `hit.span`: first sub-target under `x`.
    pub fn span(&self, x: i64) -> Option<&str> {
        self.spans
            .iter()
            .find(|s| x >= s.x1 && x <= s.x2)
            .map(|s| s.id.as_str())
    }
}

pub struct Plan<'a> {
    pub grid: Grid,
    pub rail: bool,
    pub rows: Vec<Option<RowSpec<'a>>>,
    pub regions: Vec<Region>,
}

impl Plan<'_> {
    /// `hit.at`: the region at row `y`, or bare space off the end.
    pub fn at(&self, y: i64) -> Region {
        if y >= 1 && y <= self.regions.len() as i64 {
            self.regions[(y - 1) as usize].clone()
        } else {
            Region::of(RegionKind::Space)
        }
    }

    /// `hit.drop_slot`: the slot a tab dropped at row `y` would take; a gap row drops below its card.
    pub fn drop_slot(&self, y: i64) -> i64 {
        let here = self.at(y);
        if here.kind == RegionKind::Tab
            && let Some(slot) = here.slot
        {
            return if here.part == Some(Part::Gap) {
                slot + 1
            } else {
                slot
            };
        }
        let mut last_slot = 0;
        for row in 1..=self.regions.len() as i64 {
            let h = self.at(row);
            if h.kind == RegionKind::Tab
                && let Some(slot) = h.slot
            {
                last_slot = last_slot.max(slot);
                if row > y {
                    return slot;
                }
            }
        }
        last_slot + 1
    }
}

/// `hit.on_inner_edge`: the sidebar edge that borders the content pane.
pub fn on_inner_edge(x: i64, cols: i64, position: &str) -> bool {
    if position == "right" {
        x <= 1
    } else {
        x >= cols
    }
}

struct Thumb {
    first: i64,
    len: i64,
    lit: bool,
}

pub fn plan<'a>(view: &'a RenderInput) -> Plan<'a> {
    let (cfg, cols) = (&view.cfg, view.cols);
    let rail = view.rail;
    let g = if rail {
        rail_grid(cols)
    } else {
        grid(cfg, cols)
    };
    let strip_rows = match &view.strip {
        Some(strip) => strip.rows.max(0),
        None => cfg.padding.top.max(0),
    }
    .min(view.rows);
    let footer_n = view.footer.len() as i64;
    // new_tab_rows has to see the shortened pane too, or the ghost claims the rows reserved below
    // and above it: one page row, so the outlined card is as far from the last title as the cards
    // are from each other. The one-row form has no border and needs none.
    let pad_b = cfg
        .padding
        .bottom
        .max(0)
        .min((view.rows - strip_rows).max(0));
    // the switcher outranks the ghost: it is claimed first, so the ghost is what degrades
    let spaces_h = spaces_rows(view.rows - strip_rows - footer_n - pad_b, view.spaces.len());
    let foot_n = footer_n + spaces_h;
    let ghost_h = new_tab_rows(cfg, view.rows - pad_b - 1, strip_rows, foot_n);
    let ghost_gap = i64::from(ghost_h == 3);
    let list_rows = (view.rows - strip_rows - ghost_gap - ghost_h - foot_n - pad_b).max(0);

    let ordered = apply_drag(&view.items, view.drag.as_ref());
    let (pinned, rest): (Vec<LItem<'a>>, Vec<LItem<'a>>) =
        ordered.iter().copied().partition(|i| i.is_pinned);

    let mut entries: Vec<Entry> = Vec::new();
    if view.private {
        entries.push(Entry::Header);
        entries.push(Entry::Space);
    }
    let mut slot = 0;
    let mut push = |entries: &mut Vec<Entry<'a>>, item: LItem<'a>| {
        slot += 1;
        let at_slot = slot;
        let (rows_in_card, dense, pads, content) = card_rows(&item, cfg);
        let mut n = 0;
        let mut row = |entries: &mut Vec<Entry<'a>>, part: Part| {
            n += 1;
            entries.push(Entry::Tab {
                item,
                slot: at_slot,
                part,
                rows_in_card,
                row_in_card: n,
            });
        };
        for _ in 0..pads {
            row(entries, Part::Pad);
        }
        row(entries, Part::Title);
        if content >= 2 {
            row(entries, Part::Meta);
        }
        for _ in 0..pads {
            row(entries, Part::Pad);
        }
        if !dense || item.armed_pinned.is_some() {
            for _ in 0..cfg.row_gap {
                row(entries, Part::Gap);
            }
        }
    };
    for item in &pinned {
        push(&mut entries, *item);
    }
    if !pinned.is_empty() && cfg.separator != "none" {
        entries.push(if cfg.separator == "rule" {
            Entry::Separator
        } else {
            Entry::Space
        });
    }
    for item in &rest {
        push(&mut entries, *item);
    }

    let total = entries.len() as i64;
    let max_scroll = (total - list_rows).max(0);
    let mut scroll = view.scroll.clamp(0, max_scroll);
    if let Some(want) = view.ensure_visible {
        for (idx, entry) in entries.iter().enumerate() {
            let i = idx as i64 + 1;
            if let Entry::Tab {
                item,
                part: Part::Title,
                rows_in_card,
                ..
            } = entry
                && item.item.tab_id == want
            {
                let last = i + rows_in_card - 1;
                if i <= scroll {
                    scroll = i - 1;
                } else if last > scroll + list_rows {
                    scroll = (last - list_rows).min(total - 1);
                }
                break;
            }
        }
    }
    let scroll = scroll.clamp(0, max_scroll);

    let hovered_id = view
        .hover
        .and_then(|h| match at(&entries, h.y - strip_rows + scroll) {
            Some(Entry::Tab { item, .. }) => Some(item.item.tab_id),
            _ => None,
        });

    let fade_first = (1..=list_rows).find(|i| paints(&entries, i + scroll));
    let fade_last = (1..=list_rows).rev().find(|i| paints(&entries, i + scroll));

    let mut thumb = None;
    if cfg.scroll_indicator != "never" && total > list_rows && (!rail || cols >= 7) {
        let len = 1.max(((list_rows * list_rows) as f64 / total as f64).floor() as i64);
        thumb = Some(Thumb {
            first: 1
                + (scroll as f64 * (list_rows - len) as f64 / max_scroll.max(1) as f64 + 0.5)
                    .floor() as i64,
            len,
            lit: cfg.scroll_indicator == "always"
                || view.hover.is_some()
                || view.drag.is_some_and(|d| d.active)
                || view.user_scrolled,
        });
    }

    let mut rows: Vec<Option<RowSpec>> = vec![None; view.rows.max(0) as usize];
    let mut regions: Vec<Region> = vec![Region::of(RegionKind::Space); view.rows.max(0) as usize];
    let set = |rows: &mut Vec<Option<RowSpec<'a>>>, row: i64, spec: RowSpec<'a>| {
        if row >= 1 && row <= view.rows {
            rows[(row - 1) as usize] = Some(spec);
        }
    };
    let mark = |regions: &mut Vec<Region>, row: i64, region: Region| {
        if row >= 1 && row <= view.rows {
            regions[(row - 1) as usize] = region;
        }
    };

    let (actions, action_row) = strip_actions(
        &view.strip_buttons,
        cfg,
        view.strip.as_ref(),
        &g,
        rail,
        cols,
    );
    // two rows tall, so the target stays comfortable wherever the lights' centre lands
    let band_last = action_row.map_or(0, |r| (r + 1).min(strip_rows));
    let mut lit_id = None;
    if let (Some(hover), Some(first)) = (view.hover, action_row)
        && hover.y >= first
        && hover.y <= band_last
    {
        for action in &actions {
            if hover.x >= action.x1 && hover.x <= action.x2 {
                lit_id = Some(action.id.clone());
            }
        }
    }
    for row in 1..=strip_rows {
        let in_band =
            !actions.is_empty() && action_row.is_some_and(|r| row >= r && row <= band_last);
        set(
            &mut rows,
            row,
            RowSpec::plain(RowKind::Strip {
                actions: if in_band { actions.clone() } else { Vec::new() },
                lit_id: if in_band { lit_id.clone() } else { None },
                glyph: action_row == Some(row),
            }),
        );
        let region = if in_band {
            Region {
                x1: Some(actions[0].x1),
                x2: Some(actions[actions.len() - 1].x2),
                spans: actions
                    .iter()
                    .map(|a| Span {
                        id: a.id.clone(),
                        x1: a.x1,
                        x2: a.x2,
                    })
                    .collect(),
                ..Region::of(RegionKind::Action)
            }
        } else {
            Region::of(RegionKind::Strip)
        };
        mark(&mut regions, row, region);
    }

    for i in 1..=list_rows {
        let row = strip_rows + i;
        let fade = if (Some(i) == fade_first && scroll > 0)
            || (Some(i) == fade_last && scroll < max_scroll)
        {
            Some(0.5)
        } else {
            None
        };
        let thumb_here = thumb
            .as_ref()
            .is_some_and(|t| i >= t.first && i < t.first + t.len);
        let thumb_lit = thumb_here && thumb.as_ref().is_some_and(|t| t.lit);
        let mut region = Region::of(RegionKind::Space);
        let kind = match at(&entries, i + scroll) {
            None | Some(Entry::Space) => RowKind::Space,
            Some(Entry::Header) => RowKind::Header,
            Some(Entry::Separator) => {
                region = Region::of(RegionKind::Separator);
                RowKind::Separator
            }
            Some(Entry::Tab {
                item,
                slot,
                part,
                rows_in_card,
                row_in_card,
            }) => {
                let st = St {
                    hovered: hovered_id == Some(item.item.tab_id),
                    dragging: view
                        .drag
                        .is_some_and(|d| d.active && d.tab_id == item.item.tab_id),
                    focused: view.focus_index == Some(*slot),
                    pointer_x: view.hover.map(|h| h.x),
                };
                let span = card_span(item, cfg, &st, &g, *part);
                region = Region {
                    id: Some(item.item.tab_id),
                    slot: Some(*slot),
                    part: Some(*part),
                    x1: Some(g.card_x1),
                    x2: Some(g.card_x2),
                    pinned: item.item.is_pinned,
                    spans: span
                        .iter()
                        .map(|s| Span {
                            id: s.id.to_string(),
                            x1: s.x1,
                            x2: s.x2,
                        })
                        .collect(),
                    ..Region::of(RegionKind::Tab)
                };
                RowKind::Card {
                    item: *item,
                    part: *part,
                    rows_in_card: *rows_in_card,
                    row_in_card: *row_in_card,
                    st,
                    span,
                }
            }
        };
        set(
            &mut rows,
            row,
            RowSpec {
                kind,
                fade,
                thumb: thumb_here,
                thumb_lit,
            },
        );
        mark(&mut regions, row, region);
    }

    if ghost_gap > 0 {
        let row = strip_rows + list_rows + 1;
        set(&mut rows, row, RowSpec::plain(RowKind::Space));
        mark(&mut regions, row, Region::of(RegionKind::Space));
    }
    if ghost_h > 0 {
        let base = strip_rows + list_rows + ghost_gap;
        let hovered = view
            .hover
            .is_some_and(|h| h.y > base && h.y <= base + ghost_h);
        // the rail draws the same outlined card; only a window too short falls back to a bare glyph
        let shape = if ghost_h == 3 {
            GhostShape::Card
        } else if rail {
            GhostShape::Rail
        } else {
            GhostShape::Row
        };
        for i in 1..=ghost_h {
            set(
                &mut rows,
                base + i,
                RowSpec::plain(RowKind::Ghost {
                    shape,
                    index: i,
                    hovered,
                }),
            );
            mark(
                &mut regions,
                base + i,
                Region {
                    x1: Some(g.card_x1),
                    x2: Some(g.card_x2),
                    ..Region::of(RegionKind::NewTab)
                },
            );
        }
    }

    // painted, not skipped: an unpainted row keeps whatever the pane had there before
    for i in 1..=pad_b {
        let row = view.rows - pad_b + i;
        set(&mut rows, row, RowSpec::plain(RowKind::Space));
        mark(&mut regions, row, Region::of(RegionKind::Space));
    }

    for (idx, entry) in view.footer.iter().enumerate() {
        let row = view.rows - pad_b - spaces_h - footer_n + idx as i64 + 1;
        let hovered = view.hover.is_some_and(|h| h.y == row) && entry.id.is_some();
        set(
            &mut rows,
            row,
            RowSpec::plain(RowKind::Footer { entry, hovered }),
        );
        mark(
            &mut regions,
            row,
            Region {
                x1: Some(g.card_x1),
                x2: Some(g.card_x2),
                index: Some(idx as i64 + 1),
                ..Region::of(RegionKind::Footer)
            },
        );
    }

    if spaces_h > 0 {
        let row = view.rows - pad_b;
        if spaces_h == 2 {
            set(&mut rows, row - 1, RowSpec::plain(RowKind::Space));
            mark(&mut regions, row - 1, Region::of(RegionKind::Space));
        }
        let slots = space_slots(&view.spaces, &g, action_stride(view.strip.as_ref()), cols);
        let lit_id = view
            .hover
            .filter(|h| h.y == row)
            .and_then(|h| slots.iter().find(|s| h.x >= s.x1 && h.x <= s.x2))
            .map(|s| s.id.clone());
        let region = match (slots.first(), slots.last()) {
            (Some(first), Some(last)) => Region {
                x1: Some(first.x1),
                x2: Some(last.x2),
                spans: slots
                    .iter()
                    .map(|s| Span {
                        id: s.id.clone(),
                        x1: s.x1,
                        x2: s.x2,
                    })
                    .collect(),
                ..Region::of(RegionKind::Spaces)
            },
            _ => Region::of(RegionKind::Spaces),
        };
        set(
            &mut rows,
            row,
            RowSpec::plain(RowKind::Spaces { slots, lit_id }),
        );
        mark(&mut regions, row, region);
    }

    Plan {
        grid: g,
        rail,
        rows,
        regions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spaces(n: usize, active: usize) -> Vec<SpaceEntry> {
        (0..n)
            .map(|i| SpaceEntry {
                id: format!("s{i}"),
                name: format!("s{i}"),
                icon: i.to_string(),
                is_active: i == active,
                has_unseen: false,
            })
            .collect()
    }

    fn columns(slots: &[SpaceSlot]) -> Vec<i64> {
        slots.iter().map(|s| s.x).collect()
    }

    #[test]
    fn the_switcher_takes_two_rows_when_the_pane_can_spare_them_and_none_without_spaces() {
        assert_eq!(spaces_rows(20, 3), 2);
        assert_eq!(
            spaces_rows(5, 3),
            2,
            "gap, icons and a 3-row list still fit"
        );
        assert_eq!(spaces_rows(4, 3), 1, "the gap goes first");
        assert_eq!(spaces_rows(3, 3), 1);
        assert_eq!(spaces_rows(2, 3), 0, "then the row itself");
        assert_eq!(spaces_rows(20, 0), 0);
    }

    #[test]
    fn slots_sit_centred_on_the_strip_pitch_and_their_spans_touch() {
        let slots = space_slots(&spaces(3, 1), &rail_grid(9), 3, 9);
        assert_eq!(columns(&slots), vec![2, 5, 8]);
        assert_eq!((slots[0].x1, slots[0].x2), (1, 3));
        assert_eq!((slots[2].x1, slots[2].x2), (7, 9));
        assert!(slots[1].active && !slots[0].active);
        assert!(
            slots.iter().all(|s| !s.cut),
            "nothing hidden, nothing faded"
        );
        let two = space_slots(&spaces(2, 0), &rail_grid(9), 3, 9);
        assert_eq!(
            columns(&two),
            vec![3, 6],
            "two icons centre between the same edges"
        );
    }

    #[test]
    fn past_the_width_a_window_around_the_active_space_shows_with_faded_cut_ends() {
        let late = space_slots(&spaces(5, 3), &rail_grid(9), 3, 9);
        let ids: Vec<&str> = late.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["s2", "s3", "s4"]);
        assert_eq!(
            late.iter().map(|s| s.cut).collect::<Vec<_>>(),
            vec![true, false, false],
            "the list continues to the left only"
        );
        let early = space_slots(&spaces(5, 0), &rail_grid(9), 3, 9);
        let ids: Vec<&str> = early.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["s0", "s1", "s2"]);
        assert!(early[2].cut && !early[0].cut);
    }

    #[test]
    fn a_lone_slot_shows_the_active_space_and_targets_the_next_one() {
        let slots = space_slots(&spaces(3, 1), &rail_grid(5), 3, 5);
        assert_eq!(slots.len(), 1, "a five-column rail holds one icon");
        assert_eq!(slots[0].x, 3);
        assert_eq!((slots[0].icon.as_str(), slots[0].active), ("1", true));
        assert_eq!(slots[0].id, "s2", "a click steps on");
        let last = space_slots(&spaces(3, 2), &rail_grid(5), 3, 5);
        assert_eq!(
            last[0].id, "s0",
            "and wraps, or the last space would be a dead end"
        );
        let only = space_slots(&spaces(1, 0), &rail_grid(5), 3, 5);
        assert_eq!(
            only[0].id, "s0",
            "with one space there is nowhere else to go"
        );
    }

    #[test]
    fn the_neighbour_stops_at_the_ends_and_needs_an_active_space() {
        let ids = ["a", "b", "c"];
        assert_eq!(space_neighbour(&ids, Some(1), 1).as_deref(), Some("c"));
        assert_eq!(space_neighbour(&ids, Some(1), -1).as_deref(), Some("a"));
        assert_eq!(space_neighbour(&ids, Some(2), 1), None);
        assert_eq!(space_neighbour(&ids, Some(0), -1), None);
        assert_eq!(space_neighbour(&ids, None, 1), None);
    }
}

//! Port of input.lua's gesture semantics: `on_down`/`on_drag`/`on_up`/`on_wheel`/`hover_moved`
//! and the focus-mode keymap. Pure — it reads the plan and the pointer state and returns the
//! state that follows plus the events to emit.

use vtabs_core::ui::{ArmKind, Armed, Ms, PressDrag, UiState};
use vtabs_protocol::types::{Button, Mods, Mouse, MouseKind};
use vtabs_protocol::{DoId, Event};
use vtabs_view::enrich::PopoverHits;
use vtabs_view::layout::{Part, Plan, RegionKind, on_inner_edge};

pub const DRAG_START_ROWS: i64 = 3;
pub const DRAG_START_COLS: i64 = 2;
pub const DRAG_DWELL_MS: u64 = 120;
pub const TEAR_OFF_TRAVEL: i64 = 3;

/// The mirrored `model.drag`: the press may have landed in another backend process (§1.4).
/// It carries no comparable `began` — `origin.at` is Lua's clock, not this process's — so a drag
/// this process did not start is gated on travel alone; its dwell was served where it began.
#[derive(Debug, Clone, Copy)]
pub struct MirroredDrag {
    pub id: i64,
    pub active: bool,
    pub origin_x: i64,
    pub origin_y: i64,
    pub outside: bool,
}

/// The config knobs the gestures branch on, resolved once per frame by the runtime.
pub struct Knobs<'a> {
    pub cols: i64,
    pub position: &'a str,
    pub double_click_ms: u64,
    pub tear_off: bool,
    pub wheel: &'a str,
    pub context: &'a str,
    pub hover_mode: &'a str,
    pub slot_rows: i64,
    pub focus_on: bool,
    pub focus_index: i64,
    /// `model.ordered` ids: pinned first, then the rest, which is what the digit keys count.
    pub ordered: &'a [i64],
    pub drag: Option<MirroredDrag>,
    pub scroll_top: i64,
}

#[derive(Debug, Default)]
pub struct Resolution {
    pub ui: UiState,
    pub events: Vec<Event>,
    pub repaint: bool,
}

fn button_name(b: Button) -> &'static str {
    match b {
        Button::Left => "left",
        Button::Middle => "middle",
        Button::Right => "right",
        Button::None => "none",
    }
}

fn part_name(part: Option<Part>) -> Option<&'static str> {
    match part? {
        Part::Pad => Some("pad"),
        Part::Title => Some("title"),
        Part::Meta => Some("meta"),
        Part::Gap => Some("gap"),
    }
}

/// The popover owns the whole sidebar while it is open; Rust reports, Lua decides (P4b bridge).
fn popover_mouse(pop: &PopoverHits, m: &Mouse, k: &'static str) -> Event {
    let (x, y) = (i64::from(m.x), i64::from(m.y));
    let covers = pop.covers(y);
    let row = pop.row_at(y);
    Event::do_("popover_mouse").with(|a| {
        a.k = Some(k);
        a.b = Some(button_name(m.button));
        a.x = Some(x);
        a.y = Some(y);
        a.kind = Some(if covers { "popover" } else { "scrim" });
        a.id = row.and_then(|(id, _)| id.clone());
        a.disabled = row.map(|(_, disabled)| *disabled);
        a.inside = Some(covers && pop.inside(x));
    })
}

/// `hover_moved`: motion only needs a repaint when it crosses a row or a span of the row it is on.
fn hover_moved(ui: &UiState, plan: &Plan, x: i64, y: i64) -> bool {
    let Some(prev) = ui.hover else { return true };
    if prev.y != y {
        return true;
    }
    let region = plan.at(y);
    region.span(prev.x) != region.span(x)
}

pub fn mouse(
    plan: &Plan,
    pop: Option<&PopoverHits>,
    k: &Knobs,
    ui: &UiState,
    m: &Mouse,
    now: Ms,
) -> Resolution {
    let mut out = Resolution {
        ui: ui.clone(),
        ..Default::default()
    };
    let (x, y) = (i64::from(m.x), i64::from(m.y));

    if let Some(pop) = pop {
        match m.kind {
            MouseKind::Wheel => {
                out.events
                    .push(Event::do_("popover_wheel").with(|a| a.dy = Some(i64::from(m.dy))));
            }
            MouseKind::Press => out.events.push(popover_mouse(pop, m, "down")),
            MouseKind::Release => out.events.push(popover_mouse(pop, m, "up")),
            MouseKind::Move | MouseKind::Drag => {
                out.ui.set_hover(x, y, now);
                out.events.push(popover_mouse(pop, m, "move"));
                out.repaint = true;
            }
        }
        return out;
    }

    match m.kind {
        MouseKind::Move => {
            let moved = hover_moved(ui, plan, x, y);
            out.ui.set_hover(x, y, now);
            out.repaint = moved;
        }
        MouseKind::Press => on_down(plan, k, &mut out, m, x, y, now),
        MouseKind::Drag => on_drag(plan, k, &mut out, m, x, y, now),
        MouseKind::Release => on_up(plan, k, &mut out, m, x, y),
        MouseKind::Wheel => on_wheel(k, &mut out, i64::from(m.dy)),
    }
    out
}

fn on_down(plan: &Plan, k: &Knobs, out: &mut Resolution, m: &Mouse, x: i64, y: i64, now: Ms) {
    let region = plan.at(y);
    out.ui.set_hover(x, y, now);
    out.ui.drag = None;
    out.ui.armed = None;
    out.repaint = true;

    let on_card = region.kind == RegionKind::Tab && region.in_card(x);
    let target = match (on_card, region.id) {
        (true, Some(id)) => format!("tab:{id}"),
        _ => kind_key(region.kind).to_string(),
    };
    let double = out.ui.double_click(&target, now, k.double_click_ms);
    let id = region.id.unwrap_or_default();

    match m.button {
        Button::Left => {
            if on_card {
                match region.span(x) {
                    Some("close") => {
                        out.ui.armed = Some(Armed {
                            kind: ArmKind::Close,
                            tab_id: id,
                            row: y,
                            col: x,
                            at: now,
                        })
                    }
                    Some("pin") => out.events.push(Event::do_tab("toggle_pin", id)),
                    _ => {
                        out.events.push(Event::do_tab("press_card", id).with(|a| {
                            a.x = Some(x);
                            a.y = Some(y);
                            a.part = part_name(region.part);
                        }));
                        out.ui.drag = Some(PressDrag {
                            tab_id: id,
                            origin_x: x,
                            origin_y: y,
                            began: now,
                            active: false,
                        });
                    }
                }
            } else if region.kind == RegionKind::Action && region.in_card(x) {
                if let Some(button) = region.span(x) {
                    out.events.push(Event::Do {
                        a: "strip",
                        id: Some(DoId::Name(button.to_string())),
                        args: Default::default(),
                    });
                }
            } else if region.kind == RegionKind::NewTab {
                out.events.push(Event::do_("new_tab"));
            } else if region.kind == RegionKind::Footer {
                if let Some(index) = region.index {
                    out.events
                        .push(Event::do_("footer").with(|a| a.index = Some(index)));
                }
            } else if double
                && matches!(region.kind, RegionKind::Space | RegionKind::Strip) | !on_card
            {
                out.events.push(Event::do_("new_tab"));
            }
        }
        // the ✕ and the middle click both act on the release, and for the same reason
        Button::Middle if on_card => {
            out.ui.armed = Some(Armed {
                kind: ArmKind::Close,
                tab_id: id,
                row: y,
                col: x,
                at: now,
            })
        }
        Button::Right if on_card && k.context == "popover" => {
            out.ui.armed = Some(Armed {
                kind: ArmKind::Menu,
                tab_id: id,
                row: y,
                col: x,
                at: now,
            })
        }
        _ => {}
    }
}

fn kind_key(kind: RegionKind) -> &'static str {
    match kind {
        RegionKind::Space => "space",
        RegionKind::Separator => "separator",
        RegionKind::Strip => "strip",
        RegionKind::Action => "action",
        RegionKind::Tab => "tab",
        RegionKind::NewTab => "new_tab",
        RegionKind::Footer => "footer",
    }
}

/// The press that armed this drag may live in another process, so the mirror is the fallback.
/// `None` for the dwell means there is no local clock to measure it against.
fn pending_drag(k: &Knobs, ui: &UiState) -> Option<(i64, i64, i64, Option<Ms>, bool)> {
    if let Some(d) = ui.drag {
        return Some((d.tab_id, d.origin_x, d.origin_y, Some(d.began), d.active));
    }
    k.drag
        .map(|d| (d.id, d.origin_x, d.origin_y, None, d.active))
}

fn on_drag(plan: &Plan, k: &Knobs, out: &mut Resolution, m: &Mouse, x: i64, y: i64, now: Ms) {
    // Motion cancels an armed close: wezterm drops the capture on release, so a release over the
    // content pane still arrives with translated coordinates that could land back on the ✕.
    out.ui.armed = None;
    if m.button != Button::Left {
        return;
    }
    let Some((tab_id, ox, oy, began, was_active)) = pending_drag(k, ui_of(out)) else {
        return;
    };
    let (dx, dy) = ((x - ox).abs(), (y - oy).abs());
    // a short card must not put its neighbour out of reach: never ask for more than one slot
    let rows_needed = 2.max(DRAG_START_ROWS.min(k.slot_rows - 1));
    let past = dy >= rows_needed || dx >= DRAG_START_COLS;
    let dwelt = began.is_none_or(|b| now.saturating_sub(b) >= DRAG_DWELL_MS);
    let active = was_active || (past && dwelt);
    if active {
        let slot = plan.drop_slot(y);
        let outside = k.tear_off && dx >= TEAR_OFF_TRAVEL && on_inner_edge(x, k.cols, k.position);
        out.events.push(Event::do_("drag_to").with(|a| {
            a.x = Some(x);
            a.y = Some(y);
            a.slot = Some(slot);
            a.outside = Some(outside);
        }));
    }
    if let Some(d) = out.ui.drag.as_mut() {
        d.active = active;
    } else if active {
        out.ui.drag = Some(PressDrag {
            tab_id,
            origin_x: ox,
            origin_y: oy,
            began: began.unwrap_or(now),
            active,
        });
    }
    out.ui.set_hover(x, y, now);
    out.repaint = true;
}

fn ui_of(out: &Resolution) -> &UiState {
    &out.ui
}

/// `released_on`: a press on the ✕ or a middle click closes only when the release lands there too.
fn released_on(plan: &Plan, m: &Mouse, x: i64, y: i64, armed: &Armed) -> bool {
    let region = plan.at(y);
    if region.kind != RegionKind::Tab || region.id != Some(armed.tab_id) || !region.in_card(x) {
        return false;
    }
    m.button == Button::Middle || region.span(x) == Some("close")
}

fn on_up(plan: &Plan, k: &Knobs, out: &mut Resolution, m: &Mouse, x: i64, y: i64) {
    let armed = out.ui.armed.take();
    let held = out.ui.drag.take();
    let mirrored = k.drag;
    out.repaint = true;

    if m.button == Button::Right {
        if let Some(a) = armed.filter(|a| a.kind == ArmKind::Menu) {
            out.events
                .push(Event::do_tab("open_menu", a.tab_id).with(|args| {
                    args.row = Some(a.row);
                    args.col = Some(a.col);
                }));
        }
        return;
    }
    if let Some(a) = armed.filter(|a| a.kind == ArmKind::Close)
        && released_on(plan, m, x, y, &a)
    {
        out.events
            .push(Event::do_tab("request_close", a.tab_id).with(|args| {
                args.row = Some(a.row);
                args.col = Some(a.col);
            }));
        return;
    }
    // the tear-off arming lives wherever the drag was last tracked: here, or in the mirror
    let outside_of = |d: &PressDrag| {
        mirrored
            .filter(|m| m.id == d.tab_id)
            .is_some_and(|m| m.outside)
    };
    let active = held
        .map(|d| (d.active, d.origin_x, outside_of(&d)))
        .or_else(|| mirrored.map(|d| (d.active, d.origin_x, d.outside)));
    if let Some((true, origin_x, armed_outside)) = active {
        let travelled = (x - origin_x).abs() >= TEAR_OFF_TRAVEL;
        let outside =
            armed_outside || (k.tear_off && travelled && on_inner_edge(x, k.cols, k.position));
        let slot = plan.drop_slot(y);
        out.events.push(Event::do_("drag_end").with(|a| {
            a.outside = Some(outside);
            if !outside {
                a.slot = Some(slot);
            }
        }));
        return;
    }
    if k.hover_mode == "press" {
        out.events.push(Event::do_("blur_sidebar"));
    }
}

fn on_wheel(k: &Knobs, out: &mut Resolution, dy: i64) {
    if k.wheel == "switch" {
        out.events
            .push(Event::do_("wheel_tab").with(|a| a.dy = Some(dy)));
        return;
    }
    // the list moves now and the model catches up; layout clamps, so no bound is applied here
    let top = out.ui.scroll.unwrap_or(k.scroll_top) + dy;
    out.ui.scroll = Some(top);
    out.ui.user_scrolled = true;
    out.repaint = true;
    out.events.push(Event::do_("set_scroll").with(|a| {
        a.top = Some(top);
        a.user = Some(true);
    }));
}

const MOVE: &[(&str, i64)] = &[("down", 1), ("j", 1), ("tab", 1), ("up", -1), ("k", -1)];

/// Focus-mode keys Rust owns. `r`/`J`/`K` have no `do` verb, so they fall through to Lua's own
/// focus branch as plain key events rather than being dropped.
pub fn key(k: &Knobs, name: &str, mods: Mods, raw: &[u8]) -> Resolution {
    let mut out = Resolution::default();
    let forward = |out: &mut Resolution| {
        out.events.push(Event::key(name.to_string(), mods, raw));
    };
    if !k.focus_on {
        forward(&mut out);
        return out;
    }
    let count = (k.ordered.len() as i64).max(1);
    let index = k.focus_index.clamp(1, count);
    let focus_to = |out: &mut Resolution, i: i64| {
        out.events
            .push(Event::do_("set_focus_index").with(|a| a.index = Some(i)));
    };
    let of = |i: i64| k.ordered.get((i - 1).max(0) as usize).copied();

    if name == "escape" || name == "q" || (mods.ctrl && name == "c") {
        out.events.push(Event::do_("blur_sidebar"));
    } else if name == "tab" && mods.shift {
        focus_to(&mut out, (index - 1).max(1));
    } else if let Some((_, delta)) = MOVE.iter().find(|(key, _)| *key == name) {
        focus_to(&mut out, (index + delta).clamp(1, count));
    } else if name == "home" || name == "g" {
        focus_to(&mut out, 1);
    } else if name == "end" || name == "G" {
        focus_to(&mut out, count);
    } else if let Some(n) = name.parse::<i64>().ok().filter(|n| (1..=9).contains(n)) {
        if let Some(id) = of(n) {
            out.events.push(Event::do_tab("activate_tab_by_id", id));
        }
    } else if name == "enter" || name == "space" {
        if let Some(id) = of(index) {
            out.events.push(Event::do_tab("activate_tab_by_id", id));
        }
    } else if name == "x" || name == "d" || name == "delete" {
        if let Some(id) = of(index) {
            out.events
                .push(Event::do_tab("request_close", id).with(|a| a.row = Some(index)));
        }
    } else if name == "p" {
        if let Some(id) = of(index) {
            out.events.push(Event::do_tab("toggle_pin", id));
        }
    } else if name == "m" {
        if let Some(id) = of(index) {
            out.events
                .push(Event::do_tab("open_menu", id).with(|a| a.row = Some(index)));
        }
    } else if name == "n" {
        out.events.push(Event::do_("new_tab"));
    } else {
        forward(&mut out);
    }
    out
}

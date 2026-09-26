use crate::components::row::{Content, Row, RowLayout, Trailing, trailing_control};
use crate::components::text_input::TextInput;
use crate::components::{DROP_TINT, ICON_CELLS, NESTED_INSET, TRAILING_CELLS};
use crate::element::ElementId;
use crate::icons;
use crate::input::{TextEditor, display_text};
use crate::runtime::canvas::{Canvas, RoundedSurface};
use crate::views::sidebar::{Sidebar, View};
use crate::views::{spans_machines, tab_machine, tab_name};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use vtabs_core::{Tab, TabId, TabPane};

const OCCLUDER_INSET: f32 = 6.0;

const MIN_SEGMENT_CELLS: u16 = 4;

#[derive(Clone, Debug)]
pub(crate) struct InlineRename {
    pub id: TabId,
    pub initial: String,
    pub editor: TextEditor,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PaneSlot {
    pub pane: usize,
    pub x: u16,
    pub width: u16,
    pub line: u16,
    pub tall: bool,
}

pub(crate) fn pane_slots(panes: &[TabPane], width: u16, two_lines: bool) -> Vec<PaneSlot> {
    let measured = panes.iter().any(|pane| pane.width > 0 && pane.height > 0);
    let mut lefts: Vec<u16> = if measured {
        panes.iter().map(|pane| pane.left).collect()
    } else {
        (0..panes.len() as u16).collect()
    };
    lefts.sort_unstable();
    lefts.dedup();
    let columns = lefts.len().max(1) as u16;
    let edge = |column: u16| (u32::from(column) * u32::from(width) / u32::from(columns)) as u16;
    let bottom = panes
        .iter()
        .map(|pane| u32::from(pane.top) + u32::from(pane.height))
        .max()
        .unwrap_or(0);
    let mut placed: Vec<(usize, u16, u16, u16, bool)> = panes
        .iter()
        .enumerate()
        .map(|(at, pane)| {
            let (left, right) = if measured {
                (pane.left, pane.left.saturating_add(pane.width))
            } else {
                (at as u16, at as u16 + 1)
            };
            let start = lefts.iter().position(|edge| *edge == left).unwrap_or(0) as u16;
            let end = (lefts.iter().filter(|edge| **edge < right).count() as u16).max(start + 1);
            let (top, height) = (u32::from(pane.top), u32::from(pane.height));
            let tall =
                !two_lines || !measured || (top * 4 <= bottom && (top + height) * 4 >= bottom * 3);
            let line = u16::from(!tall && top * 2 + height >= bottom);
            (at, start, end, line, tall)
        })
        .collect();
    placed.sort_by_key(|(at, start, _, line, _)| (*line, *start, panes[*at].top, *at));
    let mut slots = Vec::with_capacity(placed.len());
    let mut rest = placed.as_slice();
    while let Some(&(_, start, _, line, _)) = rest.first() {
        let count = rest
            .iter()
            .take_while(|(_, other, _, other_line, _)| (*other, *other_line) == (start, line))
            .count();
        let (group, remaining) = rest.split_at(count);
        rest = remaining;
        let end = group
            .iter()
            .map(|(_, _, end, _, _)| *end)
            .max()
            .unwrap_or(start + 1);
        let (x, span) = (edge(start), edge(end) - edge(start));
        for (share, &(pane, _, _, line, tall)) in group.iter().enumerate() {
            let share = share as u16;
            let count = count as u16;
            let offset = span * share / count;
            slots.push(PaneSlot {
                pane,
                x: x + offset,
                width: span * (share + 1) / count - offset,
                line,
                tall,
            });
        }
    }
    slots
}

impl Sidebar {
    pub(crate) fn tab(
        &mut self,
        view: &View,
        tab: &Tab,
        number: usize,
        rect: Rect,
        cx: &mut Canvas,
    ) {
        let (model, compact) = (view.model, view.compact);
        let theme = cx.theme;
        let home = model.home.as_deref();
        let rename = self.rename.as_mut().filter(|rename| rename.id == tab.id);
        let renaming = rename.is_some();
        let name = display_text(&tab_name(tab, home).unwrap_or_default());
        let segmented = !compact
            && tab.panes.len() > 1
            && rect.width.saturating_sub(ICON_CELLS + 2)
                >= tab.panes.len() as u16 * MIN_SEGMENT_CELLS;
        let (remote, os) = tab_machine(tab);
        let mut rename = rename;
        let mut rename_field = |cx: &mut Canvas, layout: &RowLayout| {
            if let Some(rename) = rename.as_mut() {
                rename_field(cx, &mut rename.editor, layout);
            }
        };
        let mut panes = |cx: &mut Canvas, layout: &RowLayout| pane_segments(cx, tab, home, layout);
        let id = ElementId::Tab(tab.id);
        let layout = Row {
            indent: if tab.folder_id.is_some() { 2 } else { 0 },
            icon_color: theme.host(remote, os),
            index: (compact || model.settings.show_indexes).then_some(number),
            tooltip: compact.then(|| name.clone()),
            selected: model.selected_tab == Some(tab.id) && !view.props.settings_open,
            compact,
            trailing: model.settings.show_close.then_some(Trailing {
                id: ElementId::CloseTab(tab.id),
                icon: icons::CLOSE,
                tooltip: "Close tab  Cmd+W",
            }),
            ..Row::new(
                id.clone(),
                rect,
                icons::host(remote, os),
                if renaming {
                    Content::Custom(&mut rename_field)
                } else if segmented {
                    Content::Custom(&mut panes)
                } else {
                    Content::Label(&name)
                },
            )
        }
        .render(cx);
        if compact {
            return;
        }
        if cx.aimed(&id) {
            split_preview(cx, &layout);
        }
        if !renaming && !segmented {
            self.title_rects.push((tab.id, layout.content));
        }
        if rect.height >= 3 {
            cx.write(
                Rect::new(layout.content.x, rect.y + 1, layout.content.width, 1),
                display_text(&tab.domain),
                theme.secondary_on(layout.fill),
            );
        }
    }
}

fn rename_field(cx: &mut Canvas, editor: &mut TextEditor, layout: &RowLayout) {
    let content = layout.content;
    let edit = Rect::new(content.x, content.y, content.width, 1);
    TextInput::new(ElementId::Editor, editor, layout.fill)
        .active(true)
        .shift(if content.height == 2 { 0.5 } else { 0.0 })
        .render(edit, cx);
    cx.hit(ElementId::Editor, edit, "Tab title");
}

fn split_preview(cx: &mut Canvas, layout: &RowLayout) {
    let theme = cx.theme;
    let area = layout.content;
    let width = (area.width / 2).max(MIN_SEGMENT_CELLS).min(area.width);
    let rect = Rect::new(area.right() - width, area.y, width, area.height.min(2));
    let progress = cx.pointer.drop_motion.map_or(1.0, |motion| motion.progress);
    cx.clear(rect);
    let fill = theme.tint(layout.fill, DROP_TINT);
    cx.surface(rect, fill, 6.0, NESTED_INSET + (1.0 - progress) * 8.0);
    cx.write(
        Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1),
        format!("{} {}", icons::PLUS, display_text(&cx.pointer.drag_label)),
        theme.accent().bg(fill),
    );
}

fn pane_segments(cx: &mut Canvas, tab: &Tab, home: Option<&str>, layout: &RowLayout) {
    let theme = cx.theme;
    let pointer = cx.pointer;
    let area = layout.content;
    let mixed = spans_machines(tab);
    let slots = pane_slots(&tab.panes, area.width, area.height >= 2);
    let mut stacked: Vec<(u16, u16)> = Vec::new();
    for slot in slots.iter().filter(|slot| !slot.tall) {
        let span = (slot.x, slot.width);
        if area.height == 2 && !stacked.contains(&span) {
            stacked.push(span);
            let rect = Rect::new(area.x + slot.x, area.y, slot.width, 2);
            cx.fill(RoundedSurface {
                stacked: true,
                ..RoundedSurface::new(rect, layout.fill, 0.0, OCCLUDER_INSET)
            });
        }
    }
    for slot in slots {
        let pane = &tab.panes[slot.pane];
        let rect = Rect::new(
            area.x + slot.x,
            area.y + slot.line,
            slot.width,
            if slot.tall { area.height.min(2) } else { 1 },
        );
        let id = ElementId::Pane(tab.id, pane.id);
        let close = ElementId::ClosePane(tab.id, pane.id);
        let hovered = [Some(&id), Some(&close)].contains(&pointer.hovered.as_ref());
        let fill = if hovered && !pointer.dragging {
            let fill = theme.lift(layout.fill, 12);
            let inset = if slot.tall { NESTED_INSET } else { 1.0 };
            cx.surface(rect, fill, 6.0, inset);
            fill
        } else {
            layout.fill
        };
        let ghost = pointer.dragging && pointer.drag.as_ref() == Some(&id);
        let style = if ghost {
            layout.style.fg(theme.lift(theme.background, 30))
        } else if pane.active {
            layout.style
        } else {
            layout.style.fg(theme.muted)
        };
        let pad = u16::from(slot.x > 0);
        let label = display_text(&pane.label(home));
        cx.write(
            Rect::new(rect.x + pad, rect.y, rect.width.saturating_sub(pad + 1), 1),
            if mixed {
                // The machine glyph stands in for the home or root prefix: 󰣇/dotfiles.
                let glyph = icons::host(pane.remote, &pane.os);
                let glyph = match theme.host(pane.remote, &pane.os) {
                    Some(color) if !ghost => Span::styled(glyph, Style::new().fg(color)),
                    _ => Span::raw(glyph),
                };
                let name = label
                    .strip_prefix("~/")
                    .or_else(|| label.strip_prefix('/'))
                    .unwrap_or(&label);
                Line::from(vec![glyph, Span::raw(format!("/{name}"))])
            } else {
                Line::from(label)
            },
            style.bg(fill),
        );
        cx.hit(id, rect, "");
        if hovered && !pointer.dragging && rect.width >= TRAILING_CELLS * 2 {
            trailing_control(
                cx,
                Trailing {
                    id: close,
                    icon: icons::CLOSE,
                    tooltip: "Close split",
                },
                rect,
                style.bg(fill),
            );
        }
    }
}

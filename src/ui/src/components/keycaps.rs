use crate::components::EDGE_INSET;
use crate::runtime::canvas::{Canvas, RoundedSurface};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

const CHORD_GAP: u16 = 2;

fn keys(mut label: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if label.is_empty() {
            return None;
        }
        // A literal plus is a key too: "Ctrl++" ends with one plus keycap.
        let (key, rest) = if label == "+" {
            (label, "")
        } else {
            label.split_once('+').unwrap_or((label, ""))
        };
        label = rest;
        Some(key)
    })
}

fn width(label: &str) -> u16 {
    keys(label).fold(0u16, |width, key| {
        width.saturating_add((key.width() + 2).min(usize::from(u16::MAX)) as u16)
    })
}

/// Keep whole chords together; extra space separates alternative bindings.
pub(crate) fn render(
    labels: &[String],
    area: Rect,
    align: Alignment,
    style: Style,
    cx: &mut Canvas,
) -> u16 {
    if area.is_empty() || labels.is_empty() {
        return 0;
    }
    let mut used = 0;
    let mut visible = 0;
    for (index, label) in labels.iter().enumerate() {
        let next = used + u16::from(index > 0) * CHORD_GAP;
        let next = next.saturating_add(width(label));
        let overflow = if index + 1 < labels.len() {
            CHORD_GAP + 1
        } else {
            0
        };
        if next.saturating_add(overflow) > area.width {
            break;
        }
        used = next;
        visible += 1;
    }
    let overflow = visible < labels.len();
    if overflow {
        used += if visible > 0 { CHORD_GAP + 1 } else { 1 };
    }
    let mut x = match align {
        Alignment::Right => area.right() - used,
        Alignment::Center => area.x + (area.width - used) / 2,
        Alignment::Left => area.x,
    };
    let fill = cx.theme.lift(style.bg.unwrap_or(cx.theme.background), 8);
    for (index, label) in labels.iter().take(visible).enumerate() {
        if index > 0 {
            x += CHORD_GAP;
        }
        for key in keys(label) {
            let width = key.width() as u16 + 2;
            let rect = Rect::new(x, area.y, width, area.height);
            cx.fill(RoundedSurface {
                scale_y: if area.height > 1 { 0.82 } else { 0.95 },
                ..RoundedSurface::new(rect, fill, 4.0, EDGE_INSET)
            });
            cx.write(
                Rect::new(x, area.y, width, 1),
                Line::from(key.to_owned()).alignment(Alignment::Center),
                style.bg(fill),
            );
            x += width;
        }
    }
    if overflow {
        x += u16::from(visible > 0) * CHORD_GAP;
        cx.write(Rect::new(x, area.y, 1, 1), "…", style);
    }
    used
}

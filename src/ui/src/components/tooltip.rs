use crate::components::anchored;
use crate::input::display_text;
use crate::runtime::canvas::{Canvas, HitRegion};
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget, Wrap};
use unicode_width::UnicodeWidthStr;

pub(crate) fn tooltip(cx: &mut Canvas, area: Rect, sidebar: Rect, target: &HitRegion) {
    if area.width < 8 || area.height < 4 {
        return;
    }
    let theme = cx.theme;
    let text = target.tooltip.lines().map(display_text).collect::<Vec<_>>();
    let natural_width = text.iter().map(|line| line.width()).max().unwrap_or(0);
    if natural_width == 0 {
        return;
    }
    let padding = if text.len() == 1 { 1 } else { 2 };
    let width = natural_width
        .min(42)
        .min(usize::from(area.width).saturating_sub(padding * 2))
        .max(1);
    let lines = wrapped_lines(&text, width);
    let height = if lines == 1 { 2 } else { lines + 2 }.min(usize::from(area.height)) as u16;
    let width = (width + padding * 2) as u16;
    let rect = place(cx, area, sidebar, target.rect, width, height);
    cx.clear(rect);
    cx.rounded(rect, theme.card);
    let padding = padding as u16;
    let content = if lines == 1 {
        Rect::new(rect.x + padding, rect.y, rect.width - padding * 2, 1)
    } else {
        Rect::new(
            rect.x + padding,
            rect.y + 1,
            rect.width - padding * 2,
            rect.height - 2,
        )
    };
    let text = text
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let style = if index == 0 {
                theme.base()
            } else {
                theme.muted()
            };
            Line::styled(line, style.bg(theme.card))
        })
        .collect::<Vec<_>>();
    Paragraph::new(text)
        .style(theme.base().bg(theme.card))
        .wrap(Wrap { trim: true })
        .render(content, cx.buf);
}

fn wrapped_lines(text: &[String], width: usize) -> usize {
    let mut lines = 0usize;
    for line in text {
        lines += 1;
        let mut used = 0usize;
        for word in line.split_whitespace() {
            let word_width = word.width();
            if used > 0 && used + 1 + word_width > width {
                lines += 1;
                used = 0;
            }
            used += usize::from(used > 0) + word_width;
            lines += used.saturating_sub(1) / width;
            used = used.saturating_sub(1) % width + 1;
        }
    }
    lines
}

fn place(cx: &Canvas, area: Rect, sidebar: Rect, target: Rect, width: u16, height: u16) -> Rect {
    let beside = if sidebar.x > area.x {
        sidebar.x.checked_sub(width + 1).filter(|x| *x >= area.x)
    } else {
        Some(sidebar.right() + 1).filter(|x| x + width <= area.right())
    };
    match beside.filter(|_| sidebar.contains(target.as_position())) {
        Some(x) => Rect::new(
            x,
            target.y.min(area.bottom().saturating_sub(height)),
            width,
            height,
        ),
        None => cx.clear_of_rows(
            anchored(
                area,
                Position::new(target.x, target.bottom().saturating_sub(1)),
                width,
                height,
            ),
            area,
        ),
    }
}

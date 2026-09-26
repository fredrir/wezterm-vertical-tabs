pub(crate) mod button;
pub(crate) mod dialog;
pub(crate) mod list;
pub(crate) mod menu;
pub(crate) mod palette;
pub(crate) mod row;
pub(crate) mod text_input;
pub(crate) mod tooltip;

use crate::runtime::canvas::Canvas;
use ratatui::layout::{Position, Rect};

pub(crate) const SURFACE_RADIUS: f32 = 9.0;
pub(crate) const ROW_INSET: f32 = 1.5;
pub(crate) const PRESS_INSET: f32 = 3.0;
pub(crate) const NESTED_INSET: f32 = 4.0;
pub(crate) const DROP_TINT: u16 = 24;
pub(crate) const ICON_CELLS: u16 = 3;
pub(crate) const TRAILING_CELLS: u16 = 3;

pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

pub(crate) fn anchored(area: Rect, anchor: Position, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = anchor.x.clamp(area.x, area.right() - width);
    let anchor_y = anchor.y.clamp(area.y, area.bottom() - 1);
    let below = anchor_y + 1;
    let y = if below + height <= area.bottom() {
        below
    } else if anchor_y >= area.y + height {
        anchor_y - height
    } else {
        area.bottom() - height
    };
    Rect::new(x, y, width, height)
}

pub(crate) struct Panel {
    pub rect: Rect,
    pub inner: Rect,
    pub framed: bool,
}

pub(crate) fn panel(cx: &mut Canvas, rect: Rect, area: Rect) -> Panel {
    let rect = cx.clear_of_rows(rect, area);
    cx.clear(rect);
    cx.rounded(rect, cx.theme.background);
    let framed = rect.width >= 4 && rect.height >= 3;
    let inner = if framed {
        Rect::new(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2)
    } else {
        rect
    };
    Panel {
        rect,
        inner,
        framed,
    }
}

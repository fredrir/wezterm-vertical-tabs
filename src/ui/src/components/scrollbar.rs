use crate::element::ElementId;
use crate::runtime::canvas::Canvas;
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Scrollbar {
    pub track: Rect,
    pub thumb: Rect,
    pub visible: usize,
    total: usize,
}

impl Scrollbar {
    pub fn new(track: Rect, visible: usize, total: usize, offset: usize) -> Option<Self> {
        if track.is_empty() || visible == 0 || total <= visible {
            return None;
        }
        let height = (usize::from(track.height) * visible / total)
            .clamp(1, usize::from(track.height.saturating_sub(1).max(1)))
            as u16;
        let max_offset = total - visible;
        let top = offset.min(max_offset) * usize::from(track.height - height) / max_offset;
        Some(Self {
            track,
            thumb: Rect::new(track.x, track.y + top as u16, track.width, height),
            visible,
            total,
        })
    }

    pub fn offset(self, y: u16, grab: u16) -> usize {
        let travel = usize::from(self.track.height - self.thumb.height);
        let top = usize::from(y.saturating_sub(grab).saturating_sub(self.track.y)).min(travel);
        (top * (self.total - self.visible) + travel / 2) / travel.max(1)
    }

    pub fn render(self, cx: &mut Canvas) {
        cx.surface(self.track, cx.theme.card, 3.0, 1.5);
        cx.surface(self.thumb, cx.theme.muted, 3.0, 1.5);
        cx.hit(ElementId::Scrollbar, self.track, "");
    }
}

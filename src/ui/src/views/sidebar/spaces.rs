use crate::components::button::Button;
use crate::components::list;
use crate::element::ElementId;
use crate::icons;
use crate::runtime::canvas::Canvas;
use crate::views::sidebar::{Sidebar, View};
use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

impl Sidebar {
    /// One button per space along `bar`, scrolled to keep the selected space in view.
    pub(crate) fn spaces(
        &mut self,
        view: &View,
        bar: Rect,
        reveal_selection: bool,
        cx: &mut Canvas,
    ) {
        let model = view.model;
        let slots_width = bar.width;
        let slot_width = if view.compact {
            slots_width.max(1)
        } else {
            slots_width.clamp(1, 4)
        };
        let slots = usize::from(slots_width / slot_width);
        self.space_scroll = self
            .space_scroll
            .min(model.spaces.len().saturating_sub(slots));
        self.spaces = bar;
        if slots > 0
            && reveal_selection
            && let Some(index) = model
                .spaces
                .iter()
                .position(|s| s.id == model.selected_space)
        {
            self.space_scroll = list::reveal_rows(self.space_scroll, index, slots);
        }
        for (offset, space) in model
            .spaces
            .iter()
            .skip(self.space_scroll)
            .take(slots)
            .enumerate()
        {
            let rect = Rect::new(
                bar.x + offset as u16 * slot_width,
                bar.y,
                slot_width.min(slots_width),
                2,
            );
            let label = if space.icon.is_empty() {
                space.name.graphemes(true).next().unwrap_or("○")
            } else {
                icons::space(space.icon.trim())
            };
            let label = if label.width() == 1 {
                format!("{label} ")
            } else {
                label.to_owned()
            };
            let activity = self.space_activity.contains(&space.id);
            Button::icon(ElementId::Space(space.id.clone()), &label)
                .tooltip(format!(
                    "{}{}",
                    space.name,
                    if activity { " (activity)" } else { "" }
                ))
                .selected(space.id == model.selected_space)
                .render(rect, cx);
        }
    }
}

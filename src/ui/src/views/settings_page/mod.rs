mod catalog;
mod controls;
mod editing;
mod keys;
mod range;

pub(crate) use range::Range;

use crate::components::button::Button;
use crate::components::scrollbar::Scrollbar;
use crate::components::search_field::{Record, SearchField};
use crate::components::{EDGE_INSET, ICON_CELLS, SURFACE_RADIUS};
use crate::element::ElementId;
use crate::icons;
use crate::input::TextEditor;
use crate::runtime::canvas::Canvas;
use catalog::CATEGORIES;
use editing::Editing;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;
use vtabs_core::Model;

pub(crate) struct SettingsPage {
    pub open: bool,
    pub listed: bool,
    pub category: String,
    pub query: TextEditor,
    pub chord: Option<String>,
    pub selected: usize,
    pub revealed: Option<usize>,
    pub scroll: usize,
    pub max_scroll: usize,
    pub rect: Rect,
    pub scrollbar: Option<Scrollbar>,
    pub editing: Editing,
    /// Where each dropdown value sits, so its menu can open beneath it.
    pub anchors: Vec<(String, Rect)>,
    pub chord_label: fn(&str) -> String,
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self {
            open: false,
            listed: false,
            category: "all".into(),
            query: TextEditor::default(),
            chord: None,
            selected: 0,
            revealed: None,
            scroll: 0,
            max_scroll: 0,
            rect: Rect::default(),
            scrollbar: None,
            editing: Editing::default(),
            anchors: Vec::new(),
            chord_label: crate::keybinds::display_chord,
        }
    }
}

impl SettingsPage {
    /// `opaque` covers whatever lies beneath, when the page shares the sidebar's cells.
    pub fn render(
        &mut self,
        model: &Model,
        area: Rect,
        overlay_open: bool,
        opaque: bool,
        cx: &mut Canvas,
    ) {
        self.scrollbar = None;
        self.anchors.clear();
        if area.is_empty() {
            return;
        }
        self.rect = area;
        if opaque {
            cx.rounded(area, cx.theme.background);
        }
        let area = if area.height >= 24 && area.width >= 48 {
            Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2)
        } else {
            area
        };
        let margin = if area.width >= 72 {
            4
        } else if area.width >= 40 {
            2
        } else {
            u16::from(area.width >= 8)
        };
        let content_width = area.width.saturating_sub(margin * 2).min(104);
        let top = u16::from(area.height >= 8);
        let inner = Rect::new(
            area.x + (area.width - content_width) / 2,
            area.y + top,
            content_width,
            area.height.saturating_sub(top * 2),
        );
        if inner.is_empty() {
            return;
        }
        let line = if inner.height >= 14 && inner.width >= 22 {
            2
        } else {
            1
        };
        let gap = u16::from(inner.height >= 20);
        let footer = if inner.height >= 12 { line } else { 0 };
        let bottom = inner
            .bottom()
            .saturating_sub(footer + if footer > 0 { gap } else { 0 });
        let mut y = inner.y;
        if y + line <= bottom {
            self.search(Rect::new(inner.x, y, inner.width, line), overlay_open, cx);
            y += line + gap;
        }
        if y + line < bottom && inner.width >= 7 {
            y = self.categories(Rect::new(inner.x, y, inner.width, line), cx) + gap;
        }
        let list = Rect::new(
            inner.x,
            y.min(bottom),
            inner.width,
            bottom.saturating_sub(y),
        );
        self.list(model, list, line, cx);
        if footer > 0 {
            let label = format!("{}  Reset defaults", icons::RESTORE);
            Button::text(ElementId::ResetSettings, &label).render(
                Rect::new(
                    inner.x,
                    inner.bottom() - footer,
                    (label.width() as u16 + 3).min(inner.width),
                    footer,
                ),
                cx,
            );
        }
    }

    fn search(&mut self, rect: Rect, overlay_open: bool, cx: &mut Canvas) {
        let recording = self.editing.recording();
        SearchField::new(ElementId::SettingsSearch, &mut self.query)
            .active(cx.focused == Some(&ElementId::SettingsSearch) && !overlay_open && !recording)
            .placeholder(if recording {
                "Recording keys"
            } else {
                "Search settings"
            })
            .record((rect.width >= 16).then_some(Record::Toggle {
                id: ElementId::SettingsRecord,
                recording,
            }))
            .render(rect, cx);
    }

    fn categories(&self, area: Rect, cx: &mut Canvas) -> u16 {
        type Chip = fn(&str, &str, &str, bool) -> (String, u16, u16);
        let layouts: [Chip; 4] = [
            |_, name, icon, _| (format!("{icon}  {name}"), name.width() as u16 + 7, 2),
            |_, name, icon, _| (format!("{icon}  {name}"), name.width() as u16 + 5, 1),
            |_, name, _, _| (name.into(), name.width() as u16 + 4, 2),
            |_, name, icon, selected| {
                if selected {
                    (format!("{icon}  {name}"), name.width() as u16 + 7, 2)
                } else {
                    (icon.into(), ICON_CELLS + 3, 2)
                }
            },
        ];
        let chips = |layout: Chip| {
            CATEGORIES
                .iter()
                .map(|&(id, name, icon)| (id, layout(id, name, icon, id == self.category)))
                .collect::<Vec<_>>()
        };
        let fitting = layouts.into_iter().map(chips).find(|chips| {
            chips
                .iter()
                .map(|(_, (_, width, _))| width + 1)
                .sum::<u16>()
                <= area.width + 1
        });
        let Some(chips) = fitting else {
            let at = CATEGORIES
                .iter()
                .position(|(id, ..)| *id == self.category)
                .unwrap_or(0);
            let previous = (at + CATEGORIES.len() - 1) % CATEGORIES.len();
            let next = (at + 1) % CATEGORIES.len();
            for (index, text, rect) in [
                (previous, "‹", Rect::new(area.x, area.y, 2, area.height)),
                (
                    at,
                    CATEGORIES[at].1,
                    Rect::new(
                        area.x + 2,
                        area.y,
                        area.width.saturating_sub(4),
                        area.height,
                    ),
                ),
                (
                    next,
                    "›",
                    Rect::new(area.right().saturating_sub(2), area.y, 2, area.height),
                ),
            ] {
                self.chip(CATEGORIES[index].0, text, rect, 1, cx);
            }
            return area.bottom();
        };
        let mut x = area.x;
        for (id, (text, width, inset)) in chips {
            self.chip(
                id,
                &text,
                Rect::new(x, area.y, width, area.height),
                inset,
                cx,
            );
            x += width + 1;
        }
        area.bottom()
    }

    fn chip(&self, category: &str, text: &str, rect: Rect, inset: u16, cx: &mut Canvas) {
        let theme = cx.theme;
        let id = ElementId::SettingsCategory(category.into());
        let selected = self.category == category;
        let active = cx.hovered(&id) || cx.focused(&id);
        let fill = if selected {
            theme.selected
        } else if active {
            theme.lift(theme.card, 6)
        } else {
            theme.card
        };
        cx.surface(rect, fill, SURFACE_RADIUS, EDGE_INSET);
        let mut style = theme.base().bg(fill).fg(if selected || active {
            theme.accent
        } else {
            theme.muted
        });
        if selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        let inset = inset.min(rect.width / 2);
        cx.write(
            Rect::new(rect.x + inset, rect.y, rect.width - inset, 1),
            text.to_owned(),
            style,
        );
        cx.hit(id, rect, "");
    }
}

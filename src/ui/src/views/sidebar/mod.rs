mod rows;
mod spaces;
mod tab;

pub(crate) use rows::{SettingsPlace, SidebarRow};
pub(crate) use tab::InlineRename;

use crate::components::button::Button;
use crate::components::row::{Content, Row, Trailing};
use crate::element::ElementId;
use crate::events::DropTarget;
use crate::icons;
use crate::input::display_text;
use crate::runtime::canvas::{Canvas, RoundedSurface};
use ratatui::layout::Rect;
use rows::GROUP_GAP;
use std::collections::BTreeSet;
use vtabs_core::{Model, SpaceId, TabId};

const DROP_BAR: f32 = 0.14;

pub(crate) struct Sidebar {
    pub rows: Vec<SidebarRow>,
    pub rows_revision: Option<(u64, bool)>,
    pub scroll: usize,
    pub space_scroll: usize,
    pub rect: Rect,
    pub list: Rect,
    pub spaces: Rect,
    pub title_rects: Vec<(TabId, Rect)>,
    pub rename: Option<InlineRename>,
    pub settings_place: Option<SettingsPlace>,
    pub reveal_selection: bool,
    pub reveal_settings: bool,
    pub space_activity: BTreeSet<SpaceId>,
    pub last_tab: Option<TabId>,
    pub last_space: Option<SpaceId>,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            rows_revision: None,
            scroll: 0,
            space_scroll: 0,
            rect: Rect::default(),
            list: Rect::default(),
            spaces: Rect::default(),
            title_rects: Vec::new(),
            rename: None,
            settings_place: None,
            reveal_selection: true,
            reveal_settings: false,
            space_activity: BTreeSet::new(),
            last_tab: None,
            last_space: None,
        }
    }
}

pub(crate) struct SidebarProps {
    pub header_inset: u16,
    pub settings_open: bool,
    pub settings_listed: bool,
}

pub(crate) struct View<'a> {
    model: &'a Model,
    props: SidebarProps,
}

impl Sidebar {
    pub fn render(&mut self, model: &Model, area: Rect, props: SidebarProps, cx: &mut Canvas) {
        let theme = cx.theme;
        let reveal_selection = self.reveal_selection;
        let view = View { model, props };
        let inset = u16::from(area.width >= 8);
        let inner = Rect::new(
            area.x + inset,
            area.y,
            area.width.saturating_sub(inset * 2),
            area.height,
        );
        let plus_label = format!("{} ", icons::PLUS);
        let plus_width = inner.width.min(4);
        let plus = Rect::new(inner.right() - plus_width, area.bottom() - 1, plus_width, 1);
        let create_space = |rect: Rect, cx: &mut Canvas| {
            Button::icon(ElementId::CreateSpace, &plus_label)
                .tooltip("New space")
                .render(rect, cx)
        };
        if area.height < 5 {
            create_space(plus, cx);
            return;
        }
        let gap = GROUP_GAP;
        let spaces_y = area.bottom() - 2;
        let footer = (!model.footer.is_empty() && area.height >= 8)
            .then(|| Rect::new(inner.x, spaces_y.saturating_sub(gap + 1), inner.width, 1));
        let list_bottom = footer.map_or_else(
            || spaces_y.saturating_sub(gap),
            |footer| footer.y.saturating_sub(gap),
        );
        self.toolbar(&view, inner, cx);
        let search_y = inner.y + TOOLBAR_ROWS + gap;
        let search_height = if area.height >= 12 { 2 } else { 1 };
        let search = Rect::new(inner.x, search_y, inner.width, search_height);
        Row {
            muted: true,
            field: true,
            ..Row::new(
                ElementId::Search,
                search,
                icons::SEARCH,
                Content::Label("Search..."),
            )
        }
        .render(cx);
        let title_y = search.bottom();
        self.list = Rect::new(
            inner.x,
            title_y,
            inner.width,
            list_bottom.saturating_sub(title_y),
        );
        let row_height = self.row_height(model);
        let tabs_y = if title_y + row_height < list_bottom {
            space_title(
                model,
                Rect::new(inner.x, title_y, inner.width, row_height),
                cx,
            );
            title_y + row_height
        } else {
            title_y.min(list_bottom)
        };
        self.list = Rect::new(
            inner.x,
            tabs_y,
            inner.width,
            list_bottom.saturating_sub(tabs_y),
        );
        self.ensure_rows(model, view.props.settings_listed);
        if self.reveal_selection {
            if let Some(id) = model.selected_tab {
                self.ensure_tab_visible(model, id, view.props.settings_listed);
            }
            self.reveal_selection = false;
        }
        if self.reveal_settings {
            if let Some(at) = self
                .rows
                .iter()
                .position(|row| matches!(row, SidebarRow::Settings { .. }))
            {
                self.reveal_row(model, at);
            }
            self.reveal_settings = false;
        }
        self.scroll = self.scroll.min(self.max_scroll(model));
        self.title_rects.clear();
        let capacity = self.visible_rows(model);
        let end = (self.scroll + capacity).min(self.rows.len());
        let mut top = tabs_y;
        for at in self.scroll..end {
            let entry = self.rows[at];
            let span = self.row_span(model, entry);
            let rect = Rect::new(
                inner.x,
                top,
                inner.width,
                span.min(list_bottom.saturating_sub(top)),
            );
            top += span;
            if rect.is_empty() {
                continue;
            }
            match entry {
                SidebarRow::Gap => {}
                SidebarRow::NewTab => {
                    Row {
                        tooltip: Some("New tab  Cmd+T".into()),
                        muted: true,
                        ..Row::new(
                            ElementId::NewTab,
                            rect,
                            icons::PLUS,
                            Content::Label("New Tab"),
                        )
                    }
                    .render(cx);
                }
                SidebarRow::Settings { number } => {
                    Row {
                        index: model.settings.show_indexes.then_some(number),
                        tooltip: Some("Settings  Cmd+,".into()),
                        selected: view.props.settings_open,
                        trailing: model.settings.show_close.then_some(Trailing {
                            id: ElementId::CloseSettingsTab,
                            icon: icons::CLOSE,
                            tooltip: "Close settings  Cmd+W",
                        }),
                        ..Row::new(
                            ElementId::SettingsTab,
                            rect,
                            icons::SETTINGS,
                            Content::Label("Settings"),
                        )
                    }
                    .render(cx);
                }
                SidebarRow::Folder { index, count } => {
                    let Some(folder) = model.folders.get(index) else {
                        continue;
                    };
                    let label = format!("{}  {count}", display_text(&folder.name));
                    Row {
                        tooltip: Some(format!(
                            "{}\nDrop tabs here. Right click to rename or ungroup.",
                            folder.name
                        )),
                        ..Row::new(
                            ElementId::Folder(folder.id.clone()),
                            rect,
                            if folder.collapsed {
                                icons::FOLDER
                            } else {
                                icons::FOLDER_OPEN
                            },
                            Content::Label(&label),
                        )
                    }
                    .render(cx);
                }
                SidebarRow::Tab { id, number } => {
                    let Some(tab) = model.tabs.get(&id) else {
                        continue;
                    };
                    self.tab(&view, tab, number, rect, cx);
                }
            }
        }
        if self.rows.len() > capacity && self.list.width > 2 {
            let y = self.list.y
                + (self.list.height.saturating_sub(1) as usize * self.scroll
                    / self.max_scroll(model).max(1)) as u16;
            cx.write(Rect::new(area.right() - 1, y, 1, 1), "▏", theme.muted());
        }
        let pointer = cx.pointer;
        if let (Some(DropTarget::Beside { .. }), Some(motion)) =
            (&pointer.drop, pointer.drop_motion)
        {
            let edge = motion.position();
            let row = (edge.floor() as u16).clamp(
                self.list.y,
                self.list.bottom().saturating_sub(1).max(self.list.y),
            );
            let bar = Rect::new(inner.x + 1, row, inner.width.saturating_sub(2), 1);
            cx.mark(RoundedSurface {
                scale_y: DROP_BAR,
                shift_y: edge - f32::from(row) - 0.5,
                ..RoundedSurface::new(bar, theme.accent, 2.0, 0.0)
            });
        }
        if let Some(footer) = footer {
            cx.write(
                footer,
                display_text(model.footer.lines().next().unwrap_or("")),
                theme.muted(),
            );
        }
        let bar = Rect::new(inner.x, spaces_y, inner.width.saturating_sub(plus_width), 2);
        self.spaces(&view, bar, reveal_selection, cx);
        create_space(Rect::new(plus.x, spaces_y, plus.width, 2), cx);
    }

    fn toolbar(&self, view: &View, inner: Rect, cx: &mut Canvas) {
        let props = &view.props;
        let left = (inner.x + props.header_inset).min(inner.right());
        let size = inner.width.min(4);
        if left + size <= inner.right() {
            Button::icon(ElementId::Rail, &format!("{} ", icons::SIDEBAR))
                .tooltip("Toggle sidebar  Cmd+B")
                .render(Rect::new(left, inner.y, size, TOOLBAR_ROWS), cx);
        }
        if inner.width >= props.header_inset + 12 {
            Button::icon(ElementId::Settings, &format!("{} ", icons::SETTINGS))
                .tooltip("Settings  Cmd+,")
                .selected(props.settings_open)
                .render(Rect::new(inner.right() - 8, inner.y, 4, TOOLBAR_ROWS), cx);
            Button::icon(ElementId::Refresh, &format!("{} ", icons::REFRESH))
                .tooltip("Refresh configuration  Cmd+Shift+R")
                .render(Rect::new(inner.right() - 4, inner.y, 4, TOOLBAR_ROWS), cx);
        }
    }
}

const TOOLBAR_ROWS: u16 = 2;

fn space_title(model: &Model, rect: Rect, cx: &mut Canvas) {
    let space = model
        .spaces
        .iter()
        .find(|space| space.id == model.selected_space);
    let collapsed = space.is_some_and(|space| space.collapsed);
    let icon = if collapsed {
        icons::COLLAPSED
    } else if cx.row_hovered(&ElementId::SpaceTitle) {
        icons::EXPANDED
    } else {
        icons::SPACE
    };
    let name = space.map_or("Space", |space| space.name.as_str());
    let (label, tooltip) = if model.private {
        (
            "Private tabs".to_owned(),
            "Private tabs and history stay in memory. Spaces and settings are shared.".into(),
        )
    } else {
        (
            display_text(name),
            format!(
                "{name}\n{} pinned tabs and folders",
                if collapsed { "Show" } else { "Hide" }
            ),
        )
    };
    Row {
        tooltip: Some(tooltip),
        muted: true,
        trailing: Some(Trailing {
            id: ElementId::CreateFolder,
            icon: icons::PLUS,
            tooltip: "New folder",
        }),
        ..Row::new(ElementId::SpaceTitle, rect, icon, Content::Label(&label))
    }
    .render(cx);
}

#[cfg(test)]
#[path = "../../../tests/views/sidebar.rs"]
mod tests;

use crate::{input::display_text, *};
use ratatui::{layout::Alignment, text::Line};
use ratatui::{
    style::{Color, Style},
    widgets::{Block, Widget},
};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vtabs_core::{Model, RailMode};

fn icon_rect(mut rect: Rect, label: &str) -> Rect {
    let width = label.width();
    if usize::from(rect.width) > width && usize::from(rect.width) % 2 != width % 2 {
        rect.x += 1;
        rect.width -= 1;
    }
    rect
}

fn platform_tooltip(tooltip: String) -> String {
    if cfg!(target_os = "macos") {
        tooltip
    } else {
        tooltip
            .replace("Cmd+Shift+", "Ctrl+Shift+")
            .replace("Cmd+", "Ctrl+Shift+")
    }
}

impl SidebarUi {
    pub(crate) fn rounded(&mut self, rect: Rect, fill: Color, border: Color) {
        let rect = rect.intersection(self.staging.area);
        if rect.is_empty() {
            return;
        }
        Block::default()
            .style(Style::default().bg(fill))
            .render(rect, &mut self.staging);
        self.rounded_surfaces.push(RoundedSurface {
            rect,
            fill,
            border,
            radius: 9.0,
            inset: 0.0,
        });
    }

    fn button(
        &mut self,
        id: ElementId,
        rect: Rect,
        label: String,
        tooltip: String,
        selected: bool,
    ) {
        if rect.is_empty() {
            return;
        }
        let icon = matches!(
            id,
            ElementId::Rail
                | ElementId::Settings
                | ElementId::Refresh
                | ElementId::CreateFolder
                | ElementId::CreateSpace
                | ElementId::Space(_)
        );
        let mut style = self.item_style(&id, selected);
        let hovered = self.hovered.as_ref() == Some(&id)
            || matches!((&id, &self.hovered), (ElementId::Tab(tab), Some(ElementId::CloseTab(close))) if tab == close)
            || (id == ElementId::SettingsTab && self.hovered == Some(ElementId::CloseSettingsTab));
        let focused = self.focused.as_ref() == Some(&id);
        let visual = if icon {
            icon_rect(rect, label.trim())
        } else {
            rect
        };
        let pressed = self.drag.as_ref() == Some(&id) && !self.dragging;
        let fill = if icon {
            if hovered || pressed {
                self.theme.hover
            } else {
                self.theme.background
            }
        } else if selected || pressed {
            self.theme.selected
        } else if hovered {
            self.theme.card
        } else {
            self.theme.background
        };
        self.rounded(visual, fill, if focused { self.theme.accent } else { fill });
        if icon {
            if let Some(surface) = self.rounded_surfaces.last_mut() {
                surface.inset = if rect.width >= 3 && rect.height >= 2 {
                    5.0
                } else {
                    2.0
                };
                surface.radius = 8.0;
            }
            style = self
                .theme
                .base()
                .fg(if selected || hovered || focused || pressed {
                    self.theme.accent
                } else {
                    self.theme.muted
                });
        }
        if id == ElementId::NewTab && !hovered && !focused {
            style = style.fg(self.theme.muted);
        }
        let row = matches!(
            id,
            ElementId::Tab(_) | ElementId::NewTab | ElementId::SettingsTab
        );
        let centered = icon || rect.width < 12 && row;
        let label = if centered {
            Line::from(label.trim().to_owned()).alignment(Alignment::Center)
        } else {
            Line::from(label)
        };
        let text_y = if row && rect.height >= 3 {
            rect.y
        } else {
            rect.y + rect.height.saturating_sub(1) / 2
        };
        self.write(
            Rect::new(visual.x, text_y, visual.width, 1),
            label,
            style.bg(fill),
        );
        self.hit(id, rect, platform_tooltip(tooltip));
    }

    fn close_control(&mut self, id: ElementId, row: Rect, fill: Color, tooltip: &str) {
        let close = Rect::new(
            row.right().saturating_sub(3),
            row.y,
            row.width.min(3),
            row.height,
        );
        for x in close.x..close.right() {
            self.staging[(x, close.y)]
                .set_symbol(" ")
                .set_style(Style::default().bg(fill));
        }
        self.write(
            Rect::new(close.x, close.y, close.width, 1),
            Line::from("×").alignment(Alignment::Center),
            self.item_style(&id, false).bg(fill),
        );
        self.hit(id, close, platform_tooltip(tooltip.into()));
    }

    pub(crate) fn ensure_sidebar_entries(&mut self, model: &Model) {
        if self.sidebar_revision == Some((model.revision, self.settings_tab)) {
            return;
        }
        self.sidebar_rows.clear();
        self.sidebar_rows
            .reserve(model.visible_ids().len() + model.folders.len() + 1);
        let mut folders = HashMap::with_capacity(model.selected_folders().count());
        folders.extend(
            model
                .selected_folders()
                .map(|folder| (folder.id.as_str(), (0usize, 0..0))),
        );
        for tab in model.tabs.values() {
            if let Some(folder) = tab.folder_id.as_deref().and_then(|id| folders.get_mut(id)) {
                folder.0 += 1;
            }
        }
        for (index, id) in model.visible_ids().iter().enumerate() {
            let Some(tab) = model.tabs.get(id) else {
                continue;
            };
            let row = SidebarRow::Tab {
                id: *id,
                number: index + 1,
            };
            if tab.pinned && tab.folder_id.is_none() {
                self.sidebar_rows.push(row);
            }
            if let Some((_, range)) = tab.folder_id.as_deref().and_then(|id| folders.get_mut(id)) {
                // The model's visible projection keeps each folder's members contiguous.
                if range.start == range.end {
                    range.start = index;
                }
                range.end = index + 1;
            }
        }
        for (index, folder) in model.folders.iter().enumerate() {
            if folder.space_id != model.selected_space {
                continue;
            }
            let (count, range) = folders.remove(folder.id.as_str()).unwrap_or_default();
            self.sidebar_rows.push(SidebarRow::Folder { index, count });
            if !folder.collapsed {
                self.sidebar_rows.extend(range.map(|index| SidebarRow::Tab {
                    id: model.visible_ids()[index],
                    number: index + 1,
                }));
            }
        }
        self.sidebar_rows.push(SidebarRow::NewTab);
        self.sidebar_rows.extend(
            model
                .visible_ids()
                .iter()
                .enumerate()
                .filter(|(_, id)| model.tabs.get(id).is_some_and(|tab| !tab.pinned))
                .map(|(index, id)| SidebarRow::Tab {
                    id: *id,
                    number: index + 1,
                }),
        );
        if self.settings_tab {
            self.sidebar_rows.push(SidebarRow::Settings);
        }
        self.sidebar_revision = Some((model.revision, self.settings_tab));
    }

    pub(crate) fn row_height(&self, model: &Model) -> u16 {
        if self.tabs_rect.width < 12 || !model.settings.cards || self.tabs_rect.height < 4 {
            1
        } else if model.settings.show_metadata {
            3
        } else {
            2
        }
    }

    pub(crate) fn compose_sidebar(&mut self, model: &Model, area: Rect) {
        let reveal_selection = self.reveal_selection;
        let compact = model.settings.rail == RailMode::Collapsed || area.width < 12;
        let inset = u16::from(area.width >= 8);
        let inner = Rect::new(
            area.x + inset,
            area.y,
            area.width.saturating_sub(inset * 2),
            area.height,
        );
        let footer_height = if area.height >= 8 {
            3
        } else if area.height >= 5 {
            2
        } else {
            1
        };
        let footer_y = area.bottom().saturating_sub(footer_height);
        let plus_width = if compact {
            (inner.width / 2).clamp(1, 3).min(inner.width)
        } else {
            inner.width.min(4)
        };
        let plus = Rect::new(inner.right() - plus_width, area.bottom() - 1, plus_width, 1);
        if area.height < 5 {
            self.button(
                ElementId::CreateSpace,
                plus,
                " +".into(),
                "New space".into(),
                false,
            );
            return;
        }
        let toolbar_height = 2;
        let left = (inner.x + self.header_inset).min(inner.right());
        let size = inner.width.min(4);
        if left + size <= inner.right() {
            self.button(
                ElementId::Rail,
                Rect::new(left, inner.y, size, toolbar_height),
                " ◧".into(),
                "Toggle sidebar  Cmd+B".into(),
                false,
            );
        }
        if !compact && inner.width >= self.header_inset + 12 {
            self.button(
                ElementId::Settings,
                Rect::new(inner.right() - 8, inner.y, 4, toolbar_height),
                " ⚙".into(),
                "Settings  Cmd+,".into(),
                self.settings_page,
            );
            self.button(
                ElementId::Refresh,
                Rect::new(inner.right() - 4, inner.y, 4, toolbar_height),
                " ↻".into(),
                "Refresh configuration  Cmd+Shift+R".into(),
                false,
            );
        }
        let search_y = inner.y + toolbar_height;
        let search_height = if area.height >= 12 { 2 } else { 1 };
        let search = Rect::new(inner.x, search_y, inner.width, search_height);
        self.search_rect = search;
        let search_visual = if compact {
            icon_rect(search, "⌕")
        } else {
            search
        };
        self.rounded(
            search_visual,
            self.theme.card,
            if self.hovered == Some(ElementId::Search) || self.focused == Some(ElementId::Search) {
                self.theme.accent
            } else {
                self.theme.border
            },
        );
        let search_label = if compact {
            Line::from("⌕").alignment(Alignment::Center)
        } else {
            Line::from("⌕  Search tabs")
        };
        self.write(
            Rect::new(
                search_visual.x + u16::from(!compact && search_visual.width > 1),
                search.y + search.height.saturating_sub(1) / 2,
                search_visual
                    .width
                    .saturating_sub(if compact { 0 } else { 2 }),
                1,
            ),
            search_label,
            self.theme.muted().bg(self.theme.card),
        );
        self.hit(
            ElementId::Search,
            search,
            platform_tooltip("Search tabs  Cmd+K".into()),
        );
        let title_y = search.bottom();
        if title_y < footer_y && !compact {
            let name = model
                .spaces
                .iter()
                .find(|s| s.id == model.selected_space)
                .map(|s| s.name.as_str())
                .unwrap_or("Space");
            let title = Rect::new(inner.x, title_y, inner.width.saturating_sub(3), 1);
            self.write(
                title,
                if model.private {
                    "  Private tabs".to_owned()
                } else {
                    format!("  {}", display_text(name))
                },
                self.theme.muted(),
            );
            if model.private {
                self.hit(
                    ElementId::PrivateInfo,
                    title,
                    "Private tabs and history stay in memory. Spaces and settings are shared.",
                );
            }
            let folder = Rect::new(
                inner.right().saturating_sub(3),
                title_y,
                inner.width.min(3),
                1,
            );
            self.button(
                ElementId::CreateFolder,
                folder,
                " +".into(),
                "New folder  Cmd+Shift+G".into(),
                false,
            );
        }
        let tabs_y = (title_y + u16::from(!compact)).min(footer_y);
        self.tabs_rect = Rect::new(
            inner.x,
            tabs_y,
            inner.width,
            footer_y.saturating_sub(tabs_y),
        );
        self.ensure_sidebar_entries(model);
        let row_height = self.row_height(model);
        let capacity = usize::from(self.tabs_rect.height / row_height).max(1);
        if self.reveal_selection {
            if let Some(id) = model.selected_tab {
                self.ensure_tab_visible(model, id);
            }
            self.reveal_selection = false;
        }
        if self.reveal_settings {
            if let Some(at) = self
                .sidebar_rows
                .iter()
                .position(|row| *row == SidebarRow::Settings)
            {
                self.reveal_row(model, at);
            }
            self.reveal_settings = false;
        }
        self.tab_scroll = self
            .tab_scroll
            .min(self.sidebar_rows.len().saturating_sub(capacity));
        let end = (self.tab_scroll + capacity).min(self.sidebar_rows.len());
        for at in self.tab_scroll..end {
            let index = at - self.tab_scroll;
            let entry = self.sidebar_rows[at];
            let rect = Rect::new(
                inner.x,
                tabs_y + index as u16 * row_height,
                inner.width,
                row_height.min(footer_y.saturating_sub(tabs_y + index as u16 * row_height)),
            );
            if rect.is_empty() {
                continue;
            }
            match entry {
                SidebarRow::NewTab => self.button(
                    ElementId::NewTab,
                    rect,
                    if compact {
                        " +".into()
                    } else {
                        " +  New Tab".into()
                    },
                    "New tab  Cmd+T".into(),
                    false,
                ),
                SidebarRow::Settings => {
                    let selected = self.settings_page;
                    self.button(
                        ElementId::SettingsTab,
                        rect,
                        if compact {
                            " ⚙".into()
                        } else {
                            " ⚙ Settings".into()
                        },
                        "Settings".into(),
                        selected,
                    );
                    let hovered = self.hovered == Some(ElementId::SettingsTab)
                        || self.hovered == Some(ElementId::CloseSettingsTab);
                    if model.settings.show_close
                        && !compact
                        && (selected
                            || hovered
                            || self.focused == Some(ElementId::CloseSettingsTab))
                    {
                        let fill = if selected {
                            self.theme.selected
                        } else if hovered {
                            self.theme.card
                        } else {
                            self.theme.background
                        };
                        self.close_control(
                            ElementId::CloseSettingsTab,
                            rect,
                            fill,
                            "Close settings  Cmd+W",
                        );
                    }
                }
                SidebarRow::Folder { index, count } => {
                    let Some(folder) = model.folders.get(index) else {
                        continue;
                    };
                    self.button(
                        ElementId::Folder(folder.id.clone()),
                        rect,
                        format!(
                            " {} ▱ {}  {}",
                            if folder.collapsed { "›" } else { "⌄" },
                            display_text(&folder.name),
                            count
                        ),
                        format!(
                            "{}\nDrop tabs here. Right click to rename or ungroup.",
                            folder.name
                        ),
                        false,
                    );
                }
                SidebarRow::Tab { id, number } => {
                    let Some(tab) = model.tabs.get(&id) else {
                        continue;
                    };
                    let selected = model.selected_tab == Some(id) && !self.settings_page;
                    let icon = if !tab.icon.is_empty() {
                        display_text(&tab.icon)
                    } else if tab.bell {
                        "!".into()
                    } else if tab.unread {
                        "●".into()
                    } else if tab.pinned {
                        "◇".into()
                    } else if tab.remote {
                        "↗".into()
                    } else {
                        "›_".into()
                    };
                    let label = if compact {
                        format!(" {number}")
                    } else {
                        format!(
                            "{}{} {}{}",
                            if tab.folder_id.is_some() { "   " } else { " " },
                            icon,
                            if model.settings.show_indices {
                                format!("{number} ")
                            } else {
                                String::new()
                            },
                            display_text(tab.display_title())
                        )
                    };
                    self.button(
                        ElementId::Tab(id),
                        rect,
                        label,
                        format!("{}\n{}\n{}", tab.display_title(), tab.cwd, tab.domain),
                        selected,
                    );
                    let close_visible = model.settings.show_close
                        && !compact
                        && (selected
                            || self.hovered == Some(ElementId::Tab(id))
                            || self.hovered == Some(ElementId::CloseTab(id))
                            || self.focused == Some(ElementId::CloseTab(id)));
                    let fill =
                        if selected || self.drag == Some(ElementId::Tab(id)) && !self.dragging {
                            self.theme.selected
                        } else if self.hovered == Some(ElementId::Tab(id))
                            || self.hovered == Some(ElementId::CloseTab(id))
                        {
                            self.theme.card
                        } else {
                            self.theme.background
                        };
                    if close_visible {
                        self.close_control(ElementId::CloseTab(id), rect, fill, "Close tab  Cmd+W");
                    }
                    if row_height >= 3 {
                        self.write(
                            Rect::new(
                                rect.x + 2,
                                rect.y + 1,
                                rect.width.saturating_sub(if close_visible { 5 } else { 4 }),
                                1,
                            ),
                            format!("{}  {}", display_text(&tab.cwd), display_text(&tab.domain)),
                            self.theme.secondary_on(fill),
                        );
                    }
                }
            }
        }
        if model.visible_ids().is_empty() && self.tabs_rect.height > row_height + 1 && !compact {
            self.write(
                Rect::new(
                    inner.x + 1,
                    tabs_y + row_height + 1,
                    inner.width.saturating_sub(2),
                    1,
                ),
                "A little room to focus",
                self.theme.muted(),
            );
        }
        if self.sidebar_rows.len() > capacity && self.tabs_rect.width > 2 {
            let y = self.tabs_rect.y
                + (self.tabs_rect.height.saturating_sub(1) as usize * self.tab_scroll
                    / self.sidebar_rows.len().saturating_sub(capacity).max(1))
                    as u16;
            self.write(
                Rect::new(area.right() - 1, y, 1, 1),
                "▏",
                self.theme.muted(),
            );
        }
        if self.dragging
            && let Some(id) = self.hovered.clone()
            && let Some(hit) = self.hits.iter().find(|h| h.id == id).cloned()
            && matches!(
                id,
                ElementId::Folder(_) | ElementId::Tab(_) | ElementId::NewTab | ElementId::Space(_)
            )
        {
            self.rounded_surfaces.push(RoundedSurface {
                rect: hit.rect,
                fill: Color::Reset,
                border: self.theme.accent,
                radius: 9.0,
                inset: 0.0,
            });
        }
        if footer_height >= 3 {
            self.write(
                Rect::new(inner.x, footer_y, inner.width, 1),
                if model.footer.is_empty() {
                    String::new()
                } else {
                    display_text(model.footer.lines().next().unwrap_or(""))
                },
                self.theme.muted(),
            );
        }
        let slots_width = inner.width.saturating_sub(plus_width);
        let slot_width = if compact {
            slots_width.max(1)
        } else {
            slots_width.clamp(1, 4)
        };
        let slots = usize::from(slots_width / slot_width);
        self.space_scroll = self
            .space_scroll
            .min(model.spaces.len().saturating_sub(slots));
        self.spaces_rect = Rect::new(inner.x, area.bottom() - 2, slots_width, 2);
        if slots > 0
            && reveal_selection
            && let Some(index) = model
                .spaces
                .iter()
                .position(|s| s.id == model.selected_space)
        {
            if index < self.space_scroll {
                self.space_scroll = index;
            } else if index >= self.space_scroll + slots {
                self.space_scroll = index + 1 - slots;
            }
        }
        for (offset, space) in model
            .spaces
            .iter()
            .skip(self.space_scroll)
            .take(slots)
            .enumerate()
        {
            let rect = Rect::new(
                inner.x + offset as u16 * slot_width,
                area.bottom() - 2,
                slot_width.min(slots_width),
                2,
            );
            let label = if space.icon.is_empty() {
                space.name.graphemes(true).next().unwrap_or("○")
            } else {
                &space.icon
            };
            let activity = self.space_activity.contains(&space.id);
            self.button(
                ElementId::Space(space.id.clone()),
                rect,
                format!(" {label}"),
                format!(
                    "{}{}",
                    space.name,
                    if activity { " (activity)" } else { "" }
                ),
                space.id == model.selected_space,
            );
        }
        self.button(
            ElementId::CreateSpace,
            Rect::new(plus.x, area.bottom() - 2, plus.width, 2),
            " +".into(),
            "New space".into(),
            false,
        );
    }
}

#[cfg(test)]
#[path = "../tests/sidebar.rs"]
mod tests;

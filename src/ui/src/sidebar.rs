use crate::{icons, input::display_text, *};
use ratatui::{layout::Alignment, text::Line};
use ratatui::{
    style::{Color, Modifier, Style},
    widgets::{Block, Widget},
};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vtabs_core::{Model, RailMode, Tab};

const GROUP_GAP: u16 = 1;
const SURFACE_RADIUS: f32 = 9.0;
/// Adjacent rows touch in cells; this pixel inset is the hairline between their pills.
const ROW_INSET: f32 = 1.5;
const PRESS_INSET: f32 = 3.0;
const NESTED_INSET: f32 = 4.0;
/// The glyph, the blank cell it overflows into, its index badge, and a gap before the text.
const ICON_CELLS: u16 = 4;
const TRAILING_CELLS: u16 = 3;
const MIN_SEGMENT_CELLS: u16 = 4;

fn icon_rect(mut rect: Rect, label: &str) -> Rect {
    let width = label.width();
    if usize::from(rect.width) > width && usize::from(rect.width) % 2 != width % 2 {
        rect.x += 1;
        rect.width -= 1;
    }
    rect
}

/// macOS chords read as key glyphs; other platforms spell out their remapped chord.
fn platform_tooltip(tooltip: String) -> String {
    if cfg!(target_os = "macos") {
        tooltip
            .replace("Cmd+Shift+", &format!("⇧{} ", icons::COMMAND))
            .replace("Cmd+", &format!("{} ", icons::COMMAND))
            .replace("Opt+", "⌥")
    } else {
        tooltip
            .replace("Cmd+Shift+", "Ctrl+Shift+")
            .replace("Cmd+", "Ctrl+Shift+")
    }
}

/// A control revealed at a row's trailing edge while the row is hovered.
struct Trailing {
    id: ElementId,
    icon: &'static str,
    tooltip: &'static str,
}

enum Content<'a> {
    Label(&'a str),
    /// Splits read as side-by-side pills inside the tab's own row.
    Panes(&'a Tab, Option<&'a str>),
    Rename(TabId),
}

struct Row<'a> {
    id: ElementId,
    rect: Rect,
    indent: u16,
    icon: &'a str,
    index: Option<usize>,
    content: Content<'a>,
    tooltip: String,
    selected: bool,
    muted: bool,
    compact: bool,
    trailing: Option<Trailing>,
}

struct RowLayout {
    fill: Color,
    style: Style,
    /// Cells after the icon and index, before any trailing control.
    content: Rect,
}

impl SidebarUi {
    pub(crate) fn surface(&mut self, rect: Rect, fill: Color, radius: f32, inset: f32) {
        self.shape(rect, fill, radius, inset, false);
    }

    fn shape(&mut self, rect: Rect, fill: Color, radius: f32, inset: f32, square: bool) {
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
            radius,
            inset,
            square,
            shift_y: 0.0,
        });
    }

    pub(crate) fn rounded(&mut self, rect: Rect, fill: Color) {
        self.surface(rect, fill, SURFACE_RADIUS, 0.0);
    }

    fn press_inset(&self, id: &ElementId) -> f32 {
        self.press
            .as_ref()
            .filter(|press| &press.id == id)
            .map_or(0.0, |press| press.level * PRESS_INSET)
    }

    fn row_hovered(&self, id: &ElementId) -> bool {
        self.hovered.as_ref().map(ElementId::row).as_ref() == Some(id)
    }

    fn icon_button(
        &mut self,
        id: ElementId,
        rect: Rect,
        label: &str,
        tooltip: String,
        selected: bool,
    ) {
        if rect.is_empty() {
            return;
        }
        let active = selected
            || self.hovered.as_ref() == Some(&id)
            || self.focused.as_ref() == Some(&id)
            || self.press.as_ref().is_some_and(|press| press.id == id);
        let fill = if self.hovered.as_ref() == Some(&id) {
            self.theme.hover
        } else {
            self.theme.background
        };
        let visual = icon_rect(rect, label);
        let inset = if rect.width >= 3 && rect.height >= 2 {
            5.0
        } else {
            2.0
        };
        self.shape(visual, fill, 8.0, inset + self.press_inset(&id), true);
        let fg = if active {
            self.theme.accent
        } else {
            self.theme.muted
        };
        self.write(
            Rect::new(visual.x, rect.y, visual.width, 1),
            Line::from(label.to_owned()).alignment(Alignment::Center),
            self.theme.base().fg(fg).bg(fill),
        );
        self.hit(id, rect, platform_tooltip(tooltip));
    }

    /// Every list entry shares this geometry, hover, press and trailing-control behavior.
    fn row(&mut self, row: Row<'_>) -> RowLayout {
        let hovered = self.row_hovered(&row.id);
        let focused = self.focused.as_ref() == Some(&row.id);
        let pressed = self.press.as_ref().is_some_and(|press| press.id == row.id);
        let fill = if row.selected {
            self.theme.selected
        } else if hovered || pressed {
            self.theme.card
        } else {
            self.theme.background
        };
        let mut style = self.theme.base().bg(fill);
        if row.selected {
            style = style.fg(self.theme.accent).add_modifier(Modifier::BOLD);
        } else if hovered {
            style = style.fg(self.theme.accent);
        } else if row.muted && !focused {
            style = style.fg(self.theme.muted);
        }
        self.surface(
            row.rect,
            fill,
            SURFACE_RADIUS,
            ROW_INSET + self.press_inset(&row.id),
        );
        let trailing = row.trailing.filter(|trailing| {
            !row.compact
                && row.rect.width > TRAILING_CELLS + ICON_CELLS
                && (hovered || self.focused.as_ref() == Some(&trailing.id))
        });
        if row.compact {
            let label = row
                .index
                .map_or_else(|| format!("{} ", row.icon), |index| index.to_string());
            let visual = icon_rect(row.rect, &label);
            self.write(
                Rect::new(visual.x, row.rect.y, visual.width, 1),
                Line::from(label).alignment(Alignment::Center),
                style,
            );
            self.hit(row.id, row.rect, platform_tooltip(row.tooltip));
            return RowLayout {
                fill,
                style,
                content: Rect::default(),
            };
        }
        let x = row.rect.x + 1 + row.indent;
        let right = row.rect.right().saturating_sub(1);
        self.write(
            Rect::new(x, row.rect.y, ICON_CELLS.min(right.saturating_sub(x)), 1),
            format!("{} {}", row.icon, icons::badge(row.index)),
            style,
        );
        let x = (x + ICON_CELLS).min(right);
        let layout = RowLayout {
            fill,
            style,
            content: Rect::new(x, row.rect.y, right - x, row.rect.height),
        };
        self.hit(row.id, row.rect, platform_tooltip(row.tooltip));
        match row.content {
            Content::Label(label) => self.write(
                Rect::new(layout.content.x, layout.content.y, layout.content.width, 1),
                label.to_owned(),
                style,
            ),
            Content::Panes(tab, home) => self.pane_segments(tab, home, &layout),
            Content::Rename(id) => self.compose_rename(id, layout.content, fill),
        }
        if let Some(trailing) = trailing {
            self.overlay_control(trailing, row.rect, style);
        }
        layout
    }

    /// Floats over the row's content so revealing it never shifts what is beneath.
    fn overlay_control(&mut self, control: Trailing, row: Rect, style: Style) {
        let rect = Rect::new(
            row.right() - TRAILING_CELLS,
            row.y,
            TRAILING_CELLS,
            row.height,
        );
        let symbols = [control.icon, " ", " "];
        for (x, symbol) in (rect.x..rect.right()).zip(symbols) {
            let cell = &mut self.staging[(x, rect.y)];
            cell.set_symbol(symbol);
            if let Some(fg) = style.fg {
                cell.set_fg(fg);
            }
        }
        self.hit(control.id, rect, platform_tooltip(control.tooltip.into()));
    }

    fn pane_segments(&mut self, tab: &Tab, home: Option<&str>, layout: &RowLayout) {
        let count = tab.panes.len() as u16;
        let area = layout.content;
        let base = area.width / count;
        let extra = area.width % count;
        let mut x = area.x;
        for (at, pane) in tab.panes.iter().enumerate() {
            let width = base + u16::from((at as u16) < extra);
            let rect = Rect::new(x, area.y, width, area.height);
            x += width;
            let id = ElementId::Pane(tab.id, pane.id);
            let hovered = self.hovered.as_ref() == Some(&id);
            let fill = self
                .theme
                .lift(layout.fill, if pane.active || hovered { 14 } else { 7 });
            self.surface(rect, fill, 6.0, NESTED_INSET);
            let label = format!(
                "{} {}",
                icons::host(pane.remote, &pane.os),
                display_text(&pane.label(home))
            );
            let style = if pane.active {
                layout.style
            } else {
                layout
                    .style
                    .fg(self.theme.muted)
                    .remove_modifier(Modifier::BOLD)
            };
            self.write(
                Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1),
                label,
                style.bg(fill),
            );
            self.hit(id, rect, "");
        }
    }

    fn compose_rename(&mut self, id: TabId, content: Rect, fill: Color) {
        let Some(mut rename) = self.rename.take().filter(|rename| rename.id == id) else {
            return;
        };
        let edit = Rect::new(content.x, content.y, content.width, 1);
        // The host centers first-row text of a two-row surface; marks follow it.
        self.editor_shift = if content.height == 2 { 0.5 } else { 0.0 };
        self.compose_editor(&mut rename.editor, edit, fill, true);
        self.hit(ElementId::Editor, edit, "Tab title");
        self.rename = Some(rename);
    }

    /// Settings keeps its place among the open tabs; tabs opened later follow it.
    fn place_settings(&mut self, model: &Model) -> usize {
        let visible = model.visible_ids();
        let first_open = visible
            .iter()
            .position(|id| model.tabs.get(id).is_some_and(|tab| !tab.pinned))
            .unwrap_or(visible.len());
        let slot = match self.settings_place {
            None => visible.len(),
            Some(SettingsPlace { anchor: None, .. }) => 0,
            Some(SettingsPlace {
                anchor: Some(anchor),
                slot,
            }) => visible
                .iter()
                .position(|id| *id == anchor)
                .map_or(slot.saturating_sub(1), |at| at + 1),
        }
        .clamp(first_open, visible.len());
        self.settings_place = Some(SettingsPlace {
            anchor: slot.checked_sub(1).map(|at| visible[at]),
            slot,
        });
        slot
    }

    pub(crate) fn ensure_sidebar_entries(&mut self, model: &Model) {
        if self.sidebar_revision == Some((model.revision, self.settings_tab)) {
            return;
        }
        let slot = self.settings_tab.then(|| self.place_settings(model));
        let number = |index: usize| index + 1 + usize::from(slot.is_some_and(|slot| index >= slot));
        self.sidebar_rows.clear();
        self.sidebar_rows
            .reserve(model.visible_ids().len() + model.folders.len() + 2);
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
        let collapsed = model
            .spaces
            .iter()
            .any(|space| space.id == model.selected_space && space.collapsed);
        for (index, id) in model.visible_ids().iter().enumerate() {
            let Some(tab) = model.tabs.get(id) else {
                continue;
            };
            let row = SidebarRow::Tab {
                id: *id,
                number: number(index),
            };
            if tab.pinned && tab.folder_id.is_none() && !collapsed {
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
            if folder.space_id != model.selected_space || collapsed {
                continue;
            }
            let (count, range) = folders.remove(folder.id.as_str()).unwrap_or_default();
            self.sidebar_rows.push(SidebarRow::Folder { index, count });
            if !folder.collapsed {
                self.sidebar_rows.extend(range.map(|index| SidebarRow::Tab {
                    id: model.visible_ids()[index],
                    number: number(index),
                }));
            }
        }
        if !self.sidebar_rows.is_empty() {
            self.sidebar_rows.push(SidebarRow::Gap);
        }
        self.sidebar_rows.push(SidebarRow::NewTab);
        let settings = slot.map(|slot| SidebarRow::Settings { number: slot + 1 });
        for (index, id) in model.visible_ids().iter().enumerate() {
            if slot == Some(index) {
                self.sidebar_rows.extend(settings);
            }
            if model.tabs.get(id).is_some_and(|tab| !tab.pinned) {
                self.sidebar_rows.push(SidebarRow::Tab {
                    id: *id,
                    number: number(index),
                });
            }
        }
        if slot == Some(model.visible_ids().len()) {
            self.sidebar_rows.extend(settings);
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

    fn row_span(&self, model: &Model, row: SidebarRow) -> u16 {
        if row == SidebarRow::Gap {
            GROUP_GAP
        } else {
            self.row_height(model)
        }
    }

    /// Whole rows that fit the list when it starts at `start`; never fewer than one.
    pub(crate) fn rows_fitting(&self, model: &Model, start: usize) -> usize {
        let mut used = 0;
        self.sidebar_rows
            .iter()
            .skip(start)
            .take_while(|row| {
                used += self.row_span(model, **row);
                used <= self.tabs_rect.height
            })
            .count()
            .max(1)
    }

    pub(crate) fn visible_rows(&self, model: &Model) -> usize {
        self.rows_fitting(model, self.tab_scroll)
    }

    fn max_scroll(&self, model: &Model) -> usize {
        let mut used = 0;
        let hidden = self
            .sidebar_rows
            .iter()
            .rev()
            .take_while(|row| {
                used += self.row_span(model, **row);
                used <= self.tabs_rect.height
            })
            .count()
            .max(1);
        self.sidebar_rows.len().saturating_sub(hidden)
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
        let plus_label = format!("{} ", icons::PLUS);
        let plus_width = if compact {
            (inner.width / 2).clamp(1, 3).min(inner.width)
        } else {
            inner.width.min(4)
        };
        let plus = Rect::new(inner.right() - plus_width, area.bottom() - 1, plus_width, 1);
        if area.height < 5 {
            self.icon_button(
                ElementId::CreateSpace,
                plus,
                &plus_label,
                "New space".into(),
                false,
            );
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
        let toolbar_height = 2;
        let left = (inner.x + self.header_inset).min(inner.right());
        let size = inner.width.min(4);
        if left + size <= inner.right() {
            self.icon_button(
                ElementId::Rail,
                Rect::new(left, inner.y, size, toolbar_height),
                &format!("{} ", icons::SIDEBAR),
                "Toggle sidebar  Cmd+B".into(),
                false,
            );
        }
        if !compact && inner.width >= self.header_inset + 12 {
            self.icon_button(
                ElementId::Settings,
                Rect::new(inner.right() - 8, inner.y, 4, toolbar_height),
                &format!("{} ", icons::SETTINGS),
                "Settings  Cmd+,".into(),
                self.settings_page,
            );
            self.icon_button(
                ElementId::Refresh,
                Rect::new(inner.right() - 4, inner.y, 4, toolbar_height),
                &format!("{} ", icons::REFRESH),
                "Refresh configuration  Cmd+Shift+R".into(),
                false,
            );
        }
        let search_y = inner.y + toolbar_height + gap;
        let search_height = if area.height >= 12 { 2 } else { 1 };
        let search = Rect::new(inner.x, search_y, inner.width, search_height);
        self.search_rect = search;
        let search_icon = format!("{} ", icons::SEARCH);
        let search_visual = if compact {
            icon_rect(search, &search_icon)
        } else {
            search
        };
        self.surface(
            search_visual,
            self.theme.selected,
            SURFACE_RADIUS,
            ROW_INSET + self.press_inset(&ElementId::Search),
        );
        let search_label = if compact {
            Line::from(search_icon).alignment(Alignment::Center)
        } else {
            Line::from(format!(
                "{:<width$}Search tabs",
                icons::SEARCH,
                width = usize::from(ICON_CELLS)
            ))
        };
        self.write(
            Rect::new(
                search_visual.x + u16::from(!compact && search_visual.width > 1),
                search.y,
                search_visual
                    .width
                    .saturating_sub(if compact { 0 } else { 2 }),
                1,
            ),
            search_label,
            self.theme.muted().bg(self.theme.selected),
        );
        self.hit(
            ElementId::Search,
            search,
            platform_tooltip("Search tabs  Cmd+K".into()),
        );
        let title_y = search.bottom() + gap;
        // Row height follows the room below search, before the space row claims its share.
        self.tabs_rect = Rect::new(
            inner.x,
            title_y,
            inner.width,
            list_bottom.saturating_sub(title_y),
        );
        let row_height = self.row_height(model);
        let tabs_y = if !compact && title_y + row_height < list_bottom {
            self.compose_space_title(model, Rect::new(inner.x, title_y, inner.width, row_height));
            title_y + row_height
        } else {
            title_y.min(list_bottom)
        };
        self.tabs_rect = Rect::new(
            inner.x,
            tabs_y,
            inner.width,
            list_bottom.saturating_sub(tabs_y),
        );
        self.ensure_sidebar_entries(model);
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
                .position(|row| matches!(row, SidebarRow::Settings { .. }))
            {
                self.reveal_row(model, at);
            }
            self.reveal_settings = false;
        }
        self.tab_scroll = self.tab_scroll.min(self.max_scroll(model));
        self.title_rects.clear();
        let capacity = self.visible_rows(model);
        let end = (self.tab_scroll + capacity).min(self.sidebar_rows.len());
        let mut top = tabs_y;
        for at in self.tab_scroll..end {
            let entry = self.sidebar_rows[at];
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
                    self.row(Row {
                        id: ElementId::NewTab,
                        rect,
                        indent: 0,
                        icon: icons::PLUS,
                        index: None,
                        content: Content::Label("New Tab"),
                        tooltip: "New tab  Cmd+T".into(),
                        selected: false,
                        muted: true,
                        compact,
                        trailing: None,
                    });
                }
                SidebarRow::Settings { number } => {
                    self.row(Row {
                        id: ElementId::SettingsTab,
                        rect,
                        indent: 0,
                        icon: icons::SETTINGS,
                        index: model.settings.show_indexes.then_some(number),
                        content: Content::Label("Settings"),
                        tooltip: "Settings  Cmd+,".into(),
                        selected: self.settings_page,
                        muted: false,
                        compact,
                        trailing: model.settings.show_close.then_some(Trailing {
                            id: ElementId::CloseSettingsTab,
                            icon: icons::CLOSE,
                            tooltip: "Close settings  Cmd+W",
                        }),
                    });
                }
                SidebarRow::Folder { index, count } => {
                    let Some(folder) = model.folders.get(index) else {
                        continue;
                    };
                    self.row(Row {
                        id: ElementId::Folder(folder.id.clone()),
                        rect,
                        indent: 0,
                        icon: if folder.collapsed {
                            icons::FOLDER
                        } else {
                            icons::FOLDER_OPEN
                        },
                        index: None,
                        content: Content::Label(&format!(
                            "{}  {count}",
                            display_text(&folder.name)
                        )),
                        tooltip: format!(
                            "{}\nDrop tabs here. Right click to rename or ungroup.",
                            folder.name
                        ),
                        selected: false,
                        muted: false,
                        compact,
                        trailing: None,
                    });
                }
                SidebarRow::Tab { id, number } => {
                    let Some(tab) = model.tabs.get(&id) else {
                        continue;
                    };
                    self.compose_tab(model, tab, number, rect, compact);
                }
            }
        }
        if self.sidebar_rows.len() > capacity && self.tabs_rect.width > 2 {
            let y = self.tabs_rect.y
                + (self.tabs_rect.height.saturating_sub(1) as usize * self.tab_scroll
                    / self.max_scroll(model).max(1)) as u16;
            self.write(
                Rect::new(area.right() - 1, y, 1, 1),
                "▏",
                self.theme.muted(),
            );
        }
        if self.dragging
            && let Some(id) = self.hovered.as_ref().map(ElementId::row)
            && let Some(hit) = self.hits.iter().find(|h| h.id == id).cloned()
            && matches!(
                id,
                ElementId::Folder(_) | ElementId::Tab(_) | ElementId::NewTab | ElementId::Space(_)
            )
        {
            self.rounded_surfaces.push(RoundedSurface {
                rect: hit.rect,
                fill: self.theme.selected,
                radius: SURFACE_RADIUS,
                inset: ROW_INSET,
                square: false,
                shift_y: 0.0,
            });
        }
        if let Some(footer) = footer {
            self.write(
                footer,
                display_text(model.footer.lines().next().unwrap_or("")),
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
        self.spaces_rect = Rect::new(inner.x, spaces_y, slots_width, 2);
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
                spaces_y,
                slot_width.min(slots_width),
                2,
            );
            let label = if space.icon.is_empty() {
                space.name.graphemes(true).next().unwrap_or("○")
            } else {
                icons::space(space.icon.trim())
            };
            // One-cell glyphs take the same blank partner as the plus beside them.
            let label = if label.width() == 1 {
                format!("{label} ")
            } else {
                label.to_owned()
            };
            let activity = self.space_activity.contains(&space.id);
            self.icon_button(
                ElementId::Space(space.id.clone()),
                rect,
                &label,
                format!(
                    "{}{}",
                    space.name,
                    if activity { " (activity)" } else { "" }
                ),
                space.id == model.selected_space,
            );
        }
        self.icon_button(
            ElementId::CreateSpace,
            Rect::new(plus.x, spaces_y, plus.width, 2),
            &plus_label,
            "New space".into(),
            false,
        );
    }

    fn compose_space_title(&mut self, model: &Model, rect: Rect) {
        let space = model
            .spaces
            .iter()
            .find(|space| space.id == model.selected_space);
        let collapsed = space.is_some_and(|space| space.collapsed);
        let icon = if collapsed {
            icons::COLLAPSED
        } else if self.row_hovered(&ElementId::SpaceTitle) {
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
        self.row(Row {
            id: ElementId::SpaceTitle,
            rect,
            indent: 0,
            icon,
            index: None,
            content: Content::Label(&label),
            tooltip,
            selected: false,
            muted: true,
            compact: false,
            trailing: Some(Trailing {
                id: ElementId::CreateFolder,
                icon: icons::PLUS,
                tooltip: "New folder",
            }),
        });
    }

    fn compose_tab(&mut self, model: &Model, tab: &Tab, number: usize, rect: Rect, compact: bool) {
        let renaming = self
            .rename
            .as_ref()
            .is_some_and(|rename| rename.id == tab.id);
        let name = tab
            .custom_title()
            .map(str::to_owned)
            .or_else(|| tab.location(model.home.as_deref()))
            .unwrap_or_default();
        let name = display_text(&name);
        let segmented = !compact
            && tab.panes.len() > 1
            && rect.width.saturating_sub(ICON_CELLS + 2)
                >= tab.panes.len() as u16 * MIN_SEGMENT_CELLS;
        let active = tab.panes.iter().find(|pane| pane.active);
        let layout = self.row(Row {
            id: ElementId::Tab(tab.id),
            rect,
            indent: if tab.folder_id.is_some() { 2 } else { 0 },
            icon: active.map_or_else(
                || icons::host(tab.remote, &tab.os),
                |pane| icons::host(pane.remote, &pane.os),
            ),
            index: (compact || model.settings.show_indexes).then_some(number),
            content: if renaming {
                Content::Rename(tab.id)
            } else if segmented {
                Content::Panes(tab, model.home.as_deref())
            } else {
                Content::Label(&name)
            },
            tooltip: if compact { name.clone() } else { String::new() },
            selected: model.selected_tab == Some(tab.id) && !self.settings_page,
            muted: false,
            compact,
            trailing: model.settings.show_close.then_some(Trailing {
                id: ElementId::CloseTab(tab.id),
                icon: icons::CLOSE,
                tooltip: "Close tab  Cmd+W",
            }),
        });
        if compact {
            return;
        }
        if !renaming && !segmented {
            self.title_rects.push((tab.id, layout.content));
        }
        if rect.height >= 3 {
            self.write(
                Rect::new(layout.content.x, rect.y + 1, layout.content.width, 1),
                display_text(&tab.domain),
                self.theme.secondary_on(layout.fill),
            );
        }
    }
}

#[cfg(test)]
#[path = "../tests/sidebar.rs"]
mod tests;

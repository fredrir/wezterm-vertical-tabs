use crate::{
    DropTarget, ElementId, Rect, TextEditor,
    components::{
        DROP_TINT, ICON_CELLS, NESTED_INSET, TRAILING_CELLS,
        button::Button,
        row::{Content, Row, RowLayout, Trailing, trailing_control},
        text_input::TextInput,
    },
    icons,
    input::display_text,
    list,
    runtime::canvas::{Canvas, RoundedSurface},
    spans_machines, tab_machine, tab_name,
};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use std::collections::{BTreeSet, HashMap};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vtabs_core::{Model, RailMode, SpaceId, Tab, TabId, TabPane};

const GROUP_GAP: u16 = 1;
const DROP_BAR: f32 = 0.14;
const OCCLUDER_INSET: f32 = 6.0;
const MIN_SEGMENT_CELLS: u16 = 4;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarRow {
    Tab {
        id: TabId,
        number: usize,
    },
    Folder {
        index: usize,
        count: usize,
    },
    /// Separates the pinned and folder group from the open tabs.
    Gap,
    NewTab,
    Settings {
        number: usize,
    },
}

/// The tab Settings follows, and the position that anchor last gave it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SettingsPlace {
    pub anchor: Option<TabId>,
    pub slot: usize,
}

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

/// What the sidebar shows of state it does not own.
pub(crate) struct SidebarProps {
    pub header_inset: u16,
    pub settings_open: bool,
    pub settings_listed: bool,
}

/// Inputs every part of one sidebar render shares.
struct View<'a> {
    model: &'a Model,
    props: SidebarProps,
    compact: bool,
}

impl Sidebar {
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

    pub fn ensure_rows(&mut self, model: &Model, settings_listed: bool) {
        if self.rows_revision == Some((model.revision, settings_listed)) {
            return;
        }
        let slot = settings_listed.then(|| self.place_settings(model));
        let number = |index: usize| index + 1 + usize::from(slot.is_some_and(|slot| index >= slot));
        self.rows.clear();
        self.rows
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
                self.rows.push(row);
            }
            if let Some((_, range)) = tab.folder_id.as_deref().and_then(|id| folders.get_mut(id)) {
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
            self.rows.push(SidebarRow::Folder { index, count });
            if !folder.collapsed {
                self.rows.extend(range.map(|index| SidebarRow::Tab {
                    id: model.visible_ids()[index],
                    number: number(index),
                }));
            }
        }
        if !self.rows.is_empty() {
            self.rows.push(SidebarRow::Gap);
        }
        self.rows.push(SidebarRow::NewTab);
        let settings = slot.map(|slot| SidebarRow::Settings { number: slot + 1 });
        for (index, id) in model.visible_ids().iter().enumerate() {
            if slot == Some(index) {
                self.rows.extend(settings);
            }
            if model.tabs.get(id).is_some_and(|tab| !tab.pinned) {
                self.rows.push(SidebarRow::Tab {
                    id: *id,
                    number: number(index),
                });
            }
        }
        if slot == Some(model.visible_ids().len()) {
            self.rows.extend(settings);
        }
        self.rows_revision = Some((model.revision, settings_listed));
    }

    pub fn row_height(&self, model: &Model) -> u16 {
        if self.list.width < 12 || !model.settings.cards || self.list.height < 4 {
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

    pub fn rows_fitting(&self, model: &Model, start: usize) -> usize {
        let mut used = 0;
        self.rows
            .iter()
            .skip(start)
            .take_while(|row| {
                used += self.row_span(model, **row);
                used <= self.list.height
            })
            .count()
            .max(1)
    }

    pub fn visible_rows(&self, model: &Model) -> usize {
        self.rows_fitting(model, self.scroll)
    }

    fn max_scroll(&self, model: &Model) -> usize {
        let mut used = 0;
        let hidden = self
            .rows
            .iter()
            .rev()
            .take_while(|row| {
                used += self.row_span(model, **row);
                used <= self.list.height
            })
            .count()
            .max(1);
        self.rows.len().saturating_sub(hidden)
    }

    pub fn reveal_row(&mut self, model: &Model, at: usize) {
        self.scroll = list::reveal(self.scroll, at, |start| {
            start + self.rows_fitting(model, start)
        });
    }

    /// Scrolls to the tab, or to its folder while the folder is collapsed.
    pub fn ensure_tab_visible(&mut self, model: &Model, id: TabId, settings_listed: bool) {
        self.ensure_rows(model, settings_listed);
        let collapsed_folder = if let Some(tab) = model.tabs.get(&id)
            && let Some(folder) = &tab.folder_id
        {
            model
                .folders
                .iter()
                .position(|f| &f.id == folder && f.collapsed)
        } else {
            None
        };
        if let Some(at) = self.rows.iter().position(|row| match row {
            SidebarRow::Folder { index, .. } => collapsed_folder == Some(*index),
            SidebarRow::Tab { id: tab, .. } => collapsed_folder.is_none() && *tab == id,
            SidebarRow::Gap | SidebarRow::NewTab | SidebarRow::Settings { .. } => false,
        }) {
            self.reveal_row(model, at);
        }
    }

    pub fn render(&mut self, model: &Model, area: Rect, props: SidebarProps, cx: &mut Canvas) {
        let theme = cx.theme;
        let reveal_selection = self.reveal_selection;
        let compact = model.settings.rail == RailMode::Collapsed || area.width < 12;
        let view = View {
            model,
            props,
            compact,
        };
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
            compact,
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
        let tabs_y = if !compact && title_y + row_height < list_bottom {
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
                        compact,
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
                        compact,
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
                        compact,
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
        if !view.compact && inner.width >= props.header_inset + 12 {
            Button::icon(ElementId::Settings, &format!("{} ", icons::SETTINGS))
                .tooltip("Settings  Cmd+,")
                .selected(props.settings_open)
                .render(Rect::new(inner.right() - 8, inner.y, 4, TOOLBAR_ROWS), cx);
            Button::icon(ElementId::Refresh, &format!("{} ", icons::REFRESH))
                .tooltip("Refresh configuration  Cmd+Shift+R")
                .render(Rect::new(inner.right() - 4, inner.y, 4, TOOLBAR_ROWS), cx);
        }
    }

    /// One button per space along `bar`, scrolled to keep the selected space in view.
    fn spaces(&mut self, view: &View, bar: Rect, reveal_selection: bool, cx: &mut Canvas) {
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

    fn tab(&mut self, view: &View, tab: &Tab, number: usize, rect: Rect, cx: &mut Canvas) {
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

fn rename_field(cx: &mut Canvas, editor: &mut TextEditor, layout: &RowLayout) {
    let content = layout.content;
    let edit = Rect::new(content.x, content.y, content.width, 1);
    TextInput::new(ElementId::Editor, editor, layout.fill)
        .active(true)
        .shift(if content.height == 2 { 0.5 } else { 0.0 })
        .render(edit, cx);
    cx.hit(ElementId::Editor, edit, "Tab title");
}

/// Where a dragged tab would join as a split, labelled with what is being dropped.
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

#[cfg(test)]
#[path = "../tests/sidebar.rs"]
mod tests;

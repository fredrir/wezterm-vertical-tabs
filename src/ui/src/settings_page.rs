use crate::*;
use ratatui::style::{Modifier, Style};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vtabs_core::{SettingDescriptor, SettingKind, settings};

const CATEGORIES: &[(&str, &str)] = &[
    ("all", "All"),
    ("layout", "Layout"),
    ("appearance", "Tabs"),
    ("theme", "Colors"),
    ("motion", "Motion"),
    ("behavior", "General"),
];

fn fields(category: &str, query: &str) -> Vec<&'static SettingDescriptor> {
    let query = query.trim().to_lowercase();
    settings::descriptors()
        .iter()
        .filter(|field| category == "all" || field.group == category)
        .filter(|field| {
            query.is_empty()
                || [field.key, field.label, field.description, field.group]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&query))
        })
        .collect()
}

fn group_label(group: &str) -> &'static str {
    match group {
        "layout" => "Your workspace",
        "appearance" => "Tabs and details",
        "theme" => "Color and atmosphere",
        "motion" => "Motion and accessibility",
        "behavior" => "Everyday behavior",
        _ => "Preferences",
    }
}

fn value_label(model: &Model, field: &SettingDescriptor) -> String {
    let value = model.settings.get(field.key).unwrap_or_default();
    match field.kind {
        SettingKind::Bool => {
            if value.as_bool().unwrap_or(false) {
                "On".into()
            } else {
                "Off".into()
            }
        }
        SettingKind::Object => format!("{} entries", value.as_object().map_or(0, |v| v.len())),
        SettingKind::List => format!("{} items", value.as_array().map_or(0, |v| v.len())),
        _ if value.is_null() => "Automatic".into(),
        _ => value
            .as_str()
            .map_or_else(|| value.to_string(), input::display_text),
    }
}

fn row_height(area: Rect) -> u16 {
    if area.height >= 18 && area.width >= 22 {
        4
    } else if area.height >= 11 && area.width >= 14 {
        2
    } else {
        1
    }
}

fn visible_end(
    fields: &[&SettingDescriptor],
    start: usize,
    height: u16,
    card_height: u16,
) -> usize {
    let mut remaining = height;
    let mut end = start;
    let mut group = "";
    for field in fields.iter().skip(start) {
        let heading = u16::from(card_height > 1 && field.group != group);
        let needed = heading + card_height;
        if needed > remaining {
            break;
        }
        remaining = remaining.saturating_sub(needed);
        group = field.group;
        end += 1;
    }
    end
}

impl SidebarUi {
    pub(crate) fn compose_settings_page(&mut self, model: &Model, area: Rect) {
        if area.is_empty() {
            return;
        }
        self.page_rect = area;
        let spacious = area.height >= 24;
        let area = if spacious && area.width >= 48 {
            self.rounded(area, self.theme.background, self.theme.background);
            Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2)
        } else {
            area
        };
        self.rounded(area, self.theme.background, self.theme.border);
        let margin = if area.width >= 72 {
            4
        } else if area.width >= 40 {
            2
        } else {
            u16::from(area.width >= 8)
        };
        let content_width = area.width.saturating_sub(margin * 2).min(88);
        let inner = Rect::new(
            area.x + (area.width - content_width) / 2,
            area.y + u16::from(area.height >= 8),
            content_width,
            area.height.saturating_sub(u16::from(area.height >= 8) * 2),
        );
        if inner.is_empty() {
            return;
        }
        let close = Rect::new(
            inner.right().saturating_sub(3).max(inner.x),
            inner.y,
            inner.width.min(3),
            1,
        );
        self.rounded(
            close,
            self.theme.card,
            self.settings_control_border(&ElementId::CloseSettings),
        );
        self.write(
            close,
            " ×",
            self.item_style(&ElementId::CloseSettings, false)
                .bg(self.theme.card),
        );
        self.hit(ElementId::CloseSettings, close, "Close settings (Escape)");
        self.write(
            Rect::new(
                inner.x,
                inner.y,
                inner.width.saturating_sub(close.width + 1),
                1,
            ),
            "Settings",
            self.theme.base().add_modifier(Modifier::BOLD),
        );
        let mut y = inner.y + 1;
        if area.height >= 20 && inner.width >= 26 {
            self.write(
                Rect::new(inner.x, y, inner.width, 1),
                "Make room for your way of working.",
                self.theme.muted(),
            );
            y += 2;
        }
        let footer_height = u16::from(inner.height >= 9);
        let bottom = inner.bottom().saturating_sub(footer_height);
        if y < bottom.saturating_sub(2) {
            let height = if area.height >= 16 { 3 } else { 1 };
            let search = Rect::new(inner.x, y, inner.width, height.min(bottom - y));
            self.compose_settings_search(search);
            y = search.bottom();
            if spacious && bottom.saturating_sub(y) > 9 {
                y += 1;
            }
        }
        if y + 2 < bottom && inner.width >= 7 {
            let reserve = (row_height(area) + 1).min(bottom.saturating_sub(y + 1));
            y = self.compose_settings_categories(Rect::new(
                inner.x,
                y,
                inner.width,
                bottom - y - reserve,
            ));
            if spacious && bottom.saturating_sub(y) > 9 {
                y += 1;
            }
        }
        let list = Rect::new(
            inner.x,
            y.min(bottom),
            inner.width,
            bottom.saturating_sub(y),
        );
        let fields = fields(&self.settings_category, self.settings_query.text());
        self.settings_selected = self.settings_selected.min(fields.len().saturating_sub(1));
        self.settings_scroll = self.settings_scroll.min(self.settings_selected);
        let mut height = row_height(area);
        if list.height < height + u16::from(height > 1) {
            height = 1;
        }
        let gap = u16::from(area.height >= 30 && height >= 4);
        let stride = height + gap;
        while self.settings_scroll < self.settings_selected
            && visible_end(&fields, self.settings_scroll, list.height, stride)
                <= self.settings_selected
        {
            self.settings_scroll += 1;
        }
        let end = visible_end(&fields, self.settings_scroll, list.height, stride);
        let mut group = "";
        for (index, field) in fields
            .iter()
            .enumerate()
            .take(end)
            .skip(self.settings_scroll)
        {
            if height > 1 && field.group != group {
                self.write(
                    Rect::new(list.x, y, list.width, 1),
                    group_label(field.group),
                    self.theme.muted(),
                );
                y += 1;
                group = field.group;
            }
            self.compose_setting_row(
                model,
                field,
                index,
                Rect::new(list.x, y, list.width, height),
            );
            y += stride;
        }
        if fields.is_empty() && list.height > 0 {
            self.write(
                Rect::new(list.x, list.y, list.width, 1),
                "No matching settings",
                self.theme.muted(),
            );
            if list.height > 2 {
                self.write(
                    Rect::new(list.x, list.y + 2, list.width, 1),
                    "Try another word or category.",
                    self.theme.muted(),
                );
            }
        }
        if footer_height > 0 {
            let reset = Rect::new(inner.x, bottom, inner.width.min(16), 1);
            self.rounded(
                reset,
                self.theme.card,
                self.settings_control_border(&ElementId::ResetSettings),
            );
            self.write(
                reset,
                " Reset defaults",
                self.item_style(&ElementId::ResetSettings, false)
                    .bg(self.theme.card),
            );
            self.hit(
                ElementId::ResetSettings,
                reset,
                "Reset saved settings to defaults",
            );
            if inner.width >= 44 {
                self.write(
                    Rect::new(
                        reset.right() + 2,
                        bottom,
                        inner.width.saturating_sub(reset.width + 2),
                        1,
                    ),
                    if inner.width >= 66 {
                        "↑↓ browse   Enter edits   Delete resets   Esc closes"
                    } else {
                        "↑↓ browse   Esc closes"
                    },
                    self.theme.muted(),
                );
            }
        }
    }

    fn settings_control_border(&self, id: &ElementId) -> ratatui::style::Color {
        if self.focused.as_ref() == Some(id) || self.hovered.as_ref() == Some(id) {
            self.theme.accent
        } else {
            self.theme.border
        }
    }

    fn compose_settings_categories(&mut self, area: Rect) -> u16 {
        let mut required_rows = 1;
        let mut used = 0;
        for (_, label) in CATEGORIES {
            let width = (label.width() as u16 + 2).min(area.width);
            if used > 0 && used + width > area.width {
                required_rows += 1;
                used = 0;
            }
            used += width + 1;
        }
        if required_rows > area.height {
            let selected = CATEGORIES
                .iter()
                .position(|(id, _)| *id == self.settings_category)
                .unwrap_or(0);
            let previous = (selected + CATEGORIES.len() - 1) % CATEGORIES.len();
            let next = (selected + 1) % CATEGORIES.len();
            for (index, text, rect) in [
                (previous, "‹", Rect::new(area.x, area.y, 2, 1)),
                (
                    selected,
                    CATEGORIES[selected].1,
                    Rect::new(area.x + 2, area.y, area.width - 4, 1),
                ),
                (next, "›", Rect::new(area.right() - 2, area.y, 2, 1)),
            ] {
                self.settings_category_chip(CATEGORIES[index].0, text, rect);
            }
            return area.y + 1;
        }
        let mut x = area.x;
        let mut y = area.y;
        for (category, label) in CATEGORIES {
            let width = (label.width() as u16 + 2).min(area.width);
            if x + width > area.right() {
                x = area.x;
                y += 1;
            }
            self.settings_category_chip(category, label, Rect::new(x, y, width, 1));
            x += width + 1;
        }
        y + 1
    }

    fn settings_category_chip(&mut self, category: &str, label: &str, rect: Rect) {
        let id = ElementId::SettingsCategory(category.into());
        let selected = self.settings_category == category;
        let fill = if selected {
            self.theme.selected
        } else {
            self.theme.card
        };
        self.rounded(
            rect,
            fill,
            if selected {
                self.theme.accent
            } else {
                self.settings_control_border(&id)
            },
        );
        self.write(
            rect,
            format!(" {label}"),
            self.item_style(&id, selected).bg(fill),
        );
        let name = CATEGORIES
            .iter()
            .find(|(id, _)| *id == category)
            .map_or(category, |(_, label)| *label);
        self.hit(id, rect, format!("{name} settings"));
    }

    fn compose_settings_search(&mut self, rect: Rect) {
        let border = if self.settings_search_focused {
            self.theme.accent
        } else {
            self.theme.border
        };
        self.rounded(rect, self.theme.card, border);
        let inset = u16::from(rect.width >= 4);
        let edit = Rect::new(
            rect.x + inset,
            rect.y + u16::from(rect.height >= 3),
            rect.width.saturating_sub(inset * 2),
            1,
        );
        self.hit(
            ElementId::SettingsSearch,
            rect,
            "Search settings (Command+F or Ctrl+F)",
        );
        self.editor_rect = edit;
        self.settings_query
            .keep_cursor_visible(usize::from(edit.width));
        let display = self.settings_query.display_text();
        let placeholder = display.is_empty() && !self.settings_search_focused;
        let text = if placeholder {
            "Search settings".into()
        } else {
            let mut column = 0;
            display
                .graphemes(true)
                .filter(|grapheme| {
                    let start = column;
                    column += grapheme.width();
                    start >= self.settings_query.scroll_columns
                })
                .collect::<String>()
        };
        self.write(
            edit,
            text,
            self.theme.base().bg(self.theme.card).fg(if placeholder {
                self.theme.muted
            } else {
                self.theme.foreground
            }),
        );
        if !self.settings_search_focused || self.overlay.is_some() {
            return;
        }
        if let Some(selection) = self.settings_query.selection_columns() {
            let start = selection
                .start
                .saturating_sub(self.settings_query.scroll_columns)
                .min(usize::from(edit.width)) as u16;
            let end = selection
                .end
                .saturating_sub(self.settings_query.scroll_columns)
                .min(usize::from(edit.width)) as u16;
            let selection = Rect::new(edit.x + start, edit.y, end - start, 1);
            let cells: Vec<_> = (selection.x..selection.right())
                .map(|x| self.staging[(x, edit.y)].clone())
                .collect();
            self.rounded(selection, self.theme.selected, self.theme.selected);
            for (x, cell) in (selection.x..selection.right()).zip(cells) {
                self.staging[(x, edit.y)] = cell;
                self.staging[(x, edit.y)].set_style(
                    Style::default()
                        .fg(self.theme.accent)
                        .bg(self.theme.selected),
                );
            }
        }
        let preedit = self.settings_query.preedit_columns();
        let start = preedit
            .start
            .saturating_sub(self.settings_query.scroll_columns)
            .min(usize::from(edit.width)) as u16;
        let end = preedit
            .end
            .saturating_sub(self.settings_query.scroll_columns)
            .min(usize::from(edit.width)) as u16;
        for x in edit.x + start..edit.x + end {
            self.staging[(x, edit.y)]
                .set_style(Style::default().add_modifier(Modifier::UNDERLINED));
        }
        if self.caret_visible && self.window_focused && edit.width > 0 {
            let x = edit.x
                + self
                    .settings_query
                    .cursor_columns()
                    .saturating_sub(self.settings_query.scroll_columns)
                    .min(usize::from(edit.width - 1)) as u16;
            self.cursor = Some(Position::new(x, edit.y));
        }
    }

    fn compose_setting_row(
        &mut self,
        model: &Model,
        field: &SettingDescriptor,
        index: usize,
        rect: Rect,
    ) {
        let id = ElementId::Setting(field.key.into());
        let selected = index == self.settings_selected
            && !self.settings_search_focused
            && self.focused.as_ref().is_none_or(|focused| focused == &id);
        let hovered = self.hovered.as_ref() == Some(&id);
        let owned = model.config_owned.contains(field.key) || self.config_owned.contains(field.key);
        let fill = if selected || hovered {
            self.theme.selected
        } else {
            self.theme.card
        };
        self.rounded(
            rect,
            fill,
            if selected {
                self.theme.accent
            } else {
                self.theme.border
            },
        );
        let inset = u16::from(rect.width >= 4);
        let width = rect.width.saturating_sub(inset * 2);
        let y = rect.y + u16::from(rect.height >= 3);
        let toggle = matches!(field.kind, SettingKind::Bool) && width >= 12;
        let value = value_label(model, field);
        let value = if toggle { format!(" {value} ") } else { value };
        let value_width = (value.width() as u16).min((width / 3).max(3)).min(width);
        let style = self.item_style(&id, selected).bg(fill).fg(if owned {
            self.theme.muted
        } else {
            self.theme.foreground
        });
        self.write(
            Rect::new(rect.x + inset, y, width.saturating_sub(value_width + 1), 1),
            field.label,
            style,
        );
        if value_width > 0 {
            let value_rect = Rect::new(rect.right() - inset - value_width, y, value_width, 1);
            let value_fill = if toggle { self.theme.background } else { fill };
            if toggle {
                self.rounded(
                    value_rect,
                    value_fill,
                    if owned {
                        self.theme.border
                    } else {
                        self.theme.accent
                    },
                );
            }
            self.write(
                value_rect,
                value,
                style.bg(value_fill).fg(if owned {
                    self.theme.muted
                } else {
                    self.theme.accent
                }),
            );
        }
        if rect.height >= 4 {
            self.write(
                Rect::new(rect.x + inset, y + 1, width, 1),
                if owned {
                    "Managed in Lua configuration"
                } else {
                    field.description
                },
                self.theme.secondary_on(fill),
            );
        }
        self.hit(
            id,
            rect,
            format!(
                "{}{}",
                field.description,
                if owned {
                    " (controlled by Lua configuration)"
                } else {
                    ". Click or press Enter to edit. Delete restores the default."
                }
            ),
        );
    }

    pub(crate) fn settings_select_category(&mut self, category: String) {
        if !CATEGORIES.iter().any(|(id, _)| *id == category) {
            return;
        }
        self.settings_category = category.clone();
        self.settings_selected = 0;
        self.settings_scroll = 0;
        self.settings_search_focused = false;
        self.focused = Some(ElementId::SettingsCategory(category));
        self.caret_deadline = None;
        self.dirty = true;
    }

    pub(crate) fn settings_input_text(&mut self, text: &str) {
        self.settings_query.insert(text);
        self.settings_selected = 0;
        self.settings_scroll = 0;
        self.settings_search_focused = true;
        self.focused = Some(ElementId::SettingsSearch);
        self.reset_caret();
        self.dirty = true;
    }

    pub(crate) fn settings_focus_setting(&mut self, _model: &Model, key: &str) {
        if let Some(index) = fields(&self.settings_category, self.settings_query.text())
            .iter()
            .position(|field| field.key == key)
        {
            self.settings_selected = index;
            self.settings_search_focused = false;
            self.focused = Some(ElementId::Setting(key.into()));
            self.caret_deadline = None;
            self.dirty = true;
        }
    }

    pub(crate) fn settings_scroll_by(&mut self, model: &Model, rows: i32) {
        let fields = fields(&self.settings_category, self.settings_query.text());
        self.settings_selected = self
            .settings_selected
            .saturating_add_signed(rows as isize)
            .min(fields.len().saturating_sub(1));
        if let Some(field) = fields.get(self.settings_selected) {
            self.settings_focus_setting(model, field.key);
        }
        self.dirty = true;
    }

    pub(crate) fn settings_key(
        &mut self,
        model: &Model,
        key: Key,
        mods: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) {
        if mods.command() && matches!(key, Key::Character('f' | 'F')) {
            self.settings_search_focused = true;
            self.focused = Some(ElementId::SettingsSearch);
            self.settings_query.select_all();
            self.reset_caret();
            self.dirty = true;
            return;
        }
        if key == Key::Tab {
            let ids: Vec<_> = self
                .hits
                .iter()
                .filter(|hit| self.page_rect.contains(hit.rect.as_position()))
                .map(|hit| hit.id.clone())
                .collect();
            if !ids.is_empty() {
                let at = self
                    .focused
                    .as_ref()
                    .and_then(|id| ids.iter().position(|candidate| candidate == id));
                let index = if mods.shift {
                    at.map_or(ids.len() - 1, |i| (i + ids.len() - 1) % ids.len())
                } else {
                    at.map_or(0, |i| (i + 1) % ids.len())
                };
                let id = ids[index].clone();
                self.settings_search_focused = id == ElementId::SettingsSearch;
                if let ElementId::Setting(key) = &id {
                    self.settings_focus_setting(model, key);
                }
                self.focused = Some(id);
                if self.settings_search_focused {
                    self.reset_caret();
                } else {
                    self.caret_deadline = None;
                }
                self.dirty = true;
            }
            return;
        }
        if self.settings_search_focused
            && !matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown)
        {
            match self.settings_query.key(&key, mods) {
                EditResult::Changed => {
                    self.settings_selected = 0;
                    self.settings_scroll = 0;
                    self.reset_caret();
                    self.dirty = true;
                }
                EditResult::Copy(text) => {
                    intents.push(UiIntent::SetClipboard(text));
                    self.settings_selected = 0;
                    self.settings_scroll = 0;
                    self.dirty = true;
                }
                EditResult::Paste => intents.push(UiIntent::RequestClipboard),
                EditResult::Submit => {
                    let fields = fields(&self.settings_category, self.settings_query.text());
                    if let Some(field) = fields.first() {
                        self.settings_focus_setting(model, field.key);
                    }
                }
                EditResult::Cancel => {
                    self.settings_search_focused = false;
                    self.focused = None;
                    self.caret_deadline = None;
                    self.dirty = true;
                }
                EditResult::Unhandled => {}
            }
            return;
        }
        if matches!(key, Key::Left | Key::Right)
            && let Some(ElementId::SettingsCategory(category)) = &self.focused
        {
            let at = CATEGORIES
                .iter()
                .position(|(id, _)| id == category)
                .unwrap_or(0);
            let next = if key == Key::Left {
                (at + CATEGORIES.len() - 1) % CATEGORIES.len()
            } else {
                (at + 1) % CATEGORIES.len()
            };
            self.settings_select_category(CATEGORIES[next].0.into());
            return;
        }
        let fields = fields(&self.settings_category, self.settings_query.text());
        match key {
            Key::Escape => self.close_settings(),
            Key::Down | Key::Up | Key::PageDown | Key::PageUp | Key::Home | Key::End => {
                self.settings_selected = match key {
                    Key::Home => 0,
                    Key::End => fields.len().saturating_sub(1),
                    Key::Down if self.settings_search_focused => 0,
                    Key::Down => (self.settings_selected + 1).min(fields.len().saturating_sub(1)),
                    Key::Up => self.settings_selected.saturating_sub(1),
                    Key::PageDown => {
                        (self.settings_selected + 5).min(fields.len().saturating_sub(1))
                    }
                    Key::PageUp => self.settings_selected.saturating_sub(5),
                    _ => self.settings_selected,
                };
                if let Some(field) = fields.get(self.settings_selected) {
                    self.settings_focus_setting(model, field.key);
                }
                self.dirty = true;
            }
            Key::Delete | Key::Backspace => {
                if let Some(field) = fields.get(self.settings_selected)
                    && self.focused.as_ref() == Some(&ElementId::Setting(field.key.into()))
                    && !model.config_owned.contains(field.key)
                {
                    intents.push(UiIntent::Domain(Intent::ResetSetting(field.key.into())));
                    self.dirty = true;
                }
            }
            Key::Enter | Key::Character(' ') | Key::Right => {
                if let Some(
                    id @ (ElementId::SettingsCategory(_)
                    | ElementId::CloseSettings
                    | ElementId::ResetSettings
                    | ElementId::SettingsSearch),
                ) = self.focused.clone()
                {
                    self.activate_element(model, id, intents);
                } else if let Some(field) = fields.get(self.settings_selected) {
                    self.edit_setting(model, field.key, intents);
                }
            }
            Key::Character(character) if !mods.command() && !mods.alt => {
                self.settings_input_text(&character.to_string())
            }
            _ => {}
        }
    }
}

use crate::{
    components::{
        button::Button,
        text_input::{Selection, TextInput},
    },
    runtime::canvas::Canvas,
    *,
};
use ratatui::style::Modifier;
use std::collections::BTreeSet;
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

pub(crate) struct SettingsPage {
    /// The page is showing; the sidebar keeps listing it while `listed`.
    pub open: bool,
    pub listed: bool,
    pub category: String,
    pub query: TextEditor,
    pub selected: usize,
    pub scroll: usize,
    pub search_focused: bool,
    pub rect: Rect,
    pub config_owned: BTreeSet<String>,
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self {
            open: false,
            listed: false,
            category: "all".into(),
            query: TextEditor::default(),
            selected: 0,
            scroll: 0,
            search_focused: false,
            rect: Rect::default(),
            config_owned: BTreeSet::new(),
        }
    }
}

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
        "behavior" => "General behavior",
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
        SettingKind::Object | SettingKind::Colors => {
            format!("{} entries", value.as_object().map_or(0, |v| v.len()))
        }
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

impl SettingsPage {
    pub fn render(&mut self, model: &Model, area: Rect, overlay_open: bool, cx: &mut Canvas) {
        if area.is_empty() {
            return;
        }
        let theme = cx.theme;
        self.rect = area;
        let spacious = area.height >= 24;
        let area = if spacious && area.width >= 48 {
            cx.rounded(area, theme.background);
            Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2)
        } else {
            area
        };
        cx.rounded(area, theme.background);
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
        Button::text(ElementId::CloseSettings, "×")
            .tooltip("Close settings (Escape)")
            .render(close, cx);
        cx.write(
            Rect::new(
                inner.x,
                inner.y,
                inner.width.saturating_sub(close.width + 1),
                1,
            ),
            "Settings",
            theme.base().add_modifier(Modifier::BOLD),
        );
        let mut y = inner.y + 1;
        let footer_height = u16::from(inner.height >= 9);
        let bottom = inner.bottom().saturating_sub(footer_height);
        if y < bottom.saturating_sub(2) {
            let height = if area.height >= 16 { 3 } else { 1 };
            let search = Rect::new(inner.x, y, inner.width, height.min(bottom - y));
            self.search(search, overlay_open, cx);
            y = search.bottom();
            if spacious && bottom.saturating_sub(y) > 9 {
                y += 1;
            }
        }
        if y + 2 < bottom && inner.width >= 7 {
            let reserve = (row_height(area) + 1).min(bottom.saturating_sub(y + 1));
            y = self.categories(Rect::new(inner.x, y, inner.width, bottom - y - reserve), cx);
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
        let fields = fields(&self.category, self.query.text());
        self.selected = self.selected.min(fields.len().saturating_sub(1));
        let mut height = row_height(area);
        if list.height < height + u16::from(height > 1) {
            height = 1;
        }
        let gap = u16::from(area.height >= 30 && height >= 4);
        let stride = height + gap;
        self.scroll = list::reveal(self.scroll, self.selected, |start| {
            visible_end(&fields, start, list.height, stride)
        });
        let end = visible_end(&fields, self.scroll, list.height, stride);
        let mut group = "";
        for (index, field) in fields.iter().enumerate().take(end).skip(self.scroll) {
            if height > 1 && field.group != group {
                cx.write(
                    Rect::new(list.x, y, list.width, 1),
                    group_label(field.group),
                    theme.muted(),
                );
                y += 1;
                group = field.group;
            }
            self.setting(
                model,
                field,
                index,
                Rect::new(list.x, y, list.width, height),
                cx,
            );
            y += stride;
        }
        if fields.is_empty() && list.height > 0 {
            cx.write(
                Rect::new(list.x, list.y, list.width, 1),
                "No results",
                theme.muted(),
            );
        }
        if footer_height > 0 {
            Button::text(ElementId::ResetSettings, "Reset defaults")
                .tooltip("Reset saved settings to defaults")
                .render(Rect::new(inner.x, bottom, inner.width.min(16), 1), cx);
        }
    }

    /// Wraps the chips onto more rows, or pages through them one at a time when they cannot fit.
    fn categories(&self, area: Rect, cx: &mut Canvas) -> u16 {
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
                .position(|(id, _)| *id == self.category)
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
                self.chip(CATEGORIES[index], text, rect, cx);
            }
            return area.y + 1;
        }
        let mut x = area.x;
        let mut y = area.y;
        for &category in CATEGORIES {
            let width = (category.1.width() as u16 + 2).min(area.width);
            if x + width > area.right() {
                x = area.x;
                y += 1;
            }
            self.chip(category, category.1, Rect::new(x, y, width, 1), cx);
            x += width + 1;
        }
        y + 1
    }

    fn chip(&self, (category, name): (&str, &str), text: &str, rect: Rect, cx: &mut Canvas) {
        Button::text(ElementId::SettingsCategory(category.into()), text)
            .tooltip(format!("{name} settings"))
            .selected(self.category == category)
            .render(rect, cx);
    }

    fn search(&mut self, rect: Rect, overlay_open: bool, cx: &mut Canvas) {
        let theme = cx.theme;
        cx.rounded(rect, theme.card);
        let inset = u16::from(rect.width >= 4);
        let edit = Rect::new(
            rect.x + inset,
            rect.y + u16::from(rect.height >= 3),
            rect.width.saturating_sub(inset * 2),
            1,
        );
        cx.hit(
            ElementId::SettingsSearch,
            rect,
            "Search settings (Command+F or Ctrl+F)",
        );
        TextInput::new(ElementId::SettingsSearch, &mut self.query, theme.card)
            .active(self.search_focused && !overlay_open)
            .placeholder((!self.search_focused).then_some("Search settings"))
            .selection(Selection::Subtle)
            .render(edit, cx);
    }

    fn setting(
        &self,
        model: &Model,
        field: &SettingDescriptor,
        index: usize,
        rect: Rect,
        cx: &mut Canvas,
    ) {
        let theme = cx.theme;
        let id = ElementId::Setting(field.key.into());
        let selected = index == self.selected
            && !self.search_focused
            && cx.focused.is_none_or(|focused| focused == &id);
        let owned = model.config_owned.contains(field.key) || self.config_owned.contains(field.key);
        let fill = if selected || cx.hovered(&id) {
            theme.selected
        } else {
            theme.card
        };
        cx.rounded(rect, fill);
        let inset = u16::from(rect.width >= 4);
        let width = rect.width.saturating_sub(inset * 2);
        let y = rect.y + u16::from(rect.height >= 3);
        let toggle = matches!(field.kind, SettingKind::Bool) && width >= 12;
        let value = value_label(model, field);
        let value = if toggle { format!(" {value} ") } else { value };
        let value_width = (value.width() as u16).min((width / 3).max(3)).min(width);
        let style = cx.item_style(&id, selected).bg(fill).fg(if owned {
            theme.muted
        } else {
            theme.foreground
        });
        cx.write(
            Rect::new(rect.x + inset, y, width.saturating_sub(value_width + 1), 1),
            field.label,
            style,
        );
        if value_width > 0 {
            let value_rect = Rect::new(rect.right() - inset - value_width, y, value_width, 1);
            let value_fill = if toggle { theme.background } else { fill };
            if toggle {
                cx.rounded(value_rect, value_fill);
            }
            cx.write(
                value_rect,
                value,
                style
                    .bg(value_fill)
                    .fg(if owned { theme.muted } else { theme.accent }),
            );
        }
        if rect.height >= 4 {
            cx.write(
                Rect::new(rect.x + inset, y + 1, width, 1),
                if owned {
                    "Managed in Lua configuration"
                } else {
                    field.description
                },
                theme.secondary_on(fill),
            );
        }
        cx.hit(
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
}

impl SidebarUi {
    pub(crate) fn settings_select_category(&mut self, category: String) {
        if !CATEGORIES.iter().any(|(id, _)| *id == category) {
            return;
        }
        self.settings.category = category.clone();
        self.settings.selected = 0;
        self.settings.scroll = 0;
        self.settings.search_focused = false;
        self.focused = Some(ElementId::SettingsCategory(category));
        self.caret.stop();
        self.frame.dirty = true;
    }

    pub(crate) fn settings_input_text(&mut self, text: &str) {
        self.settings.query.insert(text);
        self.settings.selected = 0;
        self.settings.scroll = 0;
        self.settings.search_focused = true;
        self.focused = Some(ElementId::SettingsSearch);
        self.reset_caret();
        self.frame.dirty = true;
    }

    pub(crate) fn settings_focus_setting(&mut self, _model: &Model, key: &str) {
        if let Some(index) = fields(&self.settings.category, self.settings.query.text())
            .iter()
            .position(|field| field.key == key)
        {
            self.settings.selected = index;
            self.settings.search_focused = false;
            self.focused = Some(ElementId::Setting(key.into()));
            self.caret.stop();
            self.frame.dirty = true;
        }
    }

    pub(crate) fn settings_scroll_by(&mut self, model: &Model, rows: i32) {
        let fields = fields(&self.settings.category, self.settings.query.text());
        self.settings.selected = self
            .settings
            .selected
            .saturating_add_signed(rows as isize)
            .min(fields.len().saturating_sub(1));
        if let Some(field) = fields.get(self.settings.selected) {
            self.settings_focus_setting(model, field.key);
        }
        self.frame.dirty = true;
    }

    pub(crate) fn settings_key(
        &mut self,
        model: &Model,
        key: Key,
        mods: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) {
        if mods.command() && matches!(key, Key::Character('f' | 'F')) {
            self.settings.search_focused = true;
            self.focused = Some(ElementId::SettingsSearch);
            self.settings.query.select_all();
            self.reset_caret();
            self.frame.dirty = true;
            return;
        }
        if key == Key::Tab {
            let ids: Vec<_> = self
                .paint
                .hits
                .iter()
                .filter(|hit| self.settings.rect.contains(hit.rect.as_position()))
                .map(|hit| hit.id.clone())
                .collect();
            if let Some(id) = list::cycle(&ids, self.focused.as_ref(), mods.shift) {
                self.settings.search_focused = id == ElementId::SettingsSearch;
                if let ElementId::Setting(key) = &id {
                    self.settings_focus_setting(model, key);
                }
                self.focused = Some(id);
                if self.settings.search_focused {
                    self.reset_caret();
                } else {
                    self.caret.stop();
                }
                self.frame.dirty = true;
            }
            return;
        }
        if self.settings.search_focused
            && !matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown)
        {
            match self.settings.query.key(&key, mods) {
                EditResult::Changed => {
                    self.settings.selected = 0;
                    self.settings.scroll = 0;
                    self.reset_caret();
                    self.frame.dirty = true;
                }
                EditResult::Copy(text) => {
                    intents.push(UiIntent::SetClipboard(text));
                    self.settings.selected = 0;
                    self.settings.scroll = 0;
                    self.frame.dirty = true;
                }
                EditResult::Paste => intents.push(UiIntent::RequestClipboard),
                EditResult::Submit => {
                    let fields = fields(&self.settings.category, self.settings.query.text());
                    if let Some(field) = fields.first() {
                        self.settings_focus_setting(model, field.key);
                    }
                }
                EditResult::Cancel => {
                    self.settings.search_focused = false;
                    self.focused = None;
                    self.caret.stop();
                    self.frame.dirty = true;
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
        let fields = fields(&self.settings.category, self.settings.query.text());
        match key {
            Key::Escape => self.close_settings(),
            Key::Down | Key::Up | Key::PageDown | Key::PageUp | Key::Home | Key::End => {
                self.settings.selected = match key {
                    Key::Home => 0,
                    Key::End => fields.len().saturating_sub(1),
                    Key::Down if self.settings.search_focused => 0,
                    Key::Down => (self.settings.selected + 1).min(fields.len().saturating_sub(1)),
                    Key::Up => self.settings.selected.saturating_sub(1),
                    Key::PageDown => {
                        (self.settings.selected + 5).min(fields.len().saturating_sub(1))
                    }
                    Key::PageUp => self.settings.selected.saturating_sub(5),
                    _ => self.settings.selected,
                };
                if let Some(field) = fields.get(self.settings.selected) {
                    self.settings_focus_setting(model, field.key);
                }
                self.frame.dirty = true;
            }
            Key::Delete | Key::Backspace => {
                if let Some(field) = fields.get(self.settings.selected)
                    && self.focused.as_ref() == Some(&ElementId::Setting(field.key.into()))
                    && !model.config_owned.contains(field.key)
                {
                    intents.push(UiIntent::Domain(Intent::ResetSetting(field.key.into())));
                    self.frame.dirty = true;
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
                } else if let Some(field) = fields.get(self.settings.selected) {
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

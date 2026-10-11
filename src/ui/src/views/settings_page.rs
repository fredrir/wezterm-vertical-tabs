use crate::SidebarUi;
use crate::components::button::{Button, Tone};
use crate::components::scrollbar::Scrollbar;
use crate::components::text_input::TextInput;
use crate::components::{ICON_CELLS, ROW_INSET, SURFACE_RADIUS, list};
use crate::element::{ElementId, ScrollArea};
use crate::events::EditorSlot;
use crate::icons;
use crate::input::{EditResult, Key, Modifiers, TextEditor, display_text};
use crate::intent::UiIntent;
use crate::runtime::canvas::{Canvas, RoundedSurface};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use unicode_width::UnicodeWidthStr;
use vtabs_core::{Intent, Model, SettingDescriptor, SettingKind, keybinds, settings};

const CATEGORIES: &[(&str, &str, &str)] = &[
    ("all", "All", icons::ALL),
    ("layout", "Layout", icons::LAYOUT),
    ("quick_terminal", "Quick terminal", icons::TERMINAL),
    ("appearance", "Tabs", icons::TAB),
    ("theme", "Colors", icons::PALETTE),
    ("motion", "Motion", icons::MOTION),
    ("behavior", "General", icons::TUNE),
    ("keybinds", "Keybinds", icons::KEYBOARD_OUTLINE),
];

const VALUE_CELLS: u16 = 5;

pub(crate) enum Capture {
    Search,
    Chord {
        key: String,
        index: Option<usize>,
        conflict: Option<String>,
    },
}

pub(crate) struct Entry {
    pub key: String,
    pub editor: TextEditor,
}

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
    pub search_focused: bool,
    pub rect: Rect,
    pub scrollbar: Option<Scrollbar>,
    pub config_owned: BTreeSet<String>,
    pub capture: Option<Capture>,
    pub entry: Option<Entry>,
    pub drag: Option<(String, f64)>,
    pub chip: Option<usize>,
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
            search_focused: false,
            rect: Rect::default(),
            scrollbar: None,
            config_owned: BTreeSet::new(),
            capture: None,
            entry: None,
            drag: None,
            chip: None,
            chord_label: crate::keybinds::display_chord,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Range {
    min: f64,
    max: f64,
    step: f64,
    percent: bool,
}

impl Range {
    pub(crate) fn of(kind: &SettingKind) -> Option<Self> {
        let percent = |min| Self {
            min,
            max: 1.,
            step: 0.01,
            percent: true,
        };
        match *kind {
            SettingKind::Number { min, max } => Some(Self {
                min: min.into(),
                max: max.into(),
                step: if max - min > 100 { 10. } else { 1. },
                percent: false,
            }),
            SettingKind::Fraction => Some(percent(0.01)),
            SettingKind::Opacity => Some(percent(0.)),
            _ => None,
        }
    }
    fn bound(self, value: f64) -> f64 {
        (value.max(self.min).min(self.max) * 100.).round() / 100.
    }
    /// Step multiples, except that both ends stay reachable.
    fn snap(self, value: f64) -> f64 {
        if value >= self.max - self.step / 2. {
            self.max
        } else if value <= self.min + self.step / 2. {
            self.min
        } else {
            self.bound((value / self.step).round() * self.step)
        }
    }
    pub(crate) fn step(self, value: f64, steps: i32) -> f64 {
        self.snap(value + self.step * f64::from(steps))
    }
    pub(crate) fn at(self, fraction: f32) -> f64 {
        self.snap(self.min + f64::from(fraction.clamp(0., 1.)) * (self.max - self.min))
    }
    fn fraction(self, value: f64) -> f64 {
        ((value - self.min) / (self.max - self.min)).clamp(0., 1.)
    }
    fn label(self, value: f64) -> String {
        if self.percent {
            format!("{}%", (value * 100.).round())
        } else {
            format!("{}", value.round())
        }
    }
    fn entry_text(self, value: f64) -> String {
        if self.percent {
            format!("{}", (value * 100.).round())
        } else {
            format!("{}", value.round())
        }
    }
    /// Typed digits in display units; the upper bound applies while typing.
    pub(crate) fn sanitize(self, text: &str) -> String {
        let digits: String = text.chars().filter(char::is_ascii_digit).take(5).collect();
        let limit = if self.percent { 100. } else { self.max };
        match digits.parse::<f64>() {
            Ok(typed) if typed > limit => format!("{limit}"),
            _ => digits,
        }
    }
    fn parse(self, text: &str) -> Option<f64> {
        let typed = text.parse::<f64>().ok()?;
        Some(self.bound(if self.percent { typed / 100. } else { typed }))
    }
    pub(crate) fn json(self, value: f64) -> Value {
        if self.percent {
            json!(value)
        } else {
            json!(value.round() as u64)
        }
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

pub(crate) fn choice_label(choice: &str) -> String {
    capitalized(choice)
}

fn action(field: &SettingDescriptor) -> Option<&'static str> {
    field.key.strip_prefix("shortcut.")
}

fn number(model: &Model, key: &str) -> f64 {
    model
        .settings
        .get(key)
        .and_then(|value| value.as_f64())
        .unwrap_or_default()
}

fn dimmed(model: &Model, key: &str) -> bool {
    key == "quick_terminal_blur" && model.settings.quick_terminal_opacity >= 1.
}

fn heading(fields: &[&SettingDescriptor], index: usize, headings: bool) -> u16 {
    match index {
        _ if !headings => 0,
        0 => 1,
        _ if fields[index - 1].group != fields[index].group => 2,
        _ => 0,
    }
}

fn visible_end(
    fields: &[&SettingDescriptor],
    start: usize,
    height: u16,
    line: u16,
    headings: bool,
) -> usize {
    let mut remaining = height;
    let mut end = start;
    for index in start..fields.len() {
        let needed = heading(fields, index, headings) + line;
        if needed > remaining {
            break;
        }
        remaining -= needed;
        end += 1;
    }
    end
}

struct Pill {
    id: Option<ElementId>,
    remove: Option<ElementId>,
    text: String,
    fill: Color,
    fg: Color,
}

impl SettingsPage {
    pub(crate) fn managed_in_lua(&self, model: &Model, key: &str) -> bool {
        let key = if key.starts_with("shortcut.") {
            "keybinds"
        } else {
            key
        };
        model.config_owned.contains(key) || self.config_owned.contains(key)
    }

    pub(crate) fn fields(&self, model: &Model) -> Vec<&'static SettingDescriptor> {
        if let Some(chord) = &self.chord {
            return keybinds::DESCRIPTORS
                .iter()
                .filter(|field| {
                    action(field).is_some_and(|action| {
                        keybinds::bindings(&model.settings, action).contains(chord)
                    })
                })
                .collect();
        }
        let query = self.query.text().trim().to_lowercase();
        settings::descriptors()
            .iter()
            .filter(|field| field.key != "keybinds")
            .chain(keybinds::DESCRIPTORS.iter())
            .filter(|field| self.category == "all" || field.group == self.category)
            .filter(|field| {
                query.is_empty()
                    || [field.key, field.label, field.description, field.group]
                        .iter()
                        .any(|text| text.to_lowercase().contains(&query))
            })
            .collect()
    }

    pub(crate) fn recording(&self) -> bool {
        self.capture.is_some()
    }

    pub fn render(&mut self, model: &Model, area: Rect, overlay_open: bool, cx: &mut Canvas) {
        self.scrollbar = None;
        if area.is_empty() {
            return;
        }
        let theme = cx.theme;
        self.rect = area;
        let area = if area.height >= 24 && area.width >= 48 {
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
        let theme = cx.theme;
        cx.surface(rect, theme.card, SURFACE_RADIUS, ROW_INSET);
        let lead = ICON_CELLS.min(rect.width.saturating_sub(1));
        cx.write(
            Rect::new(rect.x + 1, rect.y, lead, 1),
            icons::SEARCH,
            theme.muted().bg(theme.card),
        );
        let button = if rect.width >= 16 { 3 } else { 0 };
        let edit = Rect::new(
            rect.x + 1 + lead,
            rect.y,
            rect.width.saturating_sub(lead + 2 + button),
            1,
        );
        let recording = matches!(self.capture, Some(Capture::Search));
        cx.hit(
            ElementId::SettingsSearch,
            Rect::new(rect.x, rect.y, rect.width - button, rect.height),
            "",
        );
        TextInput::new(ElementId::SettingsSearch, &mut self.query, theme.card)
            .active(self.search_focused && !overlay_open && !recording)
            .shift(if rect.height == 2 { 0.5 } else { 0.0 })
            .placeholder(if recording {
                "Recording keys"
            } else {
                "Search settings"
            })
            .render(edit, cx);
        if button > 0 {
            Button::action(ElementId::SettingsRecord, icons::KEYBOARD, Tone::Neutral)
                .selected(recording)
                .label_right_padding(1)
                .render(
                    Rect::new(rect.right() - button - 1, rect.y, button + 1, rect.height),
                    cx,
                );
        }
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
        cx.surface(rect, fill, SURFACE_RADIUS, ROW_INSET);
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

    fn list(&mut self, model: &Model, area: Rect, line: u16, cx: &mut Canvas) {
        let theme = cx.theme;
        let fields = self.fields(model);
        self.selected = self.selected.min(fields.len().saturating_sub(1));
        if fields.is_empty() {
            self.max_scroll = 0;
            if area.height > 0 {
                cx.write(
                    Rect::new(area.x + 1, area.y, area.width.saturating_sub(1), 1),
                    "No results",
                    theme.muted(),
                );
            }
            return;
        }
        let headings = self.category == "all" && self.chord.is_none() && line > 1;
        let end = |start| visible_end(&fields, start, area.height, line, headings);
        self.max_scroll = (0..fields.len())
            .find(|start| end(*start) >= fields.len())
            .unwrap_or(fields.len() - 1);
        if self.revealed != Some(self.selected) {
            self.scroll = list::reveal(self.scroll, self.selected, end);
            self.revealed = Some(self.selected);
        }
        self.scroll = self.scroll.min(self.max_scroll);
        let mut list = area;
        self.scrollbar = Scrollbar::new(
            Rect::new(
                area.right().saturating_sub(1),
                area.y,
                u16::from(area.width > 8),
                area.height,
            ),
            fields.len() - self.max_scroll,
            fields.len(),
            self.scroll,
        );
        if let Some(scrollbar) = self.scrollbar {
            list.width -= 2;
            scrollbar.render(ScrollArea::Settings, cx);
        }
        let mut y = list.y;
        for index in self.scroll..end(self.scroll) {
            let field = fields[index];
            let above = heading(&fields, index, headings);
            if above > 0 {
                let name = CATEGORIES
                    .iter()
                    .find(|(id, ..)| *id == field.group)
                    .map_or(field.group, |(_, name, _)| name);
                cx.write(
                    Rect::new(list.x + 1, y + above - 1, list.width.saturating_sub(1), 1),
                    name,
                    theme.muted().add_modifier(Modifier::BOLD),
                );
                y += above;
            }
            self.setting(
                model,
                field,
                index,
                Rect::new(list.x, y, list.width, line),
                cx,
            );
            y += line;
        }
    }

    fn setting(
        &mut self,
        model: &Model,
        field: &SettingDescriptor,
        index: usize,
        rect: Rect,
        cx: &mut Canvas,
    ) {
        let theme = cx.theme;
        let id = ElementId::Setting(field.key.into());
        let focused = index == self.selected
            && !self.search_focused
            && cx
                .focused
                .is_none_or(|focused| focused.setting() == Some(field.key));
        let hovered = cx.pointer.hovered.as_ref().and_then(ElementId::setting) == Some(field.key);
        let owned = self.managed_in_lua(model, field.key);
        let fill = if focused {
            theme.selected
        } else if hovered {
            theme.lift(theme.card, 5)
        } else {
            theme.card
        };
        cx.surface(rect, fill, SURFACE_RADIUS, ROW_INSET);
        cx.hit(id, rect, "");
        let pad = if rect.width >= 24 {
            2
        } else {
            u16::from(rect.width >= 4)
        };
        let content = Rect::new(
            rect.x + pad,
            rect.y,
            rect.width.saturating_sub(pad * 2),
            rect.height,
        );
        let muted = owned || dimmed(model, field.key);
        let mut used = self.control(
            model, field, content, fill, focused, hovered, owned, muted, cx,
        );
        if owned && content.width > used + 4 {
            used += ICON_CELLS;
            cx.write(
                Rect::new(content.right() - used, content.y, 2, 1),
                icons::LOCK,
                theme.base().bg(fill).fg(theme.muted),
            );
        }
        cx.write(
            Rect::new(
                content.x,
                content.y,
                content.width.saturating_sub(used + 1),
                1,
            ),
            field.label,
            theme
                .base()
                .bg(fill)
                .fg(if muted { theme.muted } else { theme.foreground }),
        );
    }

    /// Draws the value control against the right edge and returns the cells it takes.
    #[allow(clippy::too_many_arguments)]
    fn control(
        &mut self,
        model: &Model,
        field: &SettingDescriptor,
        area: Rect,
        fill: Color,
        focused: bool,
        hovered: bool,
        owned: bool,
        muted: bool,
        cx: &mut Canvas,
    ) -> u16 {
        let theme = cx.theme;
        if let Some(action) = action(field) {
            return self.chords(
                model, field.key, action, area, fill, focused, hovered, owned, cx,
            );
        }
        if let Some(range) = Range::of(&field.kind) {
            return self.slider(
                field.key,
                range,
                number(model, field.key),
                area,
                fill,
                muted,
                cx,
            );
        }
        let value = model.settings.get(field.key).unwrap_or_default();
        let fg = if muted { theme.muted } else { theme.foreground };
        let pill = |text: String| Pill {
            id: None,
            remove: None,
            text,
            fill: theme.lift(fill, 8),
            fg,
        };
        match field.kind {
            SettingKind::Bool => switch(cx, area, fill, value.as_bool().unwrap_or(false), muted),
            SettingKind::Choice(_) => pills(
                cx,
                area,
                vec![pill(format!(
                    "{}  {}",
                    choice_label(value.as_str().unwrap_or_default()),
                    icons::SELECT
                ))],
            ),
            SettingKind::Color | SettingKind::OptionalColor => match value.as_str() {
                Some(hex) => {
                    let used = pills(cx, area, vec![pill(format!("   {hex}"))]);
                    if let Some(color) = crate::Theme::parse_color(hex).filter(|_| used > 0) {
                        let swatch = Rect::new(area.right() + 3 - used, area.y, 2, area.height);
                        cx.mark(RoundedSurface {
                            square: true,
                            ..RoundedSurface::new(swatch, color, 3.0, 3.0)
                        });
                    }
                    used
                }
                None => pills(cx, area, vec![pill("Auto".into())]),
            },
            SettingKind::Text => pills(
                cx,
                area,
                vec![pill(
                    value.as_str().map_or_else(|| "Auto".into(), display_text),
                )],
            ),
            SettingKind::Object | SettingKind::Colors => pills(
                cx,
                area,
                vec![pill(format!(
                    "{} entries",
                    value.as_object().map_or(0, |v| v.len())
                ))],
            ),
            SettingKind::List => pills(
                cx,
                area,
                vec![pill(format!(
                    "{} items",
                    value.as_array().map_or(0, |v| v.len())
                ))],
            ),
            _ => 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(
        &mut self,
        key: &str,
        range: Range,
        value: f64,
        area: Rect,
        fill: Color,
        muted: bool,
        cx: &mut Canvas,
    ) -> u16 {
        let theme = cx.theme;
        let entry = self.entry.as_mut().filter(|entry| entry.key == key);
        let value = match (&entry, &self.drag) {
            (Some(entry), _) => range.parse(entry.editor.text()).unwrap_or(value),
            (_, Some((dragged, preview))) if dragged == key => *preview,
            _ => value,
        };
        let text = Rect::new(
            area.right().saturating_sub(VALUE_CELLS),
            area.y,
            VALUE_CELLS.min(area.width),
            1,
        );
        let fg = if muted { theme.muted } else { theme.foreground };
        if let Some(entry) = entry {
            let field = Rect::new(
                text.x.saturating_sub(1),
                area.y,
                text.width + 2,
                area.height,
            );
            let surface = theme.lift(fill, 10);
            cx.fill(RoundedSurface::new(
                field,
                surface,
                6.0,
                if area.height > 1 { 4.0 } else { 0.0 },
            ));
            let typed = (entry.editor.text().width() as u16 + 1).clamp(2, text.width);
            TextInput::new(ElementId::SettingsEntry, &mut entry.editor, surface)
                .active(true)
                .shift(if area.height == 2 { 0.5 } else { 0.0 })
                .render(Rect::new(text.right() - typed, text.y, typed, 1), cx);
        } else {
            cx.write(
                text,
                Line::from(range.label(value)).alignment(Alignment::Right),
                theme.base().bg(fill).fg(fg),
            );
        }
        let width = (area.width * 2 / 5)
            .min(40)
            .min(area.width.saturating_sub(VALUE_CELLS + 14));
        if width < 6 {
            return VALUE_CELLS;
        }
        let track = Rect::new(text.x - 2 - width, area.y, width, area.height);
        let knob = track.x + (range.fraction(value) * f64::from(width - 2)).round() as u16;
        let thin = 0.24 / f32::from(area.height);
        let (rail, filled, handle) = if muted {
            (theme.lift(fill, 10), theme.lift(fill, 24), theme.muted)
        } else {
            (theme.lift(fill, 14), theme.accent, theme.foreground)
        };
        cx.mark(RoundedSurface {
            scale_y: thin,
            ..RoundedSurface::new(track, rail, 2.0, 0.0)
        });
        cx.mark(RoundedSurface {
            scale_y: thin,
            ..RoundedSurface::new(
                Rect::new(track.x, track.y, knob + 1 - track.x, track.height),
                filled,
                2.0,
                0.0,
            )
        });
        cx.mark(RoundedSurface {
            square: true,
            ..RoundedSurface::new(Rect::new(knob, track.y, 2, track.height), handle, 6.0, 2.0)
        });
        cx.hit(ElementId::Slider(key.into()), track, "");
        width + 2 + VALUE_CELLS
    }

    #[allow(clippy::too_many_arguments)]
    fn chords(
        &self,
        model: &Model,
        key: &str,
        action: &str,
        area: Rect,
        fill: Color,
        focused: bool,
        hovered: bool,
        owned: bool,
        cx: &mut Canvas,
    ) -> u16 {
        let theme = cx.theme;
        let chords = keybinds::bindings(&model.settings, action);
        let (recording, conflict) = match &self.capture {
            Some(Capture::Chord {
                key: target,
                index,
                conflict,
            }) if target == key => (Some(*index), conflict.as_deref()),
            _ => (None, None),
        };
        let clash = match &self.capture {
            Some(Capture::Chord {
                conflict: Some(chord),
                ..
            }) => Some(chord.as_str()),
            _ => None,
        };
        let recorder = |text: Option<&str>| Pill {
            id: None,
            remove: None,
            text: text.map_or_else(|| "●".into(), self.chord_label),
            fill: if text.is_some() {
                theme.warn(fill, 45)
            } else {
                theme.tint(fill, 30)
            },
            fg: if text.is_some() {
                theme.foreground
            } else {
                theme.accent
            },
        };
        let mut row = Vec::new();
        for (index, chord) in chords.iter().enumerate() {
            if recording == Some(Some(index)) {
                row.push(recorder(conflict));
                continue;
            }
            let id = ElementId::Chord(key.into(), index);
            let remove = ElementId::RemoveChord(key.into(), chord.clone());
            let chip = focused && self.chip == Some(index) && recording.is_none();
            let pointed = cx.hovered(&id) || cx.hovered(&remove);
            row.push(Pill {
                fill: if clash == Some(chord.as_str()) {
                    theme.warn(fill, 30)
                } else if chip {
                    theme.tint(fill, 30)
                } else if pointed {
                    theme.lift(fill, 16)
                } else {
                    theme.lift(fill, 8)
                },
                fg: if owned { theme.muted } else { theme.foreground },
                remove: (!owned && (chip || pointed)).then_some(remove),
                id: (!owned).then_some(id),
                text: (self.chord_label)(chord),
            });
        }
        if recording == Some(None) {
            row.push(recorder(conflict));
        } else if !owned && recording.is_none() && (focused || hovered || chords.is_empty()) {
            let id = ElementId::AddChord(key.into());
            let active = (focused && self.chip == Some(chords.len())) || cx.hovered(&id);
            row.push(Pill {
                fill: if active {
                    theme.tint(fill, 30)
                } else {
                    theme.lift(fill, 8)
                },
                fg: if active { theme.accent } else { theme.muted },
                id: Some(id),
                remove: None,
                text: "+".into(),
            });
        }
        pills(cx, area, row)
    }
}

/// Right-aligned rounded chips; the leftmost drop when the row runs out of room.
fn pills(cx: &mut Canvas, area: Rect, pills: Vec<Pill>) -> u16 {
    let inset = if area.height > 1 { 4.0 } else { 1.0 };
    let mut right = area.right();
    let floor = area.x + (area.width / 3).min(16);
    for pill in pills.into_iter().rev() {
        let width = pill.text.width() as u16 + 4;
        if right < floor + width {
            break;
        }
        let rect = Rect::new(right - width, area.y, width, area.height);
        cx.fill(RoundedSurface::new(rect, pill.fill, 7.0, inset));
        let style = cx.theme.base().bg(pill.fill).fg(pill.fg);
        let shifted = u16::from(pill.remove.is_some());
        cx.write(
            Rect::new(rect.x + 2 - shifted, rect.y, width - 2, 1),
            pill.text,
            style,
        );
        if let Some(id) = pill.id {
            cx.hit(id, rect, "");
        }
        if let Some(remove) = pill.remove {
            let close = Rect::new(rect.right() - 2, rect.y, 2, rect.height);
            cx.write(
                Rect::new(close.x, close.y, 1, 1),
                "×",
                style.fg(if cx.hovered(&remove) {
                    cx.theme.danger
                } else {
                    cx.theme.muted
                }),
            );
            cx.hit(remove, close, "");
        }
        right = rect.x.saturating_sub(1);
    }
    area.right() - right.min(area.right())
}

fn switch(cx: &mut Canvas, area: Rect, fill: Color, on: bool, muted: bool) -> u16 {
    if area.width < 6 {
        return 0;
    }
    let theme = cx.theme;
    let track = Rect::new(area.right() - 4, area.y, 4, area.height);
    let (rail, knob) = match (on, muted) {
        (true, false) => (theme.accent, theme.background),
        (true, true) => (theme.lift(fill, 30), fill),
        (false, _) => (theme.lift(fill, 14), theme.muted),
    };
    cx.mark(RoundedSurface {
        scale_y: 1. / f32::from(area.height),
        ..RoundedSurface::new(track, rail, 9.0, 0.0)
    });
    let x = if on { track.x + 2 } else { track.x };
    cx.mark(RoundedSurface {
        square: true,
        ..RoundedSurface::new(Rect::new(x, track.y, 2, track.height), knob, 6.0, 2.0)
    });
    4
}

impl SidebarUi {
    pub fn chord_label(&self) -> fn(&str) -> String {
        self.settings.chord_label
    }

    pub fn set_chord_label(&mut self, label: fn(&str) -> String) {
        self.settings.chord_label = label;
        self.frame.dirty = true;
    }

    pub(crate) fn settings_select_category(&mut self, category: String) {
        if !CATEGORIES.iter().any(|(id, ..)| *id == category) {
            return;
        }
        if self.settings.chord.take().is_some() {
            self.settings.query = TextEditor::default();
        }
        self.settings.category = category.clone();
        self.settings.selected = 0;
        self.settings.scroll = 0;
        self.settings.chip = None;
        self.settings.search_focused = false;
        self.focused = Some(ElementId::SettingsCategory(category));
        self.caret.stop();
        self.frame.dirty = true;
    }

    fn settings_input_text(&mut self, text: &str) {
        self.settings.search_focused = true;
        self.focused = Some(ElementId::SettingsSearch);
        self.settings.query.insert(text);
        self.edited(EditorSlot::SettingsSearch);
    }

    pub(crate) fn settings_focus_setting(&mut self, model: &Model, key: &str) {
        if let Some(index) = self
            .settings
            .fields(model)
            .iter()
            .position(|field| field.key == key)
        {
            if index != self.settings.selected {
                self.settings.chip = None;
            }
            self.settings.selected = index;
            self.settings.search_focused = false;
            self.focused = Some(ElementId::Setting(key.into()));
            if self.settings.entry.is_none() {
                self.caret.stop();
            }
            self.frame.dirty = true;
        }
    }

    pub(crate) fn settings_scroll_by(&mut self, rows: i32) {
        self.settings.scroll =
            list::scroll_by(self.settings.scroll, rows, self.settings.max_scroll);
        self.frame.dirty = true;
    }

    pub(crate) fn toggle_settings_recording(&mut self, intents: &mut Vec<UiIntent>) {
        self.commit_entry(intents);
        self.settings.capture = match self.settings.capture {
            Some(Capture::Search) => None,
            _ => Some(Capture::Search),
        };
        self.settings.search_focused = false;
        self.focused = Some(ElementId::SettingsRecord);
        self.caret.stop();
        self.frame.dirty = true;
    }

    pub(crate) fn record_chord(&mut self, model: &Model, key: &str, index: Option<usize>) {
        if self.settings.managed_in_lua(model, key) {
            return;
        }
        self.settings_focus_setting(model, key);
        self.settings.capture = Some(Capture::Chord {
            key: key.into(),
            index,
            conflict: None,
        });
        self.frame.dirty = true;
    }

    /// Returns whether the settings page owned the recording.
    pub(crate) fn settings_recorded(
        &mut self,
        model: &Model,
        chord: &str,
        intents: &mut Vec<UiIntent>,
    ) -> bool {
        let Some(capture) = self.settings.capture.take() else {
            return false;
        };
        self.frame.dirty = true;
        if chord.is_empty() {
            return true;
        }
        match capture {
            Capture::Search => {
                self.settings.chord = Some(chord.into());
                self.settings.query = TextEditor::new((self.settings.chord_label)(chord));
                self.settings.selected = 0;
                self.settings.scroll = 0;
                self.settings.capture = Some(Capture::Search);
            }
            Capture::Chord { key, index, .. } => {
                let Some(action) = key.strip_prefix("shortcut.") else {
                    return true;
                };
                let bindings =
                    super::keybinds::changed_keybinds(model, action, index, Some(chord.into()));
                if keybinds::validate(&bindings).is_ok() {
                    intents.push(UiIntent::Domain(super::keybinds::set_keybinds(bindings)));
                    if let Some(index) = index {
                        self.settings.chip = Some(index);
                    }
                } else {
                    self.settings.capture = Some(Capture::Chord {
                        key,
                        index,
                        conflict: Some(chord.into()),
                    });
                }
            }
        }
        true
    }

    pub(crate) fn start_entry(&mut self, model: &Model, key: &str, typed: Option<char>) {
        let Some(range) = settings::descriptor(key).and_then(|field| Range::of(&field.kind)) else {
            return;
        };
        if self.settings.managed_in_lua(model, key) {
            return;
        }
        self.settings_focus_setting(model, key);
        if typed.is_none()
            && self
                .settings
                .entry
                .as_ref()
                .is_some_and(|entry| entry.key == key)
        {
            return;
        }
        let mut editor = TextEditor::new(range.entry_text(number(model, key)));
        editor.select_all();
        if let Some(typed) = typed {
            editor = TextEditor::new(range.sanitize(&typed.to_string()));
        }
        self.settings.entry = Some(Entry {
            key: key.into(),
            editor,
        });
        self.reset_caret();
        self.frame.dirty = true;
    }

    pub(crate) fn sanitize_entry(&mut self) {
        let Some(entry) = &mut self.settings.entry else {
            return;
        };
        let Some(range) = settings::descriptor(&entry.key).and_then(|field| Range::of(&field.kind))
        else {
            return;
        };
        let clean = range.sanitize(entry.editor.text());
        if clean != entry.editor.text() {
            entry.editor = TextEditor::new(clean);
        }
    }

    pub(crate) fn commit_entry(&mut self, intents: &mut Vec<UiIntent>) {
        let Some(entry) = self.settings.entry.take() else {
            return;
        };
        self.caret.stop();
        self.frame.dirty = true;
        let Some(range) = settings::descriptor(&entry.key).and_then(|field| Range::of(&field.kind))
        else {
            return;
        };
        if let Some(value) = range.parse(entry.editor.text()) {
            intents.push(UiIntent::Domain(Intent::SetSetting {
                key: entry.key,
                value: range.json(value),
            }));
        }
    }

    pub(crate) fn step_setting(
        &mut self,
        model: &Model,
        key: &str,
        steps: i32,
        intents: &mut Vec<UiIntent>,
    ) {
        let Some(range) = settings::descriptor(key).and_then(|field| Range::of(&field.kind)) else {
            return;
        };
        if self.settings.managed_in_lua(model, key) {
            return;
        }
        let current = number(model, key);
        let value = range.step(current, steps);
        if value != current {
            intents.push(UiIntent::Domain(Intent::SetSetting {
                key: key.into(),
                value: range.json(value),
            }));
        }
    }

    pub(crate) fn settings_key(
        &mut self,
        model: &Model,
        key: Key,
        mods: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) {
        if self.settings.entry.is_some() {
            match key {
                Key::Escape => {
                    self.settings.entry = None;
                    self.caret.stop();
                    self.frame.dirty = true;
                    return;
                }
                Key::Enter => {
                    self.commit_entry(intents);
                    return;
                }
                Key::Up | Key::Down | Key::Tab | Key::PageUp | Key::PageDown => {
                    self.commit_entry(intents)
                }
                _ => {
                    self.editor_key(EditorSlot::Entry, &key, mods, intents);
                    return;
                }
            }
        }
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
                .filter(|hit| {
                    self.settings.rect.contains(hit.rect.as_position())
                        && matches!(
                            hit.id,
                            ElementId::SettingsSearch
                                | ElementId::SettingsRecord
                                | ElementId::SettingsCategory(_)
                                | ElementId::Setting(_)
                                | ElementId::ResetSettings
                        )
                })
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
            match self.editor_key(EditorSlot::SettingsSearch, &key, mods, intents) {
                EditResult::Submit => {
                    if let Some(field) = self.settings.fields(model).first() {
                        self.settings_focus_setting(model, field.key);
                    }
                }
                EditResult::Cancel => {
                    self.settings.search_focused = false;
                    self.focused = None;
                    self.caret.stop();
                    self.frame.dirty = true;
                }
                _ => {}
            }
            return;
        }
        if matches!(key, Key::Left | Key::Right)
            && let Some(ElementId::SettingsCategory(category)) = &self.focused
        {
            let at = CATEGORIES
                .iter()
                .position(|(id, ..)| id == category)
                .unwrap_or(0);
            let next = if key == Key::Left {
                (at + CATEGORIES.len() - 1) % CATEGORIES.len()
            } else {
                (at + 1) % CATEGORIES.len()
            };
            self.settings_select_category(CATEGORIES[next].0.into());
            return;
        }
        let fields = self.settings.fields(model);
        let field = fields.get(self.settings.selected).copied();
        let on_row = field.is_some_and(|field| {
            self.focused
                .as_ref()
                .is_none_or(|focused| focused.setting() == Some(field.key))
        });
        let field = field.filter(|_| on_row);
        let chords = field
            .and_then(action)
            .map(|action| keybinds::bindings(&model.settings, action).len());
        let ranged = field.is_some_and(|field| Range::of(&field.kind).is_some());
        match key {
            Key::Escape => {
                self.settings.chip = None;
                self.focused = None;
                self.frame.dirty = true;
            }
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
            Key::Left | Key::Right if ranged => {
                let steps = if mods.shift { 10 } else { 1 };
                let steps = if key == Key::Left { -steps } else { steps };
                self.step_setting(model, field.unwrap().key, steps, intents);
            }
            Key::Left | Key::Right
                if chords.is_some() && !self.settings.managed_in_lua(model, "keybinds") =>
            {
                let last = chords.unwrap();
                self.settings.chip = Some(match (self.settings.chip, key) {
                    (None, Key::Left) => last,
                    (None, _) => 0,
                    (Some(at), Key::Left) => at.saturating_sub(1),
                    (Some(at), _) => (at + 1).min(last),
                });
                self.frame.dirty = true;
            }
            Key::Delete | Key::Backspace => {
                if let Some(field) = field {
                    let chord = action(field)
                        .zip(self.settings.chip)
                        .and_then(|(action, at)| {
                            Some((
                                action,
                                keybinds::bindings(&model.settings, action).get(at)?.clone(),
                            ))
                        });
                    match chord {
                        Some((action, chord)) => {
                            self.remove_keybind(model, action, &chord, intents)
                        }
                        None => self.reset_setting(model, field.key, intents),
                    }
                    self.frame.dirty = true;
                }
            }
            Key::Enter | Key::Character(' ') => {
                if let Some(
                    id @ (ElementId::SettingsCategory(_)
                    | ElementId::ResetSettings
                    | ElementId::SettingsRecord
                    | ElementId::SettingsSearch),
                ) = self.focused.clone()
                {
                    self.activate_element(model, id, intents);
                } else if let Some(field) = field {
                    match chords {
                        Some(count) => {
                            let index = self.settings.chip.filter(|at| *at < count);
                            self.record_chord(model, field.key, index);
                        }
                        None => self.edit_setting(model, field.key, intents),
                    }
                }
            }
            Key::Character(digit) if ranged && digit.is_ascii_digit() && !mods.command() => {
                self.start_entry(model, field.unwrap().key, Some(digit));
            }
            Key::Character(character) if !mods.command() && !mods.alt => {
                self.settings_input_text(&character.to_string())
            }
            _ => {}
        }
    }
}

use crate::SidebarUi;
use crate::components::button::{Button, Tone};
use crate::components::scrollbar::Scrollbar;
use crate::components::text_input::TextInput;
use crate::components::{EDGE_INSET, ICON_CELLS, SURFACE_RADIUS, list};
use crate::element::{ElementId, ScrollArea};
use crate::events::EditorSlot;
use crate::icons;
use crate::input::{EditResult, Key, Modifiers, TextEditor, display_text};
use crate::intent::UiIntent;
use crate::runtime::canvas::{Canvas, RoundedSurface};
use crate::settings::SettingTarget;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use unicode_width::UnicodeWidthStr;
use vtabs_core::{
    Dimension, Intent, Model, SettingDescriptor, SettingKind, Settings, distros, keybinds, padding,
    settings,
};

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

/// Settings from other groups also listed under a category, with the label they take there.
const ALSO_IN: &[(&str, &str, &str)] = &[
    (
        "theme",
        "quick_terminal_border_color",
        "Quick terminal border",
    ),
    (
        "theme",
        "quick_terminal_background_color",
        "Quick terminal background",
    ),
    ("appearance", "selected_background", "Selected background"),
    ("appearance", "hover_background", "Hover background"),
    ("appearance", "tab_foreground", "Text"),
    ("appearance", "selected_foreground", "Selected text"),
    ("appearance", "local_glyph_color", "Local glyph"),
];

const VALUE_CELLS: u16 = 5;

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
    pub rect: Rect,
    pub scrollbar: Option<Scrollbar>,
    pub config_owned: BTreeSet<String>,
    pub recording: bool,
    pub entry: Option<Entry>,
    pub drag: Option<(String, f64)>,
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
            config_owned: BTreeSet::new(),
            recording: false,
            entry: None,
            drag: None,
            anchors: Vec::new(),
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
            SettingKind::Scale => Some(percent(0.5)),
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

pub(crate) fn choice_label(choice: &str) -> String {
    let mut chars = choice.chars().map(|c| if c == '_' { ' ' } else { c });
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
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
    match key {
        "quick_terminal_blur" => model.settings.quick_terminal_opacity >= 1.,
        "show_indexes" => !model.settings.tab_glyphs,
        _ => false,
    }
}

/// The heading above each row that starts a section: groups in All, remote glyphs anywhere.
fn sections(fields: &[&SettingDescriptor], all: bool) -> Vec<Option<&'static str>> {
    let section = |field: &SettingDescriptor| match field.key {
        key if distros::id(key).is_some() => Some("Remote glyphs"),
        _ if all => Some(
            CATEGORIES
                .iter()
                .find(|(id, ..)| *id == field.group)
                .map_or(field.group, |(_, name, _)| name),
        ),
        _ => None,
    };
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = section(field)?;
            (index == 0 || section(fields[index - 1]) != Some(name)).then_some(name)
        })
        .collect()
}

fn heading(sections: &[Option<&str>], index: usize) -> u16 {
    match sections[index] {
        None => 0,
        Some(_) if index == 0 => 1,
        Some(_) => 2,
    }
}

fn visible_end(sections: &[Option<&str>], start: usize, height: u16, line: u16) -> usize {
    let mut remaining = height;
    let mut end = start;
    for index in start..sections.len() {
        let needed = heading(sections, index) + line;
        if needed > remaining {
            break;
        }
        remaining -= needed;
        end += 1;
    }
    end
}

struct Changed {
    current: Value,
    defaults: Value,
}

impl Changed {
    fn new(settings: &Settings) -> Self {
        Self {
            current: serde_json::to_value(settings).unwrap_or_default(),
            defaults: serde_json::to_value(Settings::default()).unwrap_or_default(),
        }
    }
    fn contains(&self, field: &SettingDescriptor) -> bool {
        SettingTarget::of(field.key).changed(&self.current, &self.defaults)
    }
}

impl SettingsPage {
    pub(crate) fn managed_in_lua(&self, model: &Model, key: &str) -> bool {
        let key = SettingTarget::of(key).setting_key();
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
        let borrowed = ALSO_IN
            .iter()
            .filter(|(category, ..)| *category == self.category)
            .filter_map(|(_, key, _)| settings::descriptor(key));
        let mut fields: Vec<_> = settings::descriptors()
            .iter()
            .filter(|field| field.key != "keybinds")
            .flat_map(|field| match field.key {
                "distro_colors" => distros::DESCRIPTORS,
                "quick_terminal_window_padding" => padding::DESCRIPTORS,
                _ => std::slice::from_ref(field),
            })
            .chain(keybinds::DESCRIPTORS.iter())
            .filter(|field| self.category == "all" || field.group == self.category)
            .chain(borrowed)
            .filter(|field| {
                query.is_empty()
                    || [field.key, self.label(field), field.description, field.group]
                        .iter()
                        .any(|text| text.to_lowercase().contains(&query))
            })
            .collect();
        // Remote glyphs close the Colors tab, after the quick terminal colors.
        if self.category == "theme" {
            fields.sort_by_key(|field| distros::id(field.key).is_some());
        }
        fields
    }

    pub(crate) fn label(&self, field: &SettingDescriptor) -> &'static str {
        ALSO_IN
            .iter()
            .find(|(category, key, _)| *category == self.category && *key == field.key)
            .map_or(field.label, |(.., label)| label)
    }

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
        let theme = cx.theme;
        cx.surface(rect, theme.card, SURFACE_RADIUS, EDGE_INSET);
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
        cx.hit(
            ElementId::SettingsSearch,
            Rect::new(rect.x, rect.y, rect.width - button, rect.height),
            "",
        );
        TextInput::new(ElementId::SettingsSearch, &mut self.query, theme.card)
            .active(
                cx.focused == Some(&ElementId::SettingsSearch) && !overlay_open && !self.recording,
            )
            .shift(if rect.height == 2 { 0.5 } else { 0.0 })
            .placeholder(if self.recording {
                "Recording keys"
            } else {
                "Search settings"
            })
            .render(edit, cx);
        if button > 0 {
            Button::action(ElementId::SettingsRecord, icons::KEYBOARD, Tone::Neutral)
                .selected(self.recording)
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

    fn list(&mut self, model: &Model, area: Rect, line: u16, cx: &mut Canvas) {
        let theme = cx.theme;
        let fields = self.fields(model);
        self.selected = self.selected.min(fields.len().saturating_sub(1));
        if fields.is_empty() {
            self.max_scroll = 0;
            if area.height > 0 {
                cx.write(
                    Rect::new(area.x + 2, area.y, area.width.saturating_sub(2), 1),
                    "No results",
                    theme.muted(),
                );
            }
            return;
        }
        let sections = if self.chord.is_none() && line > 1 {
            sections(&fields, self.category == "all")
        } else {
            vec![None; fields.len()]
        };
        let end = |start| visible_end(&sections, start, area.height, line);
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
        let changed = Changed::new(&model.settings);
        let pad = padding(list.width);
        let mut y = list.y;
        for index in self.scroll..end(self.scroll) {
            let field = fields[index];
            let above = heading(&sections, index);
            if let Some(name) = sections[index] {
                cx.write(
                    Rect::new(
                        list.x + pad,
                        y + above - 1,
                        list.width.saturating_sub(pad),
                        1,
                    ),
                    name,
                    theme.muted().add_modifier(Modifier::BOLD),
                );
                y += above;
            }
            let modified = changed.contains(field);
            self.setting(
                model,
                field,
                index,
                Rect::new(list.x, y, list.width, line),
                modified,
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
        modified: bool,
        cx: &mut Canvas,
    ) {
        let theme = cx.theme;
        let id = ElementId::Setting(field.key.into());
        let focused = index == self.selected
            && cx
                .focused
                .is_none_or(|focused| focused.setting() == Some(field.key));
        let hovered = cx.pointer.hovered.as_ref().and_then(ElementId::setting) == Some(field.key);
        let owned = self.managed_in_lua(model, field.key);
        // Idle rows sit on the page like launcher results; the clear surface centers their text.
        let (fill, base) = if focused {
            (theme.selected, theme.selected)
        } else if hovered {
            (theme.card, theme.card)
        } else {
            (Color::Reset, theme.background)
        };
        cx.surface(rect, fill, SURFACE_RADIUS, EDGE_INSET);
        cx.hit(id, rect, "");
        let pad = padding(rect.width);
        let content = Rect::new(
            rect.x + pad,
            rect.y,
            rect.width.saturating_sub(pad * 2),
            rect.height,
        );
        let muted = owned || dimmed(model, field.key);
        let mut used = self.control(model, field, content, rect.right(), base, muted, cx);
        if owned && content.width > used + 4 {
            used += ICON_CELLS;
            cx.write(
                Rect::new(content.right() - used, content.y, 2, 1),
                icons::LOCK,
                theme.base().bg(fill).fg(theme.muted),
            );
        }
        let fg = if muted {
            theme.muted
        } else if modified {
            theme.accent
        } else {
            theme.foreground
        };
        let style = theme.base().bg(fill).fg(fg);
        let glyph = distros::id(field.key).map(|id| {
            let color = theme.host(true, id).unwrap_or(fg);
            Span::styled(format!("{}  ", icons::host(true, id)), style.fg(color))
        });
        cx.write(
            Rect::new(
                content.x,
                content.y,
                content.width.saturating_sub(used + 1),
                1,
            ),
            glyph
                .into_iter()
                .chain([Span::raw(self.label(field))])
                .collect::<Line>(),
            style,
        );
    }

    /// Text ends on the content edge and boxed controls on the card `edge`, so text
    /// inside a box lines up with plain values. Returns the content cells it takes.
    #[allow(clippy::too_many_arguments)]
    fn control(
        &mut self,
        model: &Model,
        field: &SettingDescriptor,
        area: Rect,
        edge: u16,
        base: Color,
        muted: bool,
        cx: &mut Canvas,
    ) -> u16 {
        let theme = cx.theme;
        let fg = if muted { theme.muted } else { theme.foreground };
        if let Some(action) = action(field) {
            let chords = keybinds::bindings(&model.settings, action)
                .iter()
                .map(|chord| (self.chord_label)(chord))
                .collect::<Vec<_>>()
                .join("   ");
            let width = (chords.width() as u16).min(area.width * 2 / 3);
            cx.write(
                Rect::new(area.right() - width, area.y, width, 1),
                Line::from(chords).alignment(Alignment::Right),
                theme.base().bg(base).fg(fg),
            );
            return width;
        }
        if let Some(range) = Range::of(&field.kind) {
            return self.slider(
                field.key,
                range,
                number(model, field.key),
                area,
                edge,
                base,
                muted,
                cx,
            );
        }
        let value = SettingTarget::of(field.key).value(&model.settings);
        let text = match field.kind {
            SettingKind::Bool => {
                return switch(
                    cx,
                    area,
                    edge,
                    base,
                    value.as_bool().unwrap_or(false),
                    muted,
                );
            }
            SettingKind::Choice(_) => format!(
                "{}  {}",
                choice_label(value.as_str().unwrap_or_default()),
                icons::EXPANDED
            ),
            SettingKind::Color | SettingKind::OptionalColor | SettingKind::Border => {
                match value.as_str() {
                    None => "Auto".into(),
                    Some("none") => "None".into(),
                    Some(hex) => format!("   {hex}"),
                }
            }
            SettingKind::Text => value.as_str().map_or_else(|| "Auto".into(), display_text),
            SettingKind::Dimension => serde_json::from_value::<Dimension>(value.clone())
                .map_or_else(|_| "Auto".into(), |dimension| dimension.to_string()),
            SettingKind::List => format!("{} items", value.as_array().map_or(0, |v| v.len())),
            _ => return 0,
        };
        let width = text.width() as u16 + 4;
        if area.width < width + (area.width / 3).min(16) {
            return 0;
        }
        let pill = Rect::new(edge - width, area.y, width, area.height);
        let fill = theme.lift(base, 8);
        cx.fill(flush(pill, fill, 7.0));
        cx.write(
            Rect::new(pill.x + 2, pill.y, width - 2, 1),
            text,
            theme.base().bg(fill).fg(fg),
        );
        if let (
            SettingKind::Color | SettingKind::OptionalColor | SettingKind::Border,
            Some(color),
        ) = (
            &field.kind,
            value.as_str().and_then(crate::Theme::parse_color),
        ) {
            cx.mark(RoundedSurface {
                square: true,
                ..RoundedSurface::new(
                    Rect::new(pill.x + 2, pill.y, 2, pill.height),
                    color,
                    3.0,
                    3.0,
                )
            });
        }
        if matches!(field.kind, SettingKind::Choice(_)) {
            self.anchors.push((field.key.into(), pill));
        }
        area.right().saturating_sub(pill.x)
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(
        &mut self,
        key: &str,
        range: Range,
        value: f64,
        area: Rect,
        edge: u16,
        base: Color,
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
            let x = text.x.saturating_sub(2);
            let field = Rect::new(x, area.y, edge.saturating_sub(x), area.height);
            let surface = theme.lift(base, 10);
            cx.fill(flush(field, surface, 6.0));
            let typed = (entry.editor.text().width() as u16 + 1).clamp(2, text.width);
            TextInput::new(ElementId::SettingsEntry, &mut entry.editor, surface)
                .active(true)
                .shift(if area.height == 2 { 0.5 } else { 0.0 })
                .render(Rect::new(text.right() - typed, text.y, typed, 1), cx);
        } else {
            cx.write(
                text,
                Line::from(range.label(value)).alignment(Alignment::Right),
                theme.base().bg(base).fg(fg),
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
            (theme.lift(base, 10), theme.lift(base, 24), theme.muted)
        } else {
            (theme.lift(base, 14), theme.accent, theme.foreground)
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
}

/// Insets sideways like the row card, so a box ending on the card's cell meets its edge.
fn flush(rect: Rect, fill: Color, radius: f32) -> RoundedSurface {
    RoundedSurface {
        scale_y: if rect.height > 1 { 0.82 } else { 0.95 },
        ..RoundedSurface::new(rect, fill, radius, EDGE_INSET)
    }
}

fn padding(width: u16) -> u16 {
    if width >= 24 {
        2
    } else {
        u16::from(width >= 4)
    }
}

fn switch(cx: &mut Canvas, area: Rect, edge: u16, base: Color, on: bool, muted: bool) -> u16 {
    if area.width < 6 {
        return 0;
    }
    let theme = cx.theme;
    let track = Rect::new(edge - 4, area.y, 4, area.height);
    let (rail, knob) = match (on, muted) {
        (true, false) => (theme.accent, theme.background),
        (true, true) => (theme.lift(base, 30), base),
        (false, _) => (theme.lift(base, 14), theme.muted),
    };
    cx.mark(RoundedSurface {
        scale_y: 1. / f32::from(area.height),
        ..RoundedSurface::new(track, rail, 6.0, EDGE_INSET)
    });
    let x = if on { track.x + 2 } else { track.x };
    cx.mark(RoundedSurface {
        square: true,
        ..RoundedSurface::new(
            Rect::new(x, track.y, 2, track.height),
            knob,
            4.0,
            EDGE_INSET + 2.0,
        )
    });
    area.right().saturating_sub(track.x)
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
        self.focused = Some(ElementId::SettingsCategory(category));
        self.caret.stop();
        self.frame.dirty = true;
    }

    fn settings_input_text(&mut self, text: &str) {
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
            self.settings.selected = index;
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
        self.settings.recording = !self.settings.recording;
        self.focused = Some(ElementId::SettingsRecord);
        self.caret.stop();
        self.frame.dirty = true;
    }

    /// A chord recorded from the search field lists the actions bound to it.
    pub(crate) fn settings_recorded(&mut self, chord: &str) {
        self.frame.dirty = true;
        if chord.is_empty() {
            self.settings.recording = false;
            return;
        }
        self.settings.chord = Some(chord.into());
        self.settings.query = TextEditor::new((self.settings.chord_label)(chord));
        self.settings.selected = 0;
        self.settings.scroll = 0;
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

    /// Steps the value being typed, like a number field; Enter still applies it.
    fn step_entry(&mut self, model: &Model, steps: i32) {
        let Some(entry) = &mut self.settings.entry else {
            return;
        };
        let Some(range) = settings::descriptor(&entry.key).and_then(|field| Range::of(&field.kind))
        else {
            return;
        };
        let current = range
            .parse(entry.editor.text())
            .unwrap_or_else(|| number(model, &entry.key));
        entry.editor = TextEditor::new(range.entry_text(range.step(current, steps)));
        self.reset_caret();
        self.frame.dirty = true;
    }

    /// Switches like the tab row; focus stays in the list when it was there.
    fn settings_switch_category(&mut self, model: &Model, delta: isize) {
        let in_list = !matches!(self.focused, Some(ElementId::SettingsCategory(_)));
        let at = CATEGORIES
            .iter()
            .position(|(id, ..)| *id == self.settings.category)
            .unwrap_or(0) as isize;
        let next = (at + delta).rem_euclid(CATEGORIES.len() as isize) as usize;
        self.settings_select_category(CATEGORIES[next].0.into());
        if in_list && let Some(field) = self.settings.fields(model).first() {
            self.settings_focus_setting(model, field.key);
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
                Key::Up | Key::Down => {
                    let steps = if mods.shift { 10 } else { 1 };
                    self.step_entry(model, if key == Key::Up { steps } else { -steps });
                    return;
                }
                Key::Tab | Key::PageUp | Key::PageDown => self.commit_entry(intents),
                _ => {
                    self.editor_key(EditorSlot::Entry, &key, mods, intents);
                    return;
                }
            }
        }
        if mods.command() && matches!(key, Key::Character('f' | 'F')) {
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
                let search = id == ElementId::SettingsSearch;
                if let ElementId::Setting(key) = &id {
                    self.settings_focus_setting(model, key);
                }
                self.focused = Some(id);
                if search {
                    self.reset_caret();
                } else {
                    self.caret.stop();
                }
                self.frame.dirty = true;
            }
            return;
        }
        let search = self.focused == Some(ElementId::SettingsSearch);
        if search && !matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
            match self.editor_key(EditorSlot::SettingsSearch, &key, mods, intents) {
                EditResult::Submit => {
                    if let Some(field) = self.settings.fields(model).first() {
                        self.settings_focus_setting(model, field.key);
                    }
                }
                EditResult::Cancel => {
                    self.focused = None;
                    self.caret.stop();
                    self.frame.dirty = true;
                }
                _ => {}
            }
            return;
        }
        if matches!(key, Key::Left | Key::Right) && !mods.command() && !mods.alt {
            self.settings_switch_category(model, if key == Key::Left { -1 } else { 1 });
            return;
        }
        let fields = self.settings.fields(model);
        let last = fields.len().saturating_sub(1);
        if let Some(ElementId::SettingsCategory(category)) = &self.focused {
            match key {
                Key::Down | Key::Up if !fields.is_empty() => {
                    let at = if key == Key::Down { 0 } else { last };
                    self.settings_focus_setting(model, fields[at].key);
                }
                Key::Enter | Key::Character(' ') => self.activate_element(
                    model,
                    ElementId::SettingsCategory(category.clone()),
                    intents,
                ),
                Key::Character(character) if !mods.command() && !mods.alt => {
                    self.settings_input_text(&character.to_string())
                }
                _ => {}
            }
            return;
        }
        let field = fields.get(self.settings.selected).copied().filter(|field| {
            self.focused
                .as_ref()
                .is_none_or(|focused| focused.setting() == Some(field.key))
        });
        let ranged = field.is_some_and(|field| Range::of(&field.kind).is_some());
        match key {
            Key::Escape => {
                self.focused = None;
                self.frame.dirty = true;
            }
            Key::Up if search => {}
            Key::Up if field.is_some() && self.settings.selected == 0 => {
                self.settings_focus_setting(model, fields[last].key)
            }
            Key::Down if field.is_some() && self.settings.selected == last => {
                self.settings_focus_setting(model, fields[0].key)
            }
            Key::Down | Key::Up | Key::PageDown | Key::PageUp | Key::Home | Key::End => {
                self.settings.selected = match key {
                    Key::Home => 0,
                    Key::End => last,
                    Key::Down if search => 0,
                    Key::Down => (self.settings.selected + 1).min(last),
                    Key::Up => self.settings.selected.saturating_sub(1),
                    Key::PageDown => (self.settings.selected + 5).min(last),
                    Key::PageUp => self.settings.selected.saturating_sub(5),
                    _ => self.settings.selected,
                };
                if let Some(field) = fields.get(self.settings.selected) {
                    self.settings_focus_setting(model, field.key);
                }
                self.frame.dirty = true;
            }
            Key::Delete | Key::Backspace => {
                if let Some(field) = field {
                    self.reset_setting(model, field.key, intents);
                }
            }
            Key::Enter | Key::Character(' ') => {
                if let Some(
                    id @ (ElementId::ResetSettings
                    | ElementId::SettingsRecord
                    | ElementId::SettingsSearch),
                ) = self.focused.clone()
                {
                    self.activate_element(model, id, intents);
                } else if let Some(field) = field {
                    self.edit_setting(model, field.key, intents);
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

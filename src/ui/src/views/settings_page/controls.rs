use super::SettingsPage;
use super::catalog::{Changed, action, choice_label, heading, sections, visible_end};
use super::range::{Range, number};
use crate::components::scrollbar::Scrollbar;
use crate::components::text_input::TextInput;
use crate::components::{EDGE_INSET, ICON_CELLS, SURFACE_RADIUS, keycaps, list};
use crate::element::{ElementId, ScrollArea};
use crate::icons;
use crate::input::display_text;
use crate::runtime::canvas::{Canvas, RoundedSurface};
use crate::settings::{SettingTarget, managed_in_lua};
use crate::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;
use vtabs_core::{Dimension, Model, SettingDescriptor, SettingKind, distros, keybinds};

const VALUE_CELLS: u16 = 5;

/// The fill beneath the controls and whether they are dimmed.
#[derive(Clone, Copy)]
struct RowCtx {
    base: Color,
    muted: bool,
}

impl RowCtx {
    fn fg(self, theme: &Theme) -> Color {
        if self.muted {
            theme.muted
        } else {
            theme.foreground
        }
    }
}

impl SettingsPage {
    pub(super) fn list(&mut self, model: &Model, area: Rect, line: u16, cx: &mut Canvas) {
        let theme = cx.theme;
        let fields = self.fields(model);
        self.selected = self.selected.min(fields.len().saturating_sub(1));
        if fields.is_empty() {
            self.max_scroll = 0;
            if area.height > 0 {
                let pad = u16::from(area.width > 1);
                cx.write(
                    Rect::new(area.x + pad, area.y, area.width - pad, 1),
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
        let mut y = list.y;
        for index in self.scroll..end(self.scroll) {
            let field = fields[index];
            let above = heading(&sections, index);
            if let Some(name) = sections[index] {
                let pad = u16::from(list.width > 1);
                cx.write(
                    Rect::new(list.x + pad, y + above - 1, list.width - pad, 1),
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
        let owned = managed_in_lua(model, field.key);
        // Idle rows sit on the page like launcher results; the clear surface centers their text.
        let (fill, base) = if focused {
            (theme.selected, theme.selected)
        } else if hovered {
            (theme.card, theme.card)
        } else {
            (Color::Reset, theme.background)
        };
        cx.surface(rect, fill, SURFACE_RADIUS, EDGE_INSET);
        let tooltip = action(field).map_or_else(String::new, |action| {
            keybinds::bindings(&model.settings, action)
                .iter()
                .map(|chord| (self.chord_label)(chord))
                .collect::<Vec<_>>()
                .join("   ")
        });
        cx.hit(id, rect, tooltip);
        let row = RowCtx {
            base,
            muted: owned || dimmed(model, field.key),
        };
        let mut used = self.control(model, field, rect, row, cx);
        if owned && rect.width > used + 4 {
            used += ICON_CELLS;
            cx.write(
                Rect::new(rect.right() - used, rect.y, 2, 1),
                icons::LOCK,
                theme.base().bg(fill).fg(theme.muted),
            );
        }
        let fg = if row.muted {
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
        let pad = u16::from(rect.width > 1);
        cx.write(
            Rect::new(
                rect.x + pad,
                rect.y,
                rect.width.saturating_sub(used + 1 + pad),
                1,
            ),
            glyph
                .into_iter()
                .chain([Span::raw(self.label(field))])
                .collect::<Line>(),
            style,
        );
    }

    /// Controls end on the row's right edge; returns the cells used.
    fn control(
        &mut self,
        model: &Model,
        field: &SettingDescriptor,
        area: Rect,
        row: RowCtx,
        cx: &mut Canvas,
    ) -> u16 {
        let theme = cx.theme;
        let fg = row.fg(theme);
        if let Some(action) = action(field) {
            let chords = keybinds::bindings(&model.settings, action)
                .iter()
                .map(|chord| (self.chord_label)(chord))
                .collect::<Vec<_>>();
            let width = area.width * 2 / 3;
            return keycaps::render(
                &chords,
                Rect::new(area.right() - width, area.y, width, area.height),
                Alignment::Right,
                theme.base().bg(row.base).fg(fg),
                cx,
            );
        }
        if let Some(range) = Range::of(&field.kind) {
            return self.slider(field.key, range, number(model, field.key), area, row, cx);
        }
        let value = SettingTarget::of(field.key).value(&model.settings);
        let text = match field.kind {
            SettingKind::Bool => {
                return switch(cx, area, row, value.as_bool().unwrap_or(false));
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
        let pill = Rect::new(area.right() - width, area.y, width, area.height);
        let fill = theme.lift(row.base, 8);
        cx.fill(flush(pill, fill, 7.0));
        cx.write(
            Rect::new(pill.x + 2, pill.y, width - 2, 1),
            text,
            theme.base().bg(fill).fg(fg),
        );
        if let (
            SettingKind::Color | SettingKind::OptionalColor | SettingKind::Border,
            Some(color),
        ) = (&field.kind, value.as_str().and_then(Theme::parse_color))
        {
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

    fn slider(
        &mut self,
        key: &str,
        range: Range,
        value: f64,
        area: Rect,
        row: RowCtx,
        cx: &mut Canvas,
    ) -> u16 {
        let theme = cx.theme;
        let value = self.editing.slider_value(key).unwrap_or(value);
        let entry = (self.editing.entry_key() == Some(key))
            .then(|| self.editing.editor_mut())
            .flatten();
        let value = entry
            .as_ref()
            .and_then(|editor| range.parse(editor.text()))
            .unwrap_or(value);
        let text = Rect::new(
            area.right().saturating_sub(VALUE_CELLS),
            area.y,
            VALUE_CELLS.min(area.width),
            1,
        );
        if let Some(entry) = entry {
            let x = text.x.saturating_sub(2);
            let field = Rect::new(x, area.y, area.right().saturating_sub(x), area.height);
            let surface = theme.lift(row.base, 10);
            cx.fill(flush(field, surface, 6.0));
            let typed = (entry.text().width() as u16 + 1).clamp(2, text.width);
            TextInput::new(ElementId::SettingsEntry, entry, surface)
                .active(true)
                .shift(if area.height == 2 { 0.5 } else { 0.0 })
                .render(Rect::new(text.right() - typed, text.y, typed, 1), cx);
        } else {
            let pad = u16::from(text.width > 1);
            cx.write(
                Rect::new(text.x, text.y, text.width - pad, 1),
                Line::from(range.label(value)).alignment(Alignment::Right),
                theme.base().bg(row.base).fg(row.fg(theme)),
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
        let (rail, filled, handle) = if row.muted {
            (
                theme.lift(row.base, 10),
                theme.lift(row.base, 24),
                theme.muted,
            )
        } else {
            (theme.lift(row.base, 14), theme.accent, theme.foreground)
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

fn dimmed(model: &Model, key: &str) -> bool {
    match key {
        "quick_terminal_blur" => model.settings.quick_terminal_opacity >= 1.,
        "show_indexes" => !model.settings.tab_glyphs,
        _ => false,
    }
}

/// Insets sideways like the row card, so a box ending on the card's cell meets its edge.
fn flush(rect: Rect, fill: Color, radius: f32) -> RoundedSurface {
    RoundedSurface {
        scale_y: if rect.height > 1 { 0.82 } else { 0.95 },
        ..RoundedSurface::new(rect, fill, radius, EDGE_INSET)
    }
}

fn switch(cx: &mut Canvas, area: Rect, row: RowCtx, on: bool) -> u16 {
    if area.width < 6 {
        return 0;
    }
    let theme = cx.theme;
    let track = Rect::new(area.right() - 4, area.y, 4, area.height);
    let (rail, knob) = match (on, row.muted) {
        (true, false) => (theme.accent, theme.background),
        (true, true) => (theme.lift(row.base, 30), row.base),
        (false, _) => (theme.lift(row.base, 14), theme.muted),
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

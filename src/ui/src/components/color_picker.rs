use crate::components::button::Button;
use crate::components::text_input::TextInput;
use crate::components::{centered, panel};
use crate::element::ElementId;
use crate::overlays::Form;
use crate::overlays::colors::{ColorPicker, PALETTE, hsv, rgb};
use crate::runtime::canvas::{Canvas, RoundedSurface};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;

fn ink(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b)
            if u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 > 145_000 =>
        {
            Color::Rgb(15, 23, 42)
        }
        _ => Color::Rgb(255, 255, 255),
    }
}

fn label(cx: &mut Canvas, rect: Rect, text: Option<&str>) {
    if let Some(text) = text {
        cx.write(rect, text.to_owned(), cx.theme.muted());
    }
}

fn preview(cx: &mut Canvas, rect: Rect, color: Color, text: &str, id: Option<ElementId>) {
    cx.surface(rect, color, 8., 2.);
    let focused = id
        .as_ref()
        .is_some_and(|id| cx.focused(id) || cx.hovered(id));
    let text = if focused {
        format!("↶ {text}")
    } else {
        text.into()
    };
    cx.write(
        Rect::new(rect.x, rect.y + rect.height / 2, rect.width, 1),
        Line::from(text).alignment(Alignment::Center),
        cx.theme
            .base()
            .bg(color)
            .fg(ink(color))
            .add_modifier(Modifier::BOLD),
    );
    if let Some(id) = id {
        cx.hit(id, rect, "");
    }
}

fn palette(cx: &mut Canvas, area: Rect, picker: &ColorPicker) {
    for (index, value) in PALETTE.iter().enumerate() {
        let col = index as u16 % 10;
        let row = index as u16 / 10;
        let x = area.x + col * area.width / 10;
        let y = area.y + row * area.height / 3;
        let rect = Rect::new(
            x,
            y,
            area.x + (col + 1) * area.width / 10 - x,
            area.y + (row + 1) * area.height / 3 - y,
        );
        if rect.is_empty() {
            continue;
        }
        let color = rgb(*value);
        let id = ElementId::ColorSwatch(index);
        let selected = picker.selected == Some(color);
        let focused = cx.focused(&id) || cx.hovered(&id);
        cx.surface(rect, color, 6., if focused { 0.5 } else { 2. });
        if selected || focused {
            cx.write(
                Rect::new(rect.x, rect.y, rect.width, 1),
                Line::from(if selected { "✓" } else { "" }).alignment(Alignment::Center),
                cx.theme.base().bg(color).fg(ink(color)),
            );
        }
        cx.hit(id, rect, "");
    }
}

fn plane(cx: &mut Canvas, rect: Rect, picker: &ColorPicker) {
    for y in 0..rect.height {
        for x in 0..rect.width {
            let color = hsv(
                picker.hue,
                f32::from(x) / f32::from(rect.width.saturating_sub(1).max(1)),
                1. - f32::from(y) / f32::from(rect.height.saturating_sub(1).max(1)),
            );
            cx.surface(Rect::new(rect.x + x, rect.y + y, 1, 1), color, 0., 0.);
            // Subdivide native surfaces to avoid cell-height bands. The buffer
            // retains the cell's color for terminal renderers.
            for slice in 0..4 {
                let offset = (slice as f32 + 0.5) / 4. - 0.5;
                let value = 1.
                    - ((f32::from(y) + offset) / f32::from(rect.height.saturating_sub(1).max(1)))
                        .clamp(0., 1.);
                cx.mark(RoundedSurface {
                    scale_y: 0.25,
                    shift_y: offset,
                    ..RoundedSurface::new(
                        Rect::new(rect.x + x, rect.y + y, 1, 1),
                        hsv(
                            picker.hue,
                            f32::from(x) / f32::from(rect.width.saturating_sub(1).max(1)),
                            value,
                        ),
                        0.,
                        0.,
                    )
                });
            }
        }
    }
    let x = rect.x + (picker.saturation * f32::from(rect.width.saturating_sub(1))).round() as u16;
    let y =
        rect.y + ((1. - picker.value) * f32::from(rect.height.saturating_sub(1))).round() as u16;
    cx.write(
        Rect::new(x, y, 1, 1),
        if cx.focused(&ElementId::ColorPlane) {
            "◆"
        } else {
            "○"
        },
        cx.theme
            .base()
            .bg(picker.preview())
            .fg(ink(picker.preview())),
    );
    cx.hit(ElementId::ColorPlane, rect, "");
}

fn hue(cx: &mut Canvas, rect: Rect, picker: &ColorPicker) {
    for x in 0..rect.width {
        let color = hsv(
            f32::from(x) / f32::from(rect.width.saturating_sub(1).max(1)) * 359.,
            1.,
            1.,
        );
        cx.surface(Rect::new(rect.x + x, rect.y, 1, 1), color, 0., 0.);
    }
    let x = rect.x + (picker.hue / 359. * f32::from(rect.width.saturating_sub(1))).round() as u16;
    let color = hsv(picker.hue, 1., 1.);
    cx.write(
        Rect::new(x.min(rect.right() - 1), rect.y, 1, 1),
        if cx.focused(&ElementId::ColorHue) {
            "◆"
        } else {
            "│"
        },
        cx.theme.base().bg(color).fg(ink(color)),
    );
    cx.hit(ElementId::ColorHue, rect, "");
}

pub(crate) fn render(cx: &mut Canvas, area: Rect, form: &mut Form, editing: bool) -> Rect {
    let panel = panel(cx, centered(area, 72, 30), area);
    let mut inner = panel.inner;
    if inner.width >= 8 {
        inner.x += 1;
        inner.width -= 2;
    }
    if inner.is_empty() {
        return panel.rect;
    }
    let picker = form.color.as_ref().expect("color form");
    let theme = cx.theme;
    cx.write(
        Rect::new(inner.x, inner.y, inner.width, 1),
        form.title.clone(),
        theme.accent().add_modifier(Modifier::BOLD),
    );

    let roomy = inner.height >= 18 && inner.width >= 32;
    let entry_y;
    if roomy {
        let swatch_height = if inner.height >= 24 { 2 } else { 1 };
        let plane_height = inner
            .height
            .saturating_sub(15 + 3 * swatch_height)
            .clamp(2, 8);
        let preview_width = (inner.width / 3).min(20);
        let shades = Rect::new(
            inner.x,
            inner.y + 3,
            inner.width - preview_width - 3,
            plane_height,
        );

        plane(cx, shades, picker);
        let preview_x = shades.right() + 3;
        label(
            cx,
            Rect::new(preview_x, inner.y + 2, preview_width, 1),
            Some("Current    New"),
        );
        let width = preview_width / 2;
        preview(
            cx,
            Rect::new(preview_x, shades.y, width, plane_height),
            picker.original.unwrap_or(picker.fallback),
            if picker.original.is_some() {
                "Aa"
            } else {
                "Auto"
            },
            Some(ElementId::ColorOriginal),
        );
        preview(
            cx,
            Rect::new(
                preview_x + width,
                shades.y,
                preview_width - width,
                plane_height,
            ),
            picker.preview(),
            if picker.selected.is_some() {
                "Aa"
            } else {
                "Auto"
            },
            None,
        );
        let hue_y = shades.bottom() + 1;
        label(cx, Rect::new(inner.x, hue_y, 5, 1), None);
        hue(
            cx,
            Rect::new(inner.x + 5, hue_y, inner.width - 5, 1),
            picker,
        );
        label(cx, Rect::new(inner.x, hue_y + 2, inner.width, 1), None);
        let swatches = Rect::new(inner.x, hue_y + 3, inner.width, swatch_height * 3);
        palette(cx, swatches, picker);
        entry_y = swatches.bottom() + 2;
        label(cx, Rect::new(inner.x, entry_y - 1, inner.width, 1), None);
    } else {
        entry_y = inner.y + u16::from(inner.height > 1);
        if inner.height >= 10 && inner.width >= 10 {
            label(
                cx,
                Rect::new(inner.x, inner.y + 4, inner.width, 1),
                Some("Palette"),
            );
            palette(cx, Rect::new(inner.x, inner.y + 5, inner.width, 3), picker);
        }
    }
    if inner.height > 1 {
        let width = if inner.width >= 18 {
            inner.width - 8
        } else {
            inner.width
        };
        let edit = Rect::new(inner.x, entry_y, width, 1);
        cx.rounded(edit, theme.card);
        TextInput::new(ElementId::Editor, &mut form.editor, theme.card)
            .active(editing)
            .placeholder(if picker.default.is_none() {
                "Automatic"
            } else {
                "#RRGGBB"
            })
            .render(edit, cx);
        cx.hit(ElementId::Editor, edit, "");
        if inner.width >= 18 {
            Button::text(
                ElementId::ColorFormat,
                if picker.rgb_entry {
                    "RGB ▾"
                } else {
                    "HEX ▾"
                },
            )
            .render(Rect::new(edit.right() + 1, entry_y, 7, 1), cx);
        }
    }

    if inner.height >= 5 {
        let height = if roomy { 2 } else { 1 };
        let y = inner.bottom() - height;
        let button_width = (inner.width / 3).min(12);
        Button::text(
            ElementId::ColorReset,
            if picker.default.is_some() {
                "Default"
            } else {
                "Automatic"
            },
        )
        .render(Rect::new(inner.x, y, button_width, height), cx);
        Button::text(ElementId::Cancel, "Cancel").render(
            Rect::new(
                inner.right() - 2 * button_width,
                y,
                button_width.saturating_sub(1),
                height,
            ),
            cx,
        );
        Button::text(ElementId::Submit, "Apply")
            .selected(form.error.is_none())
            .render(
                Rect::new(inner.right() - button_width, y, button_width, height),
                cx,
            );
    }
    panel.rect
}

use crate::SidebarUi;
use crate::components::list;
use crate::element::ElementId;
use crate::input::{Key, Modifiers, TextEditor};
use crate::intent::UiIntent;
use crate::overlays::{FormKind, Overlay};
use crate::theme::Theme;
use ratatui::style::Color;
use vtabs_core::Model;

// Light, vivid and deep shades, ordered by hue with a neutral column.
pub(crate) const PALETTE: [u32; 30] = [
    0xf8fafc, 0xfda4af, 0xfdba74, 0xfde68a, 0xbef264, 0x6ee7b7, 0x67e8f9, 0x93c5fd, 0xc4b5fd,
    0xf0abfc, 0x94a3b8, 0xf43f5e, 0xf97316, 0xeab308, 0x84cc16, 0x10b981, 0x06b6d4, 0x3b82f6,
    0x8b5cf6, 0xd946ef, 0x0f172a, 0x881337, 0x9a3412, 0x854d0e, 0x3f6212, 0x065f46, 0x155e75,
    0x1e3a8a, 0x4c1d95, 0x701a75,
];

pub(crate) fn rgb(value: u32) -> Color {
    Color::Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

pub(crate) fn hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#000000".into(),
    }
}

pub(crate) fn hsv(h: f32, s: f32, v: f32) -> Color {
    let channel = |offset: f32| {
        let k = (offset + h / 60.) % 6.;
        (255. * v * (1. - s * k.min(4. - k).clamp(0., 1.))).round() as u8
    };
    Color::Rgb(channel(5.), channel(3.), channel(1.))
}

#[derive(Clone, Debug)]
pub(crate) struct ColorPicker {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    pub original: Option<Color>,
    pub selected: Option<Color>,
    pub default: Option<Color>,
    pub fallback: Color,
    pub rgb_entry: bool,
    pub swatch: usize,
}

impl ColorPicker {
    fn new(value: &str, default: Option<&str>, fallback: Color) -> Self {
        let original = Theme::parse_color(value);
        let fallback = match fallback {
            Color::Rgb(..) => fallback,
            _ => Color::Rgb(128, 128, 128),
        };
        let mut picker = Self {
            hue: 0.,
            saturation: 0.,
            value: 0.,
            original,
            selected: original,
            default: default.and_then(Theme::parse_color),
            fallback,
            rgb_entry: false,
            swatch: 0,
        };
        picker.select(original);
        picker
    }

    pub(crate) fn preview(&self) -> Color {
        self.selected.unwrap_or(self.fallback)
    }

    fn select(&mut self, color: Option<Color>) {
        self.selected = color;
        let Color::Rgb(r, g, b) = self.preview() else {
            return;
        };
        let [r, g, b] = [r, g, b].map(|channel| f32::from(channel) / 255.);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        // Keep the hue while passing through grey or black.
        if delta > 0. {
            self.hue = (60.
                * if max == r {
                    (g - b) / delta
                } else if max == g {
                    (b - r) / delta + 2.
                } else {
                    (r - g) / delta + 4.
                })
            .rem_euclid(360.);
        }
        self.saturation = if max > 0. { delta / max } else { 0. };
        self.value = max;
        if let Some(index) = PALETTE.iter().position(|c| Some(rgb(*c)) == color) {
            self.swatch = index;
        }
    }

    pub(crate) fn parse(&self, text: &str) -> Result<Option<Color>, &'static str> {
        let text = text.trim();
        if text.is_empty() && self.default.is_none() {
            return Ok(None);
        }
        if self.rgb_entry {
            let channels: Vec<_> = text.split(',').map(|c| c.trim().parse::<u8>()).collect();
            return match channels.as_slice() {
                [Ok(r), Ok(g), Ok(b)] => Ok(Some(Color::Rgb(*r, *g, *b))),
                _ => Err("Use R, G, B with each value from 0 to 255"),
            };
        }
        let digits = text.strip_prefix('#').unwrap_or(text);
        if !matches!(digits.len(), 3 | 6) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Use a 3 or 6 digit hex color, e.g. #8eb4ed");
        }
        let expanded = if digits.len() == 3 {
            digits.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            digits.to_owned()
        };
        Ok(Theme::parse_color(&format!("#{expanded}")))
    }

    pub(crate) fn edited(&mut self, text: &str) -> Result<(), &'static str> {
        self.select(self.parse(text)?);
        Ok(())
    }

    fn text(&self) -> String {
        match self.selected {
            Some(Color::Rgb(r, g, b)) if self.rgb_entry => format!("{r}, {g}, {b}"),
            Some(color) => hex(color),
            None => String::new(),
        }
    }
}

impl SidebarUi {
    pub(crate) fn open_color_form(
        &mut self,
        title: &str,
        kind: FormKind,
        value: &str,
        default: Option<&str>,
        fallback: Color,
    ) {
        self.open_form(title, kind, value);
        if let Some(Overlay::Form(form)) = &mut self.overlays.current {
            form.color = Some(Box::new(ColorPicker::new(value, default, fallback)));
        }
    }

    fn change_color(&mut self, change: impl FnOnce(&mut ColorPicker)) {
        let Some(Overlay::Form(form)) = &mut self.overlays.current else {
            return;
        };
        let Some(color) = &mut form.color else {
            return;
        };
        change(color);
        form.editor = TextEditor::new(color.text());
        form.editor.select_all();
        form.error = None;
        self.frame.dirty = true;
    }

    pub(crate) fn color_action(&mut self, id: ElementId) {
        self.change_color(|color| match id {
            ElementId::ColorSwatch(index) => {
                if let Some(value) = PALETTE.get(index) {
                    color.select(Some(rgb(*value)));
                    color.swatch = index;
                }
            }
            ElementId::ColorFormat => color.rgb_entry = !color.rgb_entry,
            ElementId::ColorReset => color.select(color.default),
            ElementId::ColorOriginal => color.select(color.original),
            _ => {}
        });
        if id == ElementId::ColorFormat {
            self.focused = Some(ElementId::Editor);
            self.reset_caret();
        }
    }

    pub(crate) fn drag_color(&mut self, id: &ElementId, x: u16, y: u16) {
        let Some(rect) = self.paint.hit(id).map(|hit| hit.rect) else {
            return;
        };
        let at = |position: u16, start: u16, length: u16, fraction: f32| {
            ((f32::from(position) - f32::from(start) + fraction - 0.5)
                / f32::from(length.saturating_sub(1).max(1)))
            .clamp(0., 1.)
        };
        let (fx, fy) = self.pointer.fraction;
        self.change_color(|color| {
            if *id == ElementId::ColorHue {
                color.hue = at(x, rect.x, rect.width, fx) * 359.;
            } else {
                color.saturation = at(x, rect.x, rect.width, fx);
                color.value = 1. - at(y, rect.y, rect.height, fy);
            }
            color.selected = Some(hsv(color.hue, color.saturation, color.value));
        });
    }

    pub(crate) fn color_key(
        &mut self,
        model: &Model,
        key: &Key,
        modifiers: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) -> bool {
        let Some(Overlay::Form(form)) = &self.overlays.current else {
            return false;
        };
        let Some(color) = &form.color else {
            return false;
        };
        if *key == Key::Tab {
            let swatch = ElementId::ColorSwatch(color.swatch);
            let mut order: Vec<_> = [
                ElementId::Editor,
                ElementId::ColorFormat,
                ElementId::ColorPlane,
                ElementId::ColorHue,
                swatch,
                ElementId::ColorOriginal,
                ElementId::ColorReset,
                ElementId::Submit,
                ElementId::Cancel,
            ]
            .into_iter()
            .filter(|id| self.paint.hit(id).is_some())
            .collect();
            if order.is_empty() {
                order.push(ElementId::Editor);
            }
            self.focused = list::cycle(&order, self.focused.as_ref(), modifiers.shift);
            self.reset_caret();
            self.frame.dirty = true;
            return true;
        }
        if self.focused == Some(ElementId::Editor) {
            return false;
        }
        match key {
            Key::Escape => {
                self.back();
                return true;
            }
            Key::Enter => {
                if self.focused == Some(ElementId::Cancel) {
                    self.back();
                } else {
                    self.submit_form(model, intents);
                }
                return true;
            }
            _ => {}
        }
        let Some(id) = self.focused.clone() else {
            return true;
        };
        match key {
            Key::Character(' ') => self.activate_element(model, id, intents),
            Key::Left | Key::Right | Key::Up | Key::Down => {
                let direction = if matches!(key, Key::Left | Key::Down) {
                    -1.
                } else {
                    1.
                };
                let step = if modifiers.shift { 0.1 } else { 0.01 };
                match id {
                    ElementId::ColorPlane | ElementId::ColorHue => self.change_color(|color| {
                        if id == ElementId::ColorHue {
                            color.hue = (color.hue + direction * step * 100.).rem_euclid(360.);
                        } else if matches!(key, Key::Left | Key::Right) {
                            color.saturation = (color.saturation + direction * step).clamp(0., 1.);
                        } else {
                            color.value = (color.value + direction * step).clamp(0., 1.);
                        }
                        color.selected = Some(hsv(color.hue, color.saturation, color.value));
                    }),
                    ElementId::ColorSwatch(index) => {
                        let delta = match key {
                            Key::Left => -1,
                            Key::Right => 1,
                            Key::Up => -10,
                            _ => 10,
                        };
                        let index =
                            (index as i32 + delta).rem_euclid(PALETTE.len() as i32) as usize;
                        let id = ElementId::ColorSwatch(index);
                        self.color_action(id.clone());
                        self.focused = Some(id);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        true
    }
}

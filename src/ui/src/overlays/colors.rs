use crate::SidebarUi;
use crate::components::list;
use crate::element::ElementId;
use crate::input::{Key, Modifiers, TextEditor};
use crate::intent::UiIntent;
use crate::overlays::{FormKind, Overlay};
use crate::theme::Theme;
use ratatui::style::Color;
use tachyonfx::{color_from_hsv, color_to_hsv};
use vtabs_core::Model;

// Light, vivid and deep shades, ordered by hue with a neutral column.
pub(crate) const PALETTE: [u32; 30] = [
    0xf8fafc, 0xfda4af, 0xfdba74, 0xfde68a, 0xbef264, 0x6ee7b7, 0x67e8f9, 0x93c5fd, 0xc4b5fd,
    0xf0abfc, 0x94a3b8, 0xf43f5e, 0xf97316, 0xeab308, 0x84cc16, 0x10b981, 0x06b6d4, 0x3b82f6,
    0x8b5cf6, 0xd946ef, 0x0f172a, 0x881337, 0x9a3412, 0x854d0e, 0x3f6212, 0x065f46, 0x155e75,
    0x1e3a8a, 0x4c1d95, 0x701a75,
];

pub(crate) fn hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#000000".into(),
    }
}

/// Hue in degrees; saturation and value in percent, as tachyonfx expects.
#[derive(Clone, Debug)]
pub(crate) struct ColorPicker {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    pub original: Pick,
    pub selected: Pick,
    pub default: Option<Color>,
    pub fallback: Color,
    pub rgb_entry: bool,
    pub swatch: usize,
    pub allows_none: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pick {
    Auto,
    NoColor,
    Color(Color),
}

impl Pick {
    fn of(color: Option<Color>) -> Self {
        color.map_or(Self::Auto, Self::Color)
    }

    pub(crate) fn color(self) -> Option<Color> {
        match self {
            Self::Color(color) => Some(color),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::NoColor => "None",
            Self::Color(_) => "Aa",
        }
    }
}

impl ColorPicker {
    fn new(value: &str, default: Option<&str>, fallback: Color) -> Self {
        let original = Pick::of(Theme::parse_color(value));
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
            allows_none: false,
        };
        picker.select(original);
        picker
    }

    pub(crate) fn preview(&self) -> Color {
        self.selected.color().unwrap_or(self.fallback)
    }

    fn select(&mut self, pick: Pick) {
        self.selected = pick;
        let (hue, saturation, value) = color_to_hsv(&self.preview());
        // Keep the hue while passing through grey or black.
        if saturation > 0. {
            self.hue = hue;
        }
        self.saturation = saturation;
        self.value = value;
        if let Some(index) = PALETTE
            .iter()
            .position(|c| Pick::Color(Color::from_u32(*c)) == pick)
        {
            self.swatch = index;
        }
    }

    fn select_hsv(&mut self) {
        self.selected = Pick::Color(color_from_hsv(self.hue, self.saturation, self.value));
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
        if self.allows_none && text.trim().eq_ignore_ascii_case("none") {
            self.select(Pick::NoColor);
            return Ok(());
        }
        self.select(Pick::of(self.parse(text)?));
        Ok(())
    }

    fn text(&self) -> String {
        match self.selected {
            Pick::Color(Color::Rgb(r, g, b)) if self.rgb_entry => format!("{r}, {g}, {b}"),
            Pick::Color(color) => hex(color),
            Pick::NoColor => "none".into(),
            Pick::Auto => String::new(),
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
        if let Some(Overlay::Form(form)) = self.overlays.current_mut() {
            form.color = Some(Box::new(ColorPicker::new(value, default, fallback)));
        }
    }

    pub(crate) fn allow_no_color(&mut self) {
        if let Some(Overlay::Form(form)) = self.overlays.current_mut()
            && let Some(color) = &mut form.color
        {
            color.allows_none = true;
            if form.editor.text() == "none" {
                color.original = Pick::NoColor;
                color.selected = Pick::NoColor;
            }
        }
    }

    fn change_color(&mut self, change: impl FnOnce(&mut ColorPicker)) {
        let Some(Overlay::Form(form)) = self.overlays.current_mut() else {
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
                    color.select(Pick::Color(Color::from_u32(*value)));
                    color.swatch = index;
                }
            }
            ElementId::ColorFormat => color.rgb_entry = !color.rgb_entry,
            ElementId::ColorReset => color.select(Pick::of(color.default)),
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
                color.saturation = at(x, rect.x, rect.width, fx) * 100.;
                color.value = (1. - at(y, rect.y, rect.height, fy)) * 100.;
            }
            color.select_hsv();
        });
    }

    pub(crate) fn color_key(
        &mut self,
        model: &Model,
        key: &Key,
        modifiers: Modifiers,
        intents: &mut Vec<UiIntent>,
    ) -> bool {
        let Some(Overlay::Form(form)) = self.overlays.current() else {
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
                let step = direction * if modifiers.shift { 10. } else { 1. };
                match id {
                    ElementId::ColorPlane | ElementId::ColorHue => self.change_color(|color| {
                        if id == ElementId::ColorHue {
                            color.hue = (color.hue + step).rem_euclid(360.);
                        } else if matches!(key, Key::Left | Key::Right) {
                            color.saturation = (color.saturation + step).clamp(0., 100.);
                        } else {
                            color.value = (color.value + step).clamp(0., 100.);
                        }
                        color.select_hsv();
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

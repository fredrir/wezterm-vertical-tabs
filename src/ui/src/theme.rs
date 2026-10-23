use crate::icons;
use ratatui::style::{Color, Style};
use std::collections::BTreeMap;
use vtabs_core::{Model, Settings, settings};

/// WCAG contrast for text, the least a derived text color keeps against its fill.
const LEGIBLE: f32 = 4.5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub card: Color,
    pub hover: Color,
    pub selected: Color,
    pub accent: Color,
    pub danger: Color,
    pub machines: BTreeMap<String, Color>,
    pub hover_background: Option<Color>,
    pub tab_foreground: Option<Color>,
    pub selected_foreground: Option<Color>,
    pub local_glyph: Option<Color>,
}

/// Copy mode search: the widget and the match highlights, with text for each highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Search {
    pub background: Color,
    pub border: Option<Color>,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub error: Color,
    pub active_match: (Color, Color),
    pub other_match: (Color, Color),
}

impl Default for Theme {
    fn default() -> Self {
        let mut theme = Self {
            background: Color::Rgb(6, 1, 81),
            foreground: Color::Rgb(225, 231, 240),
            muted: Color::Rgb(152, 168, 190),
            card: Color::Reset,
            hover: Color::Reset,
            selected: Color::Rgb(52, 72, 95),
            accent: Color::Rgb(169, 199, 245),
            danger: Color::Rgb(250, 122, 134),
            machines: BTreeMap::new(),
            hover_background: None,
            tab_foreground: None,
            selected_foreground: None,
            local_glyph: None,
        };
        theme.sync_surfaces();
        theme
    }
}

impl Theme {
    pub(crate) fn apply(&mut self, model: &Model) {
        let settings = &model.settings;
        self.background = Self::parse_color(&settings.background).unwrap_or(self.background);
        self.foreground = Self::parse_color(&settings.foreground).unwrap_or(self.foreground);
        self.muted = Self::parse_color(&settings.muted).unwrap_or(self.muted);
        self.selected = Self::parse_color(&settings.selected_background).unwrap_or(self.selected);
        self.machines = settings
            .distro_colors
            .iter()
            .filter_map(|(os, color)| Some((os.clone(), Self::parse_color(color)?)))
            .collect();
        let optional = |value: &Option<String>| value.as_deref().and_then(Self::parse_color);
        self.hover_background = optional(&settings.hover_background);
        self.tab_foreground = optional(&settings.tab_foreground);
        self.selected_foreground = optional(&settings.selected_foreground);
        self.local_glyph = optional(&settings.local_glyph_color);
        self.accent = Self::parse_color(&settings.accent).unwrap_or(self.accent);
        self.sync_surfaces();
    }

    /// The same palette over another background, such as the terminal's behind a page.
    pub(crate) fn on(&self, background: Color) -> Self {
        let mut theme = self.clone();
        theme.background = background;
        theme.sync_surfaces();
        theme
    }

    fn sync_surfaces(&mut self) {
        if !matches!(
            (self.background, self.foreground),
            (Color::Rgb(..), Color::Rgb(..))
        ) {
            return;
        }
        self.hover = self.lift(self.background, 4);
        self.card = self.lift(self.background, 5);
    }

    pub(crate) fn lift(&self, fill: Color, amount: u16) -> Color {
        Self::blend(fill, self.foreground, amount)
    }

    pub(crate) fn tint(&self, fill: Color, amount: u16) -> Color {
        Self::blend(fill, self.accent, amount)
    }

    pub(crate) fn warn(&self, fill: Color, amount: u16) -> Color {
        Self::blend(fill, self.danger, amount)
    }

    fn blend(fill: Color, toward: Color, amount: u16) -> Color {
        let (Color::Rgb(r, g, b), Color::Rgb(fr, fg, fb)) = (fill, toward) else {
            return fill;
        };
        let channel = |base: u8, foreground: u8| {
            ((u16::from(base) * (100 - amount) + u16::from(foreground) * amount + 50) / 100) as u8
        };
        Color::Rgb(channel(r, fr), channel(g, fg), channel(b, fb))
    }

    pub(crate) fn host(&self, remote: bool, os: &str) -> Option<Color> {
        if !remote {
            return self.local_glyph;
        }
        icons::host_color(remote, os, &self.machines)
    }

    pub(crate) fn hovered(&self) -> Color {
        self.hover_background.unwrap_or(self.card)
    }

    pub(crate) fn tab_text(&self) -> Color {
        self.tab_foreground.unwrap_or(self.foreground)
    }

    pub(crate) fn selected_text(&self) -> Color {
        self.selected_foreground.unwrap_or(self.accent)
    }

    /// Unset search colors follow this palette.
    pub fn search(&self, settings: &Settings) -> Search {
        let color = |value: &Option<String>| value.as_deref().and_then(Self::parse_color);
        let background = color(&settings.search_background_color).unwrap_or(self.card);
        let border = match settings.search_border_color.as_deref() {
            Some("none") => None,
            value => Some(
                value
                    .and_then(Self::parse_color)
                    .unwrap_or_else(|| self.lift(background, 14)),
            ),
        };
        let active = color(&settings.search_active_match_color).unwrap_or(self.accent);
        let other = color(&settings.search_match_color).unwrap_or(self.selected);
        Search {
            background,
            border,
            text: color(&settings.search_text_color).unwrap_or(self.foreground),
            muted: color(&settings.search_muted_color).unwrap_or(self.muted),
            accent: self.accent,
            error: color(&settings.search_error_color)
                .unwrap_or_else(|| self.legible(self.danger, background)),
            active_match: (active, self.text_on(active)),
            other_match: (other, self.text_on(other)),
        }
    }

    /// `color` blended toward the foreground until it reads as text on `fill`.
    fn legible(&self, color: Color, fill: Color) -> Color {
        (0..=12)
            .map(|step| Self::blend(color, self.foreground, step * 5))
            .find(|candidate| contrast(*candidate, fill) >= LEGIBLE)
            .unwrap_or(self.foreground)
    }

    /// Whichever of the foreground and background reads better on `fill`.
    fn text_on(&self, fill: Color) -> Color {
        if contrast(self.foreground, fill) >= contrast(self.background, fill) {
            self.foreground
        } else {
            self.background
        }
    }

    pub fn base(&self) -> Style {
        Style::default().bg(self.background).fg(self.foreground)
    }
    pub fn muted(&self) -> Style {
        self.base().fg(self.muted)
    }
    pub(crate) fn secondary_on(&self, fill: Color) -> Style {
        self.base().bg(fill).fg(if fill == self.selected {
            self.foreground
        } else {
            self.muted
        })
    }
    pub fn accent(&self) -> Style {
        self.base().fg(self.accent)
    }
    pub fn parse_color(value: &str) -> Option<Color> {
        settings::valid_color(value)
            .then(|| value.parse().ok())
            .flatten()
    }
}

fn luminance(color: Color) -> f32 {
    let Color::Rgb(r, g, b) = color else {
        return 0.;
    };
    let linear = |channel: u8| {
        let value = f32::from(channel) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

use crate::icons;
use ratatui::style::{Color, Style};
use std::collections::BTreeMap;
use vtabs_core::{Model, settings};

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

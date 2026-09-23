use crate::icons;
use ratatui::style::{Color, Style};
use std::collections::BTreeMap;

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
    pub private: Color,
    pub machines: BTreeMap<String, Color>,
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
            private: Color::Rgb(207, 166, 255),
            machines: BTreeMap::new(),
        };
        theme.sync_surfaces();
        theme
    }
}

impl Theme {
    pub(crate) fn sync_surfaces(&mut self) {
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
        icons::host_color(remote, os, &self.machines)
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
        let hex = value.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let rgb = u32::from_str_radix(hex, 16).ok()?;
        Some(Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
    }
}

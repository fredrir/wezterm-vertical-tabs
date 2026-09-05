use ratatui::style::{Color, Style};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub card: Color,
    pub selected: Color,
    pub accent: Color,
    pub danger: Color,
    pub private: Color,
    pub border: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color::Rgb(24, 26, 34),
            foreground: Color::Rgb(221, 226, 240),
            muted: Color::Rgb(133, 144, 165),
            card: Color::Rgb(32, 36, 47),
            selected: Color::Rgb(53, 64, 88),
            accent: Color::Rgb(158, 190, 255),
            danger: Color::Rgb(250, 122, 134),
            private: Color::Rgb(207, 166, 255),
            border: Color::Rgb(65, 74, 94),
        }
    }
}

impl Theme {
    pub fn base(&self) -> Style {
        Style::default().bg(self.background).fg(self.foreground)
    }
    pub fn muted(&self) -> Style {
        self.base().fg(self.muted)
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

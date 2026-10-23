//! Copy mode search colors from the sidebar theme.
use super::*;
use ratatui::style::Color;
use window::color::SrgbaTuple;

pub(super) fn palette(theme: &ui::Theme, settings: &core::Settings) -> SearchPalette {
    let search = theme.search(settings);
    let pair = |(background, text)| (srgba(background), srgba(text));
    SearchPalette {
        background: srgba(search.background),
        border: search.border.map(srgba),
        text: srgba(search.text),
        muted: srgba(search.muted),
        accent: srgba(search.accent),
        error: srgba(search.error),
        active_match: pair(search.active_match),
        other_match: pair(search.other_match),
    }
}

fn srgba(color: Color) -> SrgbaTuple {
    match color {
        Color::Rgb(r, g, b) => SrgbaTuple::from((r, g, b)),
        _ => SrgbaTuple(0., 0., 0., 0.),
    }
}

#[cfg(test)]
#[path = "../tests/search.rs"]
mod tests;

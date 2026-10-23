//! What copy mode's floating search widget shows, and the match colors it shares.
use super::CopyOverlay;
use config::ColorSpec;
use mux::pane::PatternType;
use mux::renderable::StableCursorPosition;
use termwiz::cell::CellAttributes;
use termwiz::color::{AnsiColor, ColorAttribute};
use wezterm_term::StableRowIndex;

/// Background and foreground of the active match, then of the others.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Highlights {
    pub active: (ColorAttribute, ColorAttribute),
    pub other: (ColorAttribute, ColorAttribute),
}

impl Highlights {
    /// WezTerm's `copy_mode_*_highlight_*` colors.
    fn configured() -> Self {
        let config = config::configuration();
        let colors = &config.resolved_palette;
        let color = |spec: Option<ColorSpec>, fallback: AnsiColor| -> ColorAttribute {
            spec.unwrap_or(fallback.into()).into()
        };
        Self {
            active: (
                color(colors.copy_mode_active_highlight_bg, AnsiColor::Yellow),
                color(colors.copy_mode_active_highlight_fg, AnsiColor::Black),
            ),
            other: (
                color(colors.copy_mode_inactive_highlight_bg, AnsiColor::Fuchsia),
                color(colors.copy_mode_inactive_highlight_fg, AnsiColor::Black),
            ),
        }
    }

    pub(super) fn apply(&self, attrs: &mut CellAttributes, active: bool) {
        let (background, foreground) = if active { self.active } else { self.other };
        attrs
            .set_background(background)
            .set_foreground(foreground)
            .set_reverse(false);
    }
}

/// Widget state that outlives a frame.
#[derive(Default)]
pub(super) struct Presentation {
    highlights: Option<Highlights>,
    scroll: usize,
}

impl Presentation {
    pub(super) fn highlights(&self) -> Highlights {
        self.highlights.unwrap_or_else(Highlights::configured)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchView {
    pub query: String,
    /// Byte offset of the text cursor in `query`.
    pub cursor: usize,
    pub editing: bool,
    pub pattern: PatternType,
    /// The active match, counted from the newest.
    pub active: Option<usize>,
    pub matches: usize,
    /// Rows still to search while the search runs.
    pub searching: Option<StableRowIndex>,
    /// The first query column the widget showed last.
    pub scroll: usize,
}

impl CopyOverlay {
    /// Shown while the pattern is edited or not empty, as the bar it replaces was.
    pub fn search_view(&self) -> Option<SearchView> {
        let render = self.render.lock();
        let query = render.search_line.get_line();
        if !render.editing_search && query.is_empty() {
            return None;
        }
        Some(SearchView {
            query: query.to_string(),
            cursor: render.search_line.get_cursor(),
            editing: render.editing_search,
            pattern: render.pattern_type.clone(),
            active: render.result_pos,
            matches: render.results.len(),
            searching: render.searching.as_ref().map(|searching| searching.remain),
            scroll: render.search.scroll,
        })
    }

    pub fn scroll_search(&self, scroll: usize) {
        self.render.lock().search.scroll = scroll;
    }

    /// Returns whether they changed, which repaints the matches.
    pub fn set_highlights(&self, highlights: Highlights) -> bool {
        let mut render = self.render.lock();
        let changed = render.search.highlights != Some(highlights);
        render.search.highlights = Some(highlights);
        changed
    }

    /// Copy mode's own cursor, also while the pattern is edited.
    pub fn cursor(&self) -> StableCursorPosition {
        self.render.lock().cursor
    }
}

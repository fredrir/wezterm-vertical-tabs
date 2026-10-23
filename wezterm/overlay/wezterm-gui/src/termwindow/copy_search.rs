//! Copy mode's search as a floating widget over the pane that owns the overlay.
use super::ui_host::{Bounds, SearchPalette};
use super::TermWindow;
use crate::overlay::copy::{Highlights, SearchView};
use crate::overlay::CopyOverlay;
use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator};
use crate::termwindow::render::RenderScreenLineParams;
use luahelper::impl_lua_conversion_dynamic;
use mux::pane::{PaneId, PatternType};
use mux::renderable::RenderableDimensions;
use mux::tab::PositionedPane;
use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;
use termwiz::surface::SEQ_ZERO;
use unicode_segmentation::UnicodeSegmentation;
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_term::{unicode_column_width, Line, StableRowIndex};
use window::color::{LinearRgba, SrgbaTuple};
use window::WindowOps;

/// Above the panes, below the sidebar UI and its popovers.
const ZINDEX: i8 = 1;
/// `/` and a space.
const PROMPT: usize = 2;
const MIN_FIELD: usize = 12;
const MAX_FIELD: usize = 40;
/// A narrow pane shrinks the field this far before dropping the count.
const SHRUNK_FIELD: usize = 4;
/// Fits `No results` and `999 of 999`; larger counts widen the widget.
const COUNT: usize = 10;
const GAP: usize = 2;
/// Logical pixels at 96 dpi, as the sidebar's rounded surfaces.
const INSET: f32 = 8.;
const RADIUS: f32 = 9.;

#[derive(FromDynamic, ToDynamic)]
pub(crate) struct CopyCursor {
    x: usize,
    y: StableRowIndex,
}
impl_lua_conversion_dynamic!(CopyCursor);

/// The widget in whole pixels, at the top right of a pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    pub(super) panel: Bounds,
    /// Top left of the first text cell.
    pub(super) text: (f32, f32),
    /// Query columns after the prompt.
    pub(super) field: usize,
    /// Columns after the field for the mode and count; none in a narrow pane.
    pub(super) trail: usize,
}

impl Layout {
    pub(super) fn new(
        pane: Bounds,
        cell: (f32, f32),
        dpi: f32,
        query: usize,
        trail: usize,
    ) -> Option<Self> {
        let scale = (dpi / 96.).max(1.);
        let inset = (INSET * scale).round();
        let padding = (cell.0.round(), (cell.1 * 0.35).round());
        let fits = ((pane.width - 2. * (inset + padding.0)) / cell.0)
            .floor()
            .max(0.) as usize;
        let wanted = (query + 1).clamp(MIN_FIELD, MAX_FIELD);
        let (field, trail) = if PROMPT + wanted + trail <= fits {
            (wanted, trail)
        } else if fits >= PROMPT + SHRUNK_FIELD + trail {
            (fits - PROMPT - trail, trail)
        } else {
            (wanted.min(fits.saturating_sub(PROMPT)), 0)
        };
        let height = cell.1 + 2. * padding.1;
        if field == 0 || pane.height < height + inset {
            return None;
        }
        let width = (PROMPT + field + trail) as f32 * cell.0 + 2. * padding.0;
        let x = (pane.x + pane.width - inset - width).round();
        let y = (pane.y + inset).round();
        Some(Self {
            panel: Bounds {
                x,
                y,
                width,
                height,
            },
            text: (x + padding.0, y + padding.1),
            field,
            trail,
        })
    }

    pub(super) fn columns(&self) -> usize {
        PROMPT + self.field + self.trail
    }
}

/// The first query column shown, moved only as far as keeps the caret in `field`.
pub(super) fn scroll(previous: usize, caret: usize, width: usize, field: usize) -> usize {
    let scroll = previous.min((width + 1).saturating_sub(field));
    if caret < scroll {
        caret
    } else if caret + 1 > scroll + field {
        caret + 1 - field
    } else {
        scroll
    }
}

/// Shown only for the types smart case is not.
pub(super) fn mode(pattern: &PatternType) -> Option<&'static str> {
    match pattern {
        PatternType::CaseSmartString => None,
        PatternType::CaseSensitiveString => Some("Aa"),
        PatternType::CaseInSensitiveString => Some("aa"),
        PatternType::Regex => Some(".*"),
    }
}

/// Nothing until the query has text and the first matches, or the search's end, arrive.
pub(super) fn count(view: &SearchView) -> Option<String> {
    match (view.matches, view.searching.is_some()) {
        _ if view.query.is_empty() => None,
        (0, true) => None,
        (0, false) => Some("No results".into()),
        (matches, _) => {
            let at = view.active.map_or(0, |at| at + 1);
            Some(format!("{at} of {matches}"))
        }
    }
}

pub(super) fn no_results(view: &SearchView) -> bool {
    !view.query.is_empty() && view.matches == 0 && view.searching.is_none()
}

fn trail(view: &SearchView) -> usize {
    let count = count(view).map_or(0, |count| count.len());
    GAP + mode(&view.pattern).map_or(0, |mode| mode.len() + GAP) + count.max(COUNT)
}

fn colored(color: SrgbaTuple) -> CellAttributes {
    let mut attrs = CellAttributes::default();
    attrs.set_foreground(ColorAttribute::TrueColorWithDefaultFallback(color));
    attrs
}

fn highlights(palette: &SearchPalette) -> Highlights {
    let pair = |(background, text)| {
        (
            ColorAttribute::TrueColorWithDefaultFallback(background),
            ColorAttribute::TrueColorWithDefaultFallback(text),
        )
    };
    Highlights {
        active: pair(palette.active_match),
        other: pair(palette.other_match),
    }
}

/// The prompt, the visible query from `scroll`, and the mode and count against the right edge.
pub(super) fn line(
    view: &SearchView,
    layout: &Layout,
    scroll: usize,
    palette: &SearchPalette,
) -> Line {
    let columns = layout.columns();
    let mut line = Line::with_width(columns, SEQ_ZERO);
    line.overlay_text_with_attribute(0, "/", colored(palette.muted), SEQ_ZERO);
    let mut column = 0;
    for grapheme in view.query.graphemes(true) {
        let width = unicode_column_width(grapheme, None);
        if width > 0 && column >= scroll && column + width <= scroll + layout.field {
            line.overlay_text_with_attribute(
                PROMPT + column - scroll,
                grapheme,
                colored(palette.text),
                SEQ_ZERO,
            );
        }
        column += width;
    }
    if layout.trail == 0 {
        return line;
    }
    let mut right = columns;
    if let Some(count) = count(view) {
        let color = if no_results(view) {
            palette.error
        } else {
            palette.muted
        };
        right -= count.len();
        line.overlay_text_with_attribute(right, &count, colored(color), SEQ_ZERO);
        right -= GAP;
    }
    if let Some(mode) = mode(&view.pattern) {
        line.overlay_text_with_attribute(
            right - mode.len(),
            mode,
            colored(palette.accent),
            SEQ_ZERO,
        );
    }
    line
}

impl TermWindow {
    pub(crate) fn copy_cursor(&self, pane_id: PaneId) -> Option<CopyCursor> {
        let state = self.pane_state(pane_id);
        let copy = state.overlay.as_ref()?.pane.downcast_ref::<CopyOverlay>()?;
        let cursor = copy.cursor();
        Some(CopyCursor {
            x: cursor.x,
            y: cursor.y,
        })
    }

    /// Panes in the window, beneath the quick terminal when it shows.
    pub(super) fn paint_copy_search(&self) -> anyhow::Result<()> {
        let mut painted = HeapQuadAllocator::default();
        let mut any = false;
        for pos in self.panes_beneath_terminal_overlay() {
            any |= self.copy_search_quads(&pos, &mut painted, (0., 0.))?;
        }
        if any {
            let layer = self
                .render_state
                .as_ref()
                .unwrap()
                .layer_for_zindex(ZINDEX)?;
            painted.apply_to(&mut layer.quad_allocator())?;
        }
        Ok(())
    }

    /// The quick terminal's pane, moved and clipped as its content is.
    pub(super) fn paint_copy_search_moved(
        &self,
        zindex: i8,
        pos: &PositionedPane,
        offset: (f32, f32),
        clip: (f32, f32, f32, f32),
    ) -> anyhow::Result<()> {
        let mut painted = HeapQuadAllocator::default();
        if self.copy_search_quads(pos, &mut painted, offset)? {
            let layer = self
                .render_state
                .as_ref()
                .unwrap()
                .layer_for_zindex(zindex)?;
            painted.apply_transformed(&mut layer.quad_allocator(), offset, 1., clip);
        }
        Ok(())
    }

    fn copy_search_pane_bounds(&self, pos: &PositionedPane) -> anyhow::Result<Bounds> {
        let (padding_left, padding_top) = self.pane_padding_left_top(pos.pane.pane_id());
        let tab_bar_height = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height()?
        } else {
            0.
        };
        let border = self.get_os_border();
        let cell = self.render_metrics.cell_size;
        Ok(Bounds {
            x: padding_left + border.left.get() as f32 + (pos.left as isize * cell.width) as f32,
            y: tab_bar_height
                + padding_top
                + border.top.get() as f32
                + (pos.top as isize * cell.height) as f32,
            width: (pos.width as isize * cell.width) as f32,
            height: (pos.height as isize * cell.height) as f32,
        })
    }

    /// Returns whether the pane shows the widget; `offset` moves the IME caret with the pane.
    fn copy_search_quads(
        &self,
        pos: &PositionedPane,
        quads: &mut HeapQuadAllocator,
        offset: (f32, f32),
    ) -> anyhow::Result<bool> {
        let Some(copy) = pos.pane.downcast_ref::<CopyOverlay>() else {
            return Ok(false);
        };
        let Some(palette) = self.ui_host.as_ref().map(|ui| ui.provider.search_palette()) else {
            return Ok(false);
        };
        if copy.set_highlights(highlights(&palette)) {
            if let Some(window) = &self.window {
                window.invalidate();
            }
        }
        let Some(view) = copy.search_view() else {
            return Ok(false);
        };
        let cell = (
            self.render_metrics.cell_size.width as f32,
            self.render_metrics.cell_size.height as f32,
        );
        let width = unicode_column_width(&view.query, None);
        let Some(layout) = Layout::new(
            self.copy_search_pane_bounds(pos)?,
            cell,
            self.dimensions.dpi as f32,
            width,
            trail(&view),
        ) else {
            return Ok(false);
        };
        let before = view.query.get(..view.cursor).unwrap_or(&view.query);
        let caret = unicode_column_width(before, None);
        let scroll = scroll(view.scroll, caret, width, layout.field);
        copy.scroll_search(scroll);

        let layers = &mut TripleLayerQuadAllocator::Heap(quads);
        let scale = (self.dimensions.dpi as f32 / 96.).max(1.);
        let radius = RADIUS * scale;
        let border = if no_results(&view) {
            Some(palette.error)
        } else {
            palette.border
        };
        let fill = match border {
            Some(color) => {
                self.paint_vtabs_rounded_fill(layers, layout.panel, radius, color.to_linear())?;
                let edge = scale.round();
                Bounds {
                    x: layout.panel.x + edge,
                    y: layout.panel.y + edge,
                    width: layout.panel.width - 2. * edge,
                    height: layout.panel.height - 2. * edge,
                }
            }
            None => layout.panel,
        };
        let radius = radius - (layout.panel.width - fill.width) / 2.;
        self.paint_vtabs_rounded_fill(layers, fill, radius, palette.background.to_linear())?;

        let line = line(&view, &layout, scroll, &palette);
        let gl_state = self.render_state.as_ref().unwrap();
        self.render_screen_line(
            RenderScreenLineParams {
                top_pixel_y: layout.text.1,
                left_pixel_x: layout.text.0,
                pixel_width: layout.columns() as f32 * cell.0,
                stable_line_idx: None,
                line: &line,
                selection: 0..0,
                cursor: &Default::default(),
                palette: &pos.pane.palette(),
                dims: &RenderableDimensions {
                    cols: layout.columns(),
                    physical_top: 0,
                    scrollback_rows: 0,
                    scrollback_top: 0,
                    viewport_rows: 1,
                    dpi: self.terminal_size.dpi,
                    pixel_height: cell.1 as usize,
                    pixel_width: (layout.columns() as f32 * cell.0) as usize,
                    reverse_video: false,
                },
                config: &self.config,
                cursor_border_color: LinearRgba::default(),
                foreground: palette.text.to_linear(),
                pane: None,
                is_active: true,
                selection_fg: LinearRgba::default(),
                selection_bg: LinearRgba::default(),
                cursor_fg: LinearRgba::default(),
                cursor_bg: LinearRgba::default(),
                cursor_is_default_color: true,
                white_space: gl_state.util_sprites.white_space.texture_coords(),
                filled_box: gl_state.util_sprites.filled_box.texture_coords(),
                window_is_transparent: true,
                default_bg: LinearRgba::default(),
                style: None,
                font: None,
                use_pixel_positioning: self.config.experimental_pixel_positioning,
                render_metrics: self.render_metrics,
                shape_key: None,
                password_input: false,
                center_icons: false,
            },
            layers,
        )?;

        if view.editing {
            let x = (layout.text.0 + (PROMPT + caret - scroll) as f32 * cell.0).round();
            let caret_width = (cell.0 * 0.12).round().max(1.);
            self.filled_rectangle(
                layers,
                2,
                euclid::rect(x, layout.text.1, caret_width, cell.1),
                palette.accent.to_linear(),
            )?;
            if let (true, Some(window)) = (pos.is_active, &self.window) {
                window.set_text_cursor_position(window::Rect::new(
                    euclid::point2((x + offset.0) as isize, (layout.text.1 + offset.1) as isize),
                    self.render_metrics.cell_size,
                ));
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
#[path = "../../tests/vtabs/copy_search.rs"]
mod vtabs_tests;

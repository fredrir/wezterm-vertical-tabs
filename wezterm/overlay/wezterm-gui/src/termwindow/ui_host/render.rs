//! Painting and quad reuse for the window-owned UI surface.
use super::{Bounds, Geometry, RoundedSurface};
use crate::quad::{HeapQuadAllocator, QuadTrait, TripleLayerQuadAllocator};
use crate::termwindow::TermWindow;
use crate::termwindow::render::RenderScreenLineParams;
use crate::termwindow::render::corners::*;
use mux::renderable::RenderableDimensions;
use std::{rc::Rc, time::Instant};
use termwiz::surface::Line;
use wezterm_term::color::ColorAttribute;
use window::{WindowOps, color::LinearRgba};

use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
const VTABS_UI_ZINDEX: i8 = 5;

#[derive(Default)]
pub(super) struct RenderCache {
    rows: Vec<RowCache>,
    primitives: Vec<PrimitiveCache>,
    primitive_key: (usize, usize, usize, u32, u32, u32, u32, u32),
    row_key: (usize, usize, usize, usize, usize, bool, u64),
    glyph_config: Option<((usize, u64), config::ConfigHandle)>,
}
impl RenderCache {
    /// Patched Nerd Fonts size each icon set differently, so the bundled symbols font leads.
    fn styled_config(&mut self, config: &config::ConfigHandle, scale: f64) -> config::ConfigHandle {
        let key = (config.generation(), scale.to_bits());
        if let Some((cached, handle)) = &self.glyph_config {
            if *cached == key {
                return handle.clone();
            }
        }
        let symbols = config::FontAttributes {
            scale: ordered_float::NotNan::new(scale).ok(),
            ..config::FontAttributes::new("Symbols Nerd Font Mono")
        };
        let mut styled = (**config).clone();
        for style in std::iter::once(&mut styled.font)
            .chain(styled.font_rules.iter_mut().map(|rule| &mut rule.font))
        {
            style.font.insert(0, symbols.clone());
        }
        let handle = config.derive(styled);
        self.glyph_config = Some((key, handle.clone()));
        handle
    }
}

fn vtabs_inset_bounds(mut bounds: Bounds, inset: f32, square: bool) -> Bounds {
    let inset = if inset.is_finite() { inset.max(0.) } else { 0. };
    if square {
        let side = bounds.width.min(bounds.height).max(0.);
        bounds.x += (bounds.width - side) / 2.;
        bounds.y += (bounds.height - side) / 2.;
        bounds.width = side;
        bounds.height = side;
    }
    let x = inset.min(bounds.width.max(0.) / 2.);
    let y = inset.min(bounds.height.max(0.) / 2.);
    bounds.x += x;
    bounds.y += y;
    bounds.width = (bounds.width - x * 2.).max(0.);
    bounds.height = (bounds.height - y * 2.).max(0.);
    bounds
}
struct PrimitiveCache {
    surface: RoundedSurface,
    quads: HeapQuadAllocator,
}
struct RowCache {
    line: Rc<Line>,
    centered_ranges: Vec<(f32, f32)>,
    quads: HeapQuadAllocator,
    origin: (f32, f32),
}
impl RowCache {
    fn matches(&self, line: &Rc<Line>, centered_ranges: &[(f32, f32)]) -> bool {
        Rc::ptr_eq(&self.line, line) && self.centered_ranges == centered_ranges
    }
}
fn vtabs_caret_rect(
    geometry: Geometry,
    offset: (f32, f32),
    caret: (usize, usize),
    whole_window: bool,
) -> Option<window::Rect> {
    let bounds = geometry.ui_bounds(whole_window);
    let x = bounds.x + offset.0 + caret.0 as f32 * geometry.cell_width;
    let y = bounds.y + offset.1 + caret.1 as f32 * geometry.cell_height;
    let bounds: window::RectF = euclid::rect(bounds.x, bounds.y, bounds.width, bounds.height);
    // Bounds lead so a NaN caret coordinate yields an empty intersection.
    let caret = euclid::rect(x, y, geometry.cell_width, geometry.cell_height);
    bounds.intersection(&caret)?.round_out().try_cast()
}
const FRAME_TOP_LEFT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
        PolyCommand::QuadTo {
            control: (BlockCoord::Zero, BlockCoord::Zero),
            to: (BlockCoord::Zero, BlockCoord::One),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];
const FRAME_TOP_RIGHT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
        PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
        PolyCommand::QuadTo {
            control: (BlockCoord::One, BlockCoord::Zero),
            to: (BlockCoord::Zero, BlockCoord::Zero),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];
const FRAME_BOTTOM_LEFT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero),
        PolyCommand::QuadTo {
            control: (BlockCoord::Zero, BlockCoord::One),
            to: (BlockCoord::One, BlockCoord::One),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];
const FRAME_BOTTOM_RIGHT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::One),
        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
        PolyCommand::QuadTo {
            control: (BlockCoord::One, BlockCoord::One),
            to: (BlockCoord::One, BlockCoord::Zero),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];

impl TermWindow {
    pub(in crate::termwindow) fn paint_vtabs_rounded_fill(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        rect: Bounds,
        radius: f32,
        color: LinearRgba,
    ) -> anyhow::Result<()> {
        if ![rect.x, rect.y, rect.width, rect.height, radius]
            .iter()
            .all(|value| value.is_finite())
            || rect.width <= 0.
            || rect.height <= 0.
            || color.tuple().3 <= 0.
        {
            return Ok(());
        }
        let radius = radius.max(0.).min(rect.width / 2.).min(rect.height / 2.);
        if radius < 1. {
            self.filled_rectangle(
                layers,
                0,
                euclid::rect(rect.x, rect.y, rect.width, rect.height),
                color,
            )?;
            return Ok(());
        }
        // Disjoint center strips avoid doubling alpha on translucent surfaces.
        for (x, y, width, height) in [
            (
                rect.x + radius,
                rect.y,
                rect.width - radius * 2.,
                rect.height,
            ),
            (rect.x, rect.y + radius, radius, rect.height - radius * 2.),
            (
                rect.x + rect.width - radius,
                rect.y + radius,
                radius,
                rect.height - radius * 2.,
            ),
        ] {
            if width > 0. && height > 0. {
                self.filled_rectangle(layers, 0, euclid::rect(x, y, width, height), color)?;
            }
        }
        for (x, y, corner) in [
            (rect.x, rect.y, TOP_LEFT_ROUNDED_CORNER),
            (
                rect.x + rect.width - radius,
                rect.y,
                TOP_RIGHT_ROUNDED_CORNER,
            ),
            (
                rect.x,
                rect.y + rect.height - radius,
                BOTTOM_LEFT_ROUNDED_CORNER,
            ),
            (
                rect.x + rect.width - radius,
                rect.y + rect.height - radius,
                BOTTOM_RIGHT_ROUNDED_CORNER,
            ),
        ] {
            self.poly_quad(
                layers,
                0,
                euclid::point2(x, y),
                corner,
                0,
                euclid::size2(radius, radius),
                color,
            )?
            .set_grayscale();
        }
        Ok(())
    }
    fn paint_vtabs_frame(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        content: Bounds,
        color: LinearRgba,
    ) -> anyhow::Result<()> {
        let dpi = self.dimensions.dpi as f32;
        let Bounds {
            x,
            y,
            width,
            height,
        } = content;
        if ![x, y, width, height, dpi]
            .iter()
            .all(|value| value.is_finite())
            || width <= 0.
            || height <= 0.
            || dpi <= 0.
        {
            return Ok(());
        }
        let edge = (8. * dpi / 96.)
            .round()
            .max(0.)
            .min(width / 2.)
            .min(height / 2.);
        let radius = (Self::vtabs_frame_gutter_impl(dpi) - edge)
            .max(0.)
            .min((width / 2. - edge).max(0.))
            .min((height / 2. - edge).max(0.));
        for (x, y, width, height) in [
            (x, y, width, edge),
            (x, y + height - edge, width, edge),
            (x, y + edge, edge, height - edge * 2.),
            (x + width - edge, y + edge, edge, height - edge * 2.),
        ] {
            if width > 0. && height > 0. {
                self.filled_rectangle(layers, 0, euclid::rect(x, y, width, height), color)?;
            }
        }
        if radius >= 1. {
            for (x, y, corner) in [
                (x + edge, y + edge, FRAME_TOP_LEFT),
                (x + width - edge - radius, y + edge, FRAME_TOP_RIGHT),
                (x + edge, y + height - edge - radius, FRAME_BOTTOM_LEFT),
                (
                    x + width - edge - radius,
                    y + height - edge - radius,
                    FRAME_BOTTOM_RIGHT,
                ),
            ] {
                self.poly_quad(
                    layers,
                    0,
                    euclid::point2(x, y),
                    corner,
                    0,
                    euclid::size2(radius, radius),
                    color,
                )?
                .set_grayscale();
            }
        }
        Ok(())
    }
    pub fn paint_vtabs_ui(&mut self) -> anyhow::Result<()> {
        let geometry = self.vtabs_geometry();
        // All terminal image, text and background layers remain below the frame and popovers.
        let layer = self
            .render_state
            .as_ref()
            .unwrap()
            .layer_for_zindex(VTABS_UI_ZINDEX)?;
        let mut allocation = layer.quad_allocator();
        let layers = &mut allocation;
        let Some(mut ui) = self.ui_host.take() else {
            return Ok(());
        };
        let result = (|| {
            let start = Instant::now();
            self.ensure_vtabs_titlebar(&mut ui, geometry.header_width())?;
            if let Some(chrome) = ui.chrome.element() {
                self.render_element(chrome, self.render_state.as_ref().unwrap(), None)?;
            }
            ui.geometry = geometry;
            ui.provider.render(geometry, start);
            let styled = ui
                .render
                .styled_config(&self.config, ui.provider.glyph_scale());
            let surface = ui.provider.surface();
            let bounds =
                geometry.ui_bounds(ui.provider.content_page() || ui.provider.overlay_surface());
            // Transient surfaces leave the terminal visible beneath their own rounded shapes.
            let background_bounds = geometry.ui_bounds(false);
            let rounded = !ui.provider.primitives().is_empty();
            let clip = (
                bounds.x - self.dimensions.pixel_width as f32 / 2.,
                bounds.y - self.dimensions.pixel_height as f32 / 2.,
                bounds.x + bounds.width - self.dimensions.pixel_width as f32 / 2.,
                bounds.y + bounds.height - self.dimensions.pixel_height as f32 / 2.,
            );
            let origin = (clip.0, clip.1);
            let grid_width = surface.columns as f32 * geometry.cell_width;
            let key = (
                self.shape_generation,
                usize::from(self.focused.is_some()),
                self.config.generation(),
                geometry.cell_width as usize,
                geometry.cell_height as usize,
                rounded,
                ui.provider.glyph_scale().to_bits(),
            );
            if ui.render.row_key != key {
                ui.render.rows.clear();
                ui.render.row_key = key;
            }
            ui.render.rows.truncate(surface.rows.len());
            let palette = self.palette().clone();
            let gl_state = self.render_state.as_ref().unwrap();
            let white_space = gl_state.util_sprites.white_space.texture_coords();
            let filled_box = gl_state.util_sprites.filled_box.texture_coords();
            let background = ui
                .provider
                .background()
                .or_else(|| {
                    surface.rows.first().and_then(|line| {
                        line.visible_cells()
                            .next()
                            .map(|cell| palette.resolve_bg(cell.attrs().background()).to_linear())
                    })
                })
                .unwrap_or(palette.background.to_linear());
            if self.vtabs_integrated_macos() {
                let border = self.get_os_border();
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(
                        border.left.get() as f32,
                        border.top.get() as f32,
                        geometry.header_width(),
                        self.vtabs_titlebar_height(),
                    ),
                    background,
                )?;
            }
            if ui.provider.content_page() {
                // Pages fill the content frame the way a shell's background does.
                let page = palette
                    .background
                    .to_linear()
                    .mul_alpha(self.config.window_background_opacity);
                let content = geometry.content;
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(content.x, content.y, content.width, content.height),
                    page,
                )?;
            }
            self.paint_vtabs_frame(layers, geometry.content, background)?;
            self.filled_rectangle(
                layers,
                0,
                euclid::rect(
                    background_bounds.x,
                    background_bounds.y,
                    background_bounds.width,
                    background_bounds.height,
                ),
                background,
            )?;
            let primitive_key = (
                self.shape_generation,
                self.dimensions.pixel_width,
                self.dimensions.pixel_height,
                geometry.cell_width.to_bits(),
                geometry.cell_height.to_bits(),
                geometry.dpi.to_bits(),
                bounds.x.to_bits(),
                bounds.y.to_bits(),
            );
            if ui.render.primitive_key != primitive_key {
                // Corner glyph UVs belong to one atlas, even when surface geometry is unchanged.
                ui.render.primitives.clear();
                ui.render.primitive_key = primitive_key;
            }
            ui.render
                .primitives
                .truncate(ui.provider.primitives().len());
            for (index, primitive) in ui.provider.primitives().iter().enumerate() {
                if ui
                    .render
                    .primitives
                    .get(index)
                    .is_none_or(|cached| cached.surface != *primitive)
                {
                    let rect = Bounds {
                        x: bounds.x + primitive.bounds.x * geometry.cell_width,
                        y: bounds.y + primitive.bounds.y * geometry.cell_height,
                        width: primitive.bounds.width * geometry.cell_width,
                        height: primitive.bounds.height * geometry.cell_height,
                    };
                    let scale = geometry.dpi / 96.;
                    let rect = vtabs_inset_bounds(rect, primitive.inset * scale, primitive.square);
                    let mut quads = HeapQuadAllocator::default();
                    self.paint_vtabs_rounded_fill(
                        &mut TripleLayerQuadAllocator::Heap(&mut quads),
                        rect,
                        primitive.radius * scale,
                        primitive.fill,
                    )?;
                    let cached = PrimitiveCache {
                        surface: *primitive,
                        quads,
                    };
                    if index < ui.render.primitives.len() {
                        ui.render.primitives[index] = cached;
                    } else {
                        ui.render.primitives.push(cached);
                    }
                }
                ui.render.primitives[index].quads.apply_transformed(
                    layers,
                    surface.offset,
                    surface.opacity,
                    clip,
                );
            }
            for (y, line) in surface.rows.iter().enumerate() {
                let centered_ranges = surface.centered.get(y).map_or(&[][..], Vec::as_slice);
                if ui
                    .render
                    .rows
                    .get(y)
                    .is_none_or(|cached| !cached.matches(line, centered_ranges))
                {
                    let mut quads = HeapQuadAllocator::default();
                    self.render_screen_line(
                        RenderScreenLineParams {
                            top_pixel_y: bounds.y + y as f32 * geometry.cell_height,
                            left_pixel_x: bounds.x,
                            pixel_width: grid_width,
                            stable_line_idx: None,
                            line,
                            selection: 0..0,
                            cursor: &Default::default(),
                            palette: &palette,
                            dims: &RenderableDimensions {
                                cols: surface.columns,
                                physical_top: 0,
                                scrollback_rows: 0,
                                scrollback_top: 0,
                                viewport_rows: surface.rows.len(),
                                dpi: self.terminal_size.dpi,
                                pixel_height: (surface.rows.len() as f32 * geometry.cell_height)
                                    as usize,
                                pixel_width: grid_width as usize,
                                reverse_video: false,
                            },
                            config: &styled,
                            cursor_border_color: LinearRgba::default(),
                            foreground: palette.foreground.to_linear(),
                            pane: None,
                            is_active: true,
                            selection_fg: LinearRgba::default(),
                            selection_bg: LinearRgba::default(),
                            cursor_fg: LinearRgba::default(),
                            cursor_bg: LinearRgba::default(),
                            cursor_is_default_color: true,
                            white_space,
                            filled_box,
                            window_is_transparent: false,
                            default_bg: palette.resolve_bg(ColorAttribute::Default).to_linear(),
                            style: None,
                            font: None,
                            use_pixel_positioning: false,
                            render_metrics: self.render_metrics,
                            shape_key: None,
                            password_input: false,
                        },
                        &mut TripleLayerQuadAllocator::Heap(&mut quads),
                    )?;
                    if rounded {
                        quads.remove_solid_backgrounds();
                        quads.offset_glyphs_in_ranges(
                            centered_ranges,
                            origin.0,
                            geometry.cell_height / 2.,
                        );
                    }
                    let cached = RowCache {
                        line: Rc::clone(line),
                        centered_ranges: centered_ranges.to_vec(),
                        quads,
                        origin,
                    };
                    if y < ui.render.rows.len() {
                        ui.render.rows[y] = cached;
                    } else {
                        ui.render.rows.push(cached);
                    }
                }
                ui.render.rows[y].quads.apply_transformed(
                    layers,
                    (
                        surface.offset.0 + origin.0 - ui.render.rows[y].origin.0,
                        surface.offset.1 + origin.1 - ui.render.rows[y].origin.1,
                    ),
                    surface.opacity,
                    clip,
                );
            }
            if ui.provider.keyboard_focus() {
                if let (Some(window), Some(caret)) = (&self.window, ui.provider.caret()) {
                    let mut offset = surface.offset;
                    let x = (caret.0 as f32 + 0.5) * geometry.cell_width;
                    if surface.centered.get(caret.1).is_some_and(|ranges| {
                        ranges.iter().any(|(left, right)| x >= *left && x < *right)
                    }) {
                        offset.1 += geometry.cell_height / 2.;
                    }
                    if let Some(rect) = vtabs_caret_rect(
                        geometry,
                        offset,
                        caret,
                        ui.provider.content_page() || ui.provider.overlay_surface(),
                    ) {
                        window.set_text_cursor_position(rect);
                    }
                }
            }
            metrics::histogram!("ui_host.paint").record(start.elapsed());
            Ok(())
        })();
        self.ui_host = Some(ui);
        self.vtabs_schedule_provider(self.mux_window_id);
        result
    }
}

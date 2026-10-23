use super::*;
use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::quad::QuadTrait;

const TOP_LEFT: &[Poly] = &[Poly {
    path: &[PolyCommand::Oval {
        center: (BlockCoord::One, BlockCoord::One),
        radiuses: (BlockCoord::Frac(23, 24), BlockCoord::Frac(23, 24)),
    }],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Outline,
}];

const TOP_RIGHT: &[Poly] = &[Poly {
    path: &[PolyCommand::Oval {
        center: (BlockCoord::Zero, BlockCoord::One),
        radiuses: (BlockCoord::Frac(23, 24), BlockCoord::Frac(23, 24)),
    }],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Outline,
}];

const BOTTOM_LEFT: &[Poly] = &[Poly {
    path: &[PolyCommand::Oval {
        center: (BlockCoord::One, BlockCoord::Zero),
        radiuses: (BlockCoord::Frac(23, 24), BlockCoord::Frac(23, 24)),
    }],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Outline,
}];

const BOTTOM_RIGHT: &[Poly] = &[Poly {
    path: &[PolyCommand::Oval {
        center: (BlockCoord::Zero, BlockCoord::Zero),
        radiuses: (BlockCoord::Frac(23, 24), BlockCoord::Frac(23, 24)),
    }],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Outline,
}];

impl TermWindow {
    /// Disjoint edges and stroked corners leave the translucent center untouched.
    fn paint_terminal_border(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        rect: Bounds,
        border: f32,
        color: LinearRgba,
    ) -> anyhow::Result<()> {
        let radius = corner_radius(self.dimensions.dpi as u32)
            .min(rect.width / 2.)
            .min(rect.height / 2.);
        for (x, y, width, height) in [
            (rect.x + radius, rect.y, rect.width - 2. * radius, border),
            (
                rect.x + radius,
                rect.y + rect.height - border,
                rect.width - 2. * radius,
                border,
            ),
            (rect.x, rect.y + radius, border, rect.height - 2. * radius),
            (
                rect.x + rect.width - border,
                rect.y + radius,
                border,
                rect.height - 2. * radius,
            ),
        ] {
            if width > 0. && height > 0. {
                self.filled_rectangle(layers, 0, euclid::rect(x, y, width, height), color)?;
            }
        }
        for (x, y, corner) in [
            (rect.x, rect.y, TOP_LEFT),
            (rect.x + rect.width - radius, rect.y, TOP_RIGHT),
            (rect.x, rect.y + rect.height - radius, BOTTOM_LEFT),
            (
                rect.x + rect.width - radius,
                rect.y + rect.height - radius,
                BOTTOM_RIGHT,
            ),
        ] {
            self.poly_quad(
                layers,
                0,
                euclid::point2(x, y),
                corner,
                border.round() as _,
                euclid::size2(radius, radius),
                color,
            )?
            .set_grayscale();
        }
        Ok(())
    }

    pub(in crate::termwindow) fn paint_terminal_overlay(&mut self) -> anyhow::Result<()> {
        if !self.terminal_overlay_visible() {
            return Ok(());
        }
        let drawn = self.terminal_overlay_drawn();
        let (layout, painted) = (drawn.layout, drawn.painted);
        let Some(config) = self.terminal_overlay_config() else {
            return Ok(());
        };
        if drawn.moving {
            let interval = Duration::from_secs(1) / self.config.max_fps.clamp(1, 1000) as u32;
            self.update_next_frame_time(Some(Instant::now() + interval));
        }
        let layer = self
            .render_state
            .as_ref()
            .unwrap()
            .layer_for_zindex(ZINDEX)?;
        let mut layers = layer.quad_allocator();
        self.filled_rectangle(
            &mut layers,
            0,
            euclid::rect(
                0.,
                0.,
                self.dimensions.pixel_width as f32,
                self.dimensions.pixel_height as f32,
            ),
            LinearRgba::with_components(0., 0., 0., 0.2),
        )?;
        let background = config
            .background_color
            .unwrap_or_else(|| {
                self.terminal_overlay_pane()
                    .map(|pane| pane.palette().background)
                    .unwrap_or(self.palette().background)
                    .to_linear()
            })
            .mul_alpha(config.opacity);
        let scale = border_width(self.dimensions.dpi as u32);
        let border_color = match config.border {
            TerminalBorder::None => None,
            TerminalBorder::Palette => {
                let foreground = self.terminal_overlay_pane().map_or_else(
                    || self.palette().foreground,
                    |pane| pane.palette().foreground,
                );
                Some(foreground.to_linear().mul_alpha(0.22))
            }
            TerminalBorder::Color(color) => Some(color),
        };
        let border = if let Some(color) = border_color {
            self.paint_terminal_border(&mut layers, painted.frame, scale, color)?;
            scale
        } else {
            0.
        };
        self.paint_vtabs_rounded_fill(
            &mut layers,
            Bounds {
                x: painted.frame.x + border,
                y: painted.frame.y + border,
                width: (painted.frame.width - 2. * border).max(0.),
                height: (painted.frame.height - 2. * border).max(0.),
            },
            corner_radius(self.dimensions.dpi as u32) - border,
            background,
        )?;
        if let Some(pos) = self.positioned_terminal_overlay() {
            self.update_text_cursor(&pos);
            if self.focused.is_some() {
                pos.pane.advise_focus();
            }
            let mut quads = HeapQuadAllocator::default();
            self.paint_pane(&pos, &mut TripleLayerQuadAllocator::Heap(&mut quads))?;
            let x = self.dimensions.pixel_width as f32 / 2.;
            let y = self.dimensions.pixel_height as f32 / 2.;
            let content = layout.content;
            let offset = (drawn.origin.0 - content.x, drawn.origin.1 - content.y);
            let inner = painted.inner;
            let clip = (
                drawn.origin.0.max(inner.x) - x,
                drawn.origin.1.max(inner.y) - y,
                (drawn.origin.0 + content.width).min(inner.x + inner.width) - x,
                (drawn.origin.1 + content.height).min(inner.y + inner.height) - y,
            );
            quads.apply_transformed(&mut layers, offset, 1., clip);
            self.paint_copy_search_moved(ZINDEX + 1, &pos, offset, clip)?;
        }
        Ok(())
    }
}

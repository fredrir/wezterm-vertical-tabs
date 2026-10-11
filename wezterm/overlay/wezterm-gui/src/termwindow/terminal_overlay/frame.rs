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
    pub(super) fn paint_terminal_border(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        rect: Bounds,
        border: f32,
        color: LinearRgba,
    ) -> anyhow::Result<()> {
        let radius = (12. * border).min(rect.width / 2.).min(rect.height / 2.);
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
}

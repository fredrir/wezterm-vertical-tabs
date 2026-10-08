//! Cached quads replayed with an offset, opacity and clip, and their centering adjustments.
use super::{
    HeapQuadAllocator, TripleLayerQuadAllocator, TripleLayerQuadAllocatorTrait, IS_BG_IMAGE,
    IS_SOLID_COLOR,
};

impl HeapQuadAllocator {
    pub fn remove_solid_backgrounds(&mut self) {
        self.layer0.retain(|quad| quad.has_color != IS_SOLID_COLOR);
    }

    pub fn offset_glyphs_in_ranges(&mut self, ranges: &[(f32, f32)], origin_x: f32, dy: f32) {
        if ranges.is_empty() || !origin_x.is_finite() || !dy.is_finite() || dy == 0. {
            return;
        }
        for quads in [&mut self.layer0, &mut self.layer1, &mut self.layer2] {
            for quad in quads {
                if quad.has_color == IS_SOLID_COLOR || quad.has_color == IS_BG_IMAGE {
                    continue;
                }
                let center = (quad.position.0 + quad.position.2) / 2. - origin_x;
                // Nested button and drag-target surfaces can overlap the same glyph.
                if ranges
                    .iter()
                    .any(|(left, right)| center >= *left && center < *right)
                {
                    quad.position.1 += dy;
                    quad.position.3 += dy;
                }
            }
        }
    }

    pub fn apply_transformed(
        &self,
        other: &mut TripleLayerQuadAllocator,
        offset: (f32, f32),
        opacity: f32,
        clip: (f32, f32, f32, f32),
    ) {
        for (layer, quads) in [(0, &self.layer0), (1, &self.layer1), (2, &self.layer2)] {
            for quad in quads {
                let mut vertices = quad.to_vertices();
                let (left, top, right, bottom) = quad.position;
                let (left, top, right, bottom) = (
                    left + offset.0,
                    top + offset.1,
                    right + offset.0,
                    bottom + offset.1,
                );
                let (x1, y1, x2, y2) = (
                    left.max(clip.0),
                    top.max(clip.1),
                    right.min(clip.2),
                    bottom.min(clip.3),
                );
                if x1 >= x2 || y1 >= y2 {
                    continue;
                }
                let (u1, u2, v1, v2) = quad.tex;
                let uv = |x: f32, y: f32| {
                    [
                        u1 + (u2 - u1) * (x - left) / (right - left),
                        v1 + (v2 - v1) * (y - top) / (bottom - top),
                    ]
                };
                for (vertex, (x, y)) in vertices
                    .iter_mut()
                    .zip([(x1, y1), (x2, y1), (x1, y2), (x2, y2)])
                {
                    vertex.position = [x, y];
                    vertex.tex = uv(x, y);
                    vertex.fg_color[3] *= opacity;
                    vertex.alt_color[3] *= opacity;
                }
                other.extend_with(layer, &vertices);
            }
        }
    }
}

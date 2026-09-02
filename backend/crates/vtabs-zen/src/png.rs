//! RGBA8 canvas and rounded-card renderer, encoded with the `png` crate.

/// An RGBA8 canvas that knows how to write itself out as a PNG.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for _ in 0..(width as usize) * (height as usize) {
            pixels.extend_from_slice(&fill);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    fn set(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = ((y as usize) * (self.width as usize) + x as usize) * 4;
        self.pixels[at..at + 4].copy_from_slice(&rgba);
    }

    /// `coverage` in 0..=1 blends `rgba` over what is already there, which is what antialiases an
    /// edge. Fully covered pixels take the colour verbatim, so a fill stays exact.
    fn blend(&mut self, x: u32, y: u32, rgba: [u8; 4], coverage: f32) {
        if coverage <= 0.0 {
            return;
        }
        if coverage >= 1.0 {
            self.set(x, y, rgba);
            return;
        }
        if x >= self.width || y >= self.height {
            return;
        }
        let at = ((y as usize) * (self.width as usize) + x as usize) * 4;
        for (i, over) in rgba.iter().enumerate() {
            let under = self.pixels[at + i] as f32;
            let blended = under + (*over as f32 - under) * coverage;
            self.pixels[at + i] = blended.round().clamp(0.0, 255.0) as u8;
        }
    }

    fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y as usize) * (self.width as usize) + x as usize) * 4;
        [
            self.pixels[at],
            self.pixels[at + 1],
            self.pixels[at + 2],
            self.pixels[at + 3],
        ]
    }

    /// Fraction of the pixel at `x, y` inside a rounded rectangle, from the signed distance of its
    /// centre: exact on a straight edge, smooth on an arc, and uniform for a stroke of any width,
    /// where 4x4 supersampling had 17 levels and beaded along the corners.
    fn coverage(&self, x: u32, y: u32, rect: &RoundRect) -> f32 {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return 0.0;
        }
        (0.5 - rect.distance(x as f32 + 0.5, y as f32 + 0.5)).clamp(0.0, 1.0)
    }

    /// True where a pixel needs sampling: within a pixel of a straight edge, or inside a corner's
    /// own box. Everywhere else is wholly inside, and the window-sized canvas makes that the
    /// difference between a memset and a hundred million samples on the GUI thread.
    fn needs_sampling(rect: &RoundRect, x: u32, y: u32) -> bool {
        let (px, py) = (x as f32, y as f32);
        let (x0, y0) = (rect.x, rect.y);
        let (x1, y1) = (rect.x + rect.w, rect.y + rect.h);
        let near_edge =
            px < x0 + 1.0 || px + 1.0 > x1 - 1.0 || py < y0 + 1.0 || py + 1.0 > y1 - 1.0;
        let in_corner_col = px < x0 + rect.r || px + 1.0 > x1 - rect.r;
        let in_corner_row = py < y0 + rect.r || py + 1.0 > y1 - rect.r;
        near_edge || (in_corner_col && in_corner_row)
    }

    pub fn rounded_rect(&mut self, rect: &RoundRect, fill: [u8; 4]) {
        let (x0, y0, x1, y1) = rect.bounds(self.width, self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                if Self::needs_sampling(rect, x, y) {
                    self.blend(x, y, fill, self.coverage(x, y, rect));
                } else {
                    self.set(x, y, fill);
                }
            }
        }
    }

    /// The card and its border in one pass: an edge pixel mixes page, stroke and fill once, where
    /// painting the fill and then the ring over it left a fill-coloured halo outside the stroke.
    pub fn rounded_card(
        &mut self,
        rect: &RoundRect,
        fill: [u8; 4],
        border: Option<([u8; 4], f32)>,
    ) {
        let border = border.filter(|(_, width)| *width > 0.0);
        let inner = border.map(|(_, width)| rect.inset(width));
        let (x0, y0, x1, y1) = rect.bounds(self.width, self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let plain = !Self::needs_sampling(rect, x, y)
                    && inner
                        .as_ref()
                        .is_none_or(|i| !Self::needs_sampling(i, x, y));
                if plain {
                    self.set(x, y, fill);
                    continue;
                }
                let cover = self.coverage(x, y, rect);
                let ring = match (&inner, border) {
                    (Some(i), Some(_)) => (cover - self.coverage(x, y, i)).clamp(0.0, cover),
                    _ => 0.0,
                };
                let stroke = border.map_or(fill, |(colour, _)| colour);
                let page = self.get(x, y);
                let mut out = [0u8; 4];
                for (i, slot) in out.iter_mut().enumerate() {
                    let mixed = page[i] as f32 * (1.0 - cover)
                        + stroke[i] as f32 * ring
                        + fill[i] as f32 * (cover - ring);
                    *slot = mixed.round().clamp(0.0, 255.0) as u8;
                }
                self.set(x, y, out);
            }
        }
    }

    /// The border is the fill's coverage minus an inset copy's: one pass, no seams.
    pub fn rounded_border(&mut self, rect: &RoundRect, colour: [u8; 4], width: f32) {
        if width <= 0.0 {
            return;
        }
        let inner = rect.inset(width);
        let (x0, y0, x1, y1) = rect.bounds(self.width, self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                // Wholly inside the inset copy is wholly off the ring: only the band between the
                // two outlines can carry any of it.
                if !Self::needs_sampling(rect, x, y) && !Self::needs_sampling(&inner, x, y) {
                    continue;
                }
                let ring = self.coverage(x, y, rect) - self.coverage(x, y, &inner);
                self.blend(x, y, colour, ring.clamp(0.0, 1.0));
            }
        }
    }

    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::Filter::Sub);
            let mut writer = encoder.write_header().expect("valid PNG header");
            writer
                .write_image_data(&self.pixels)
                .expect("valid RGBA8 pixel buffer");
        }
        out
    }
}

/// A rectangle with equal corner radii, in pixel space.
pub struct RoundRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
}

impl RoundRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32, r: f32) -> Self {
        let r = r.max(0.0).min(w.max(0.0) / 2.0).min(h.max(0.0) / 2.0);
        Self { x, y, w, h, r }
    }

    fn inset(&self, by: f32) -> Self {
        Self::new(
            self.x + by,
            self.y + by,
            (self.w - by * 2.0).max(0.0),
            (self.h - by * 2.0).max(0.0),
            (self.r - by).max(0.0),
        )
    }

    fn bounds(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let x0 = self.x.floor().max(0.0) as u32;
        let y0 = self.y.floor().max(0.0) as u32;
        let x1 = ((self.x + self.w).ceil().max(0.0) as u32).min(width);
        let y1 = ((self.y + self.h).ceil().max(0.0) as u32).min(height);
        (x0.min(x1), y0.min(y1), x1, y1)
    }

    /// Signed distance from a point to the outline, negative inside: the rounded-box distance
    /// field, so the arcs and the straight runs are one continuous function.
    fn distance(&self, px: f32, py: f32) -> f32 {
        let (half_w, half_h) = (self.w / 2.0, self.h / 2.0);
        let qx = (px - self.x - half_w).abs() - half_w + self.r;
        let qy = (py - self.y - half_h).abs() - half_h + self.r;
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        outside + qx.max(qy).min(0.0) - self.r
    }
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa`; alpha defaults to opaque.
pub fn parse_colour(spec: &str) -> Option<[u8; 4]> {
    let hex = spec.strip_prefix('#').unwrap_or(spec);
    let hex = hex.as_bytes();
    let nibble = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let byte = |at: usize| Some(nibble(*hex.get(at)?)? * 16 + nibble(*hex.get(at + 1)?)?);
    match hex {
        [r, g, b] => {
            let mut out = [0u8, 0, 0, 255];
            for (i, c) in [r, g, b].into_iter().enumerate() {
                let v = nibble(*c)?;
                out[i] = v * 17;
            }
            Some(out)
        }
        [_, _, _, _, _, _] => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        [_, _, _, _, _, _, _, _] => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/png.rs"]
mod tests;

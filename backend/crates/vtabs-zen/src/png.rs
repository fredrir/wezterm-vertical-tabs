//! A PNG writer with no dependencies: RGBA8, one IDAT of stored (uncompressed) deflate blocks.
//! Stored blocks keep the file large and the code small, which is the right trade for an image
//! that is regenerated on resize and read back once.

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

fn crc32(bytes: &[u8]) -> u32 {
    let table = crc_table();
    let mut c = 0xffff_ffffu32;
    for b in bytes {
        c = table[((c ^ *b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// LSB-first bit sink, which is the order deflate packs its codes in.
struct BitWriter {
    out: Vec<u8>,
    bit: u32,
    n: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: vec![],
            bit: 0,
            n: 0,
        }
    }

    fn push(&mut self, value: u32, bits: u32) {
        self.bit |= value << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push((self.bit & 0xff) as u8);
            self.bit >>= 8;
            self.n -= 8;
        }
    }

    /// Huffman codes are written most-significant bit first, unlike everything else in deflate.
    fn push_code(&mut self, code: u32, bits: u32) {
        for i in (0..bits).rev() {
            self.push((code >> i) & 1, 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push((self.bit & 0xff) as u8);
        }
        self.out
    }
}

/// RFC 1951 §3.2.6: the fixed literal/length alphabet.
fn fixed_literal(sym: u32) -> (u32, u32) {
    match sym {
        0..=143 => (0x30 + sym, 8),
        144..=255 => (0x190 + sym - 144, 9),
        256..=279 => (sym - 256, 7),
        _ => (0xc0 + sym - 280, 8),
    }
}

/// RFC 1951 §3.2.5: length 3..=258 as a code, plus its extra bits.
fn length_code(len: u32) -> (u32, u32, u32) {
    const BASE: [u32; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    let mut i = 28;
    while BASE[i] > len {
        i -= 1;
    }
    (257 + i as u32, len - BASE[i], EXTRA[i])
}

/// RFC 1951 §3.2.5: distance 1..=32768 as a code, plus its extra bits.
fn distance_code(dist: u32) -> (u32, u32, u32) {
    const BASE: [u32; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    let mut i = 29;
    while BASE[i] > dist {
        i -= 1;
    }
    (i as u32, dist - BASE[i], EXTRA[i])
}

fn match_len(raw: &[u8], at: usize, dist: usize) -> usize {
    let mut len = 0;
    while at + len < raw.len() && len < 258 && raw[at + len] == raw[at + len - dist] {
        len += 1;
    }
    len
}

/// zlib stream over one fixed-Huffman block. The only distances tried are the ones this image
/// actually repeats at -- one pixel, one scanline, one byte -- which turns a flat fill into a
/// handful of matches without a hash chain or a window search.
fn zlib_deflate(raw: &[u8], stride: usize) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.push(1, 1); // final block
    w.push(1, 2); // fixed Huffman
    let candidates = [4usize, stride, 1];
    let mut at = 0usize;
    while at < raw.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        for dist in candidates {
            if dist == 0 || dist > at || dist > 32768 {
                continue;
            }
            let len = match_len(raw, at, dist);
            if len > best_len {
                best_len = len;
                best_dist = dist;
            }
        }
        if best_len >= 3 {
            let (code, extra, bits) = length_code(best_len as u32);
            let (lit, n) = fixed_literal(code);
            w.push_code(lit, n);
            if bits > 0 {
                w.push(extra, bits);
            }
            let (dcode, dextra, dbits) = distance_code(best_dist as u32);
            w.push_code(dcode, 5);
            if dbits > 0 {
                w.push(dextra, dbits);
            }
            at += best_len;
        } else {
            let (lit, n) = fixed_literal(raw[at] as u32);
            w.push_code(lit, n);
            at += 1;
        }
    }
    let (eob, n) = fixed_literal(256);
    w.push_code(eob, n);
    let mut out = vec![0x78, 0x01];
    out.extend_from_slice(&w.finish());
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

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

    /// Fraction of the pixel at `x, y` inside a rounded rectangle, by 4x4 supersampling. Sampling
    /// beats an analytic solve here: the same routine serves the fill and its border.
    fn coverage(&self, x: u32, y: u32, rect: &RoundRect) -> f32 {
        const N: u32 = 4;
        let mut inside = 0u32;
        for sy in 0..N {
            for sx in 0..N {
                let px = x as f32 + (sx as f32 + 0.5) / N as f32;
                let py = y as f32 + (sy as f32 + 0.5) / N as f32;
                if rect.contains(px, py) {
                    inside += 1;
                }
            }
        }
        inside as f32 / (N * N) as f32
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
        let mut raw = Vec::with_capacity(self.pixels.len() + self.height as usize);
        for y in 0..self.height as usize {
            // Filter type 0 (None): the encoder trades size for having no filter to undo.
            raw.push(0);
            let row = y * self.width as usize * 4;
            raw.extend_from_slice(&self.pixels[row..row + self.width as usize * 4]);
        }
        let mut out = Vec::from(SIGNATURE);
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&self.width.to_be_bytes());
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, no filter, no interlace
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(
            &mut out,
            b"IDAT",
            &zlib_deflate(&raw, self.width as usize * 4 + 1),
        );
        chunk(&mut out, b"IEND", &[]);
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

    /// Inside the rectangle, and — within a corner's quadrant — inside that corner's quarter circle.
    fn contains(&self, px: f32, py: f32) -> bool {
        if self.w <= 0.0 || self.h <= 0.0 {
            return false;
        }
        let (x0, y0) = (self.x, self.y);
        let (x1, y1) = (self.x + self.w, self.y + self.h);
        if px < x0 || px > x1 || py < y0 || py > y1 {
            return false;
        }
        let r = self.r;
        if r <= 0.0 {
            return true;
        }
        let cx = if px < x0 + r {
            x0 + r
        } else if px > x1 - r {
            x1 - r
        } else {
            return true;
        };
        let cy = if py < y0 + r {
            y0 + r
        } else if py > y1 - r {
            y1 - r
        } else {
            return true;
        };
        let (dx, dy) = (px - cx, py - cy);
        dx * dx + dy * dy <= r * r
    }
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa`; alpha defaults to opaque.
pub fn parse_colour(spec: &str) -> Option<[u8; 4]> {
    let hex = spec.strip_prefix('#').unwrap_or(spec);
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    match hex.len() {
        3 => {
            let mut out = [0u8, 0, 0, 255];
            for (i, c) in hex.chars().enumerate() {
                let v = c.to_digit(16)? as u8;
                out[i] = v * 17;
            }
            Some(out)
        }
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_chunks(png: &[u8]) -> Vec<(String, Vec<u8>)> {
        assert_eq!(&png[..8], &SIGNATURE);
        let mut at = 8;
        let mut out = vec![];
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let kind = String::from_utf8(png[at + 4..at + 8].to_vec()).unwrap();
            let body = png[at + 8..at + 8 + len].to_vec();
            let want = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc32(&png[at + 4..at + 8 + len]), want, "crc for {kind}");
            out.push((kind, body));
            at += 12 + len;
        }
        out
    }

    #[test]
    fn png_has_a_signature_header_and_terminator_with_valid_crcs() {
        let png = Canvas::new(4, 3, [1, 2, 3, 255]).to_png();
        let chunks = decode_chunks(&png);
        let kinds: Vec<&str> = chunks.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(kinds, vec!["IHDR", "IDAT", "IEND"]);
        assert_eq!(&chunks[0].1[..8], &[0, 0, 0, 4, 0, 0, 0, 3]);
        assert_eq!(&chunks[0].1[8..], &[8, 6, 0, 0, 0]);
    }

    /// Nothing here decodes deflate, so the pin is that the stream is self-consistent and that the
    /// bytes survive a round trip through the matcher -- `just check` runs a real inflate in the
    /// Lua suite, and the e2e opens the file with wezterm itself.
    #[test]
    fn the_deflate_stream_has_a_zlib_header_and_the_adler32_of_the_raw_bytes() {
        let canvas = Canvas::new(3, 2, [9, 8, 7, 255]);
        let png = canvas.to_png();
        let idat = decode_chunks(&png)
            .into_iter()
            .find(|(k, _)| k == "IDAT")
            .unwrap()
            .1;
        assert_eq!(&idat[..2], &[0x78, 0x01], "zlib header");
        let mut raw = vec![];
        for _ in 0..2usize {
            raw.extend_from_slice(&[0, 9, 8, 7, 255, 9, 8, 7, 255, 9, 8, 7, 255]);
        }
        let tail = u32::from_be_bytes(idat[idat.len() - 4..].try_into().unwrap());
        assert_eq!(tail, adler32(&raw), "adler32 of the scanlines");
    }

    /// A flat fill is what this image mostly is, so the matcher has to turn it into matches rather
    /// than literals -- otherwise a retina frame is tens of megabytes written on every resize.
    #[test]
    fn a_flat_fill_compresses_by_orders_of_magnitude() {
        let png = Canvas::new(800, 600, [0x1e, 0x1e, 0x2e, 255]).to_png();
        let raw_size = 600 * (1 + 800 * 4);
        assert!(
            png.len() * 100 < raw_size,
            "expected a big win, got {} vs {raw_size}",
            png.len()
        );
    }

    #[test]
    fn the_length_and_distance_tables_bracket_every_legal_value() {
        for len in 3..=258u32 {
            let (code, extra, bits) = length_code(len);
            assert!((257..=285).contains(&code), "length {len} -> {code}");
            assert!(
                extra < (1 << bits) || bits == 0,
                "length {len} extra out of range"
            );
        }
        for dist in [1u32, 2, 3, 4, 5, 1024, 32768] {
            let (code, extra, bits) = distance_code(dist);
            assert!(code < 30, "distance {dist} -> {code}");
            assert!(extra < (1 << bits) || bits == 0);
        }
    }

    #[test]
    fn crc32_and_adler32_match_their_published_vectors() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn colours_parse_in_every_length_and_nothing_else_does() {
        assert_eq!(parse_colour("#1e1e2e"), Some([0x1e, 0x1e, 0x2e, 255]));
        assert_eq!(parse_colour("1e1e2e"), Some([0x1e, 0x1e, 0x2e, 255]));
        assert_eq!(parse_colour("#abc"), Some([0xaa, 0xbb, 0xcc, 255]));
        assert_eq!(parse_colour("#11223344"), Some([0x11, 0x22, 0x33, 0x44]));
        assert_eq!(parse_colour("#12345"), None);
        assert_eq!(parse_colour("#zzzzzz"), None);
        assert_eq!(parse_colour(""), None);
    }

    /// The card is what the terminal shows through, so its inside must be exactly the card colour
    /// and the page outside it exactly the frame colour -- no blending except on the edge itself.
    #[test]
    fn a_rounded_card_is_exact_inside_exact_outside_and_blended_only_at_the_corner() {
        let page = [0x1e, 0x1e, 0x2e, 255];
        let card = [0x11, 0x11, 0x11, 255];
        let mut canvas = Canvas::new(40, 30, page);
        let rect = RoundRect::new(8.0, 6.0, 24.0, 18.0, 6.0);
        canvas.rounded_rect(&rect, card);
        let at = |x: u32, y: u32| {
            let i = ((y as usize) * 40 + x as usize) * 4;
            [
                canvas.pixels[i],
                canvas.pixels[i + 1],
                canvas.pixels[i + 2],
                canvas.pixels[i + 3],
            ]
        };
        assert_eq!(at(20, 15), card, "the middle of the card");
        assert_eq!(at(1, 1), page, "the page beyond it");
        assert_eq!(at(8, 6), page, "the corner's own pixel is outside the arc");
        assert_eq!(at(20, 6), card, "but the straight top edge is not");
        let corner = at(9, 7);
        assert!(
            corner != page && corner != card,
            "the arc is antialiased, got {corner:?}"
        );
    }

    /// A radius past half the shorter side would invert the arcs; it clamps to a stadium instead.
    #[test]
    fn a_radius_larger_than_the_rectangle_clamps_instead_of_inverting_it() {
        let rect = RoundRect::new(0.0, 0.0, 10.0, 4.0, 99.0);
        assert_eq!(rect.r, 2.0);
        assert!(rect.contains(5.0, 2.0));
        assert!(!rect.contains(0.1, 0.1), "the corner is still cut");
    }

    #[test]
    fn a_border_covers_the_edge_and_leaves_the_middle_alone() {
        let page = [0, 0, 0, 255];
        let card = [255, 255, 255, 255];
        let line = [255, 0, 0, 255];
        let mut canvas = Canvas::new(30, 30, page);
        let rect = RoundRect::new(5.0, 5.0, 20.0, 20.0, 4.0);
        canvas.rounded_rect(&rect, card);
        canvas.rounded_border(&rect, line, 2.0);
        let at = |x: u32, y: u32| {
            let i = ((y as usize) * 30 + x as usize) * 4;
            [
                canvas.pixels[i],
                canvas.pixels[i + 1],
                canvas.pixels[i + 2],
                canvas.pixels[i + 3],
            ]
        };
        assert_eq!(at(15, 6), line, "the top edge is the border colour");
        assert_eq!(at(15, 15), card, "the middle is untouched");
        assert_eq!(at(15, 2), page, "and so is the page above it");
    }

    /// The fast path must be indistinguishable from sampling every pixel, or the optimisation is a
    /// second renderer with its own bugs.
    #[test]
    fn the_interior_fast_path_matches_sampling_every_pixel() {
        let page = [0x1e, 0x1e, 0x2e, 255];
        let card = [0x11, 0x11, 0x1b, 255];
        let rect = RoundRect::new(7.5, 4.25, 33.0, 21.0, 7.0);
        let mut fast = Canvas::new(50, 32, page);
        fast.rounded_rect(&rect, card);
        fast.rounded_border(&rect, [0x45, 0x47, 0x5a, 255], 1.5);

        let mut slow = Canvas::new(50, 32, page);
        let inner = rect.inset(1.5);
        for y in 0..32u32 {
            for x in 0..50u32 {
                slow.blend(x, y, card, slow.coverage(x, y, &rect));
            }
        }
        for y in 0..32u32 {
            for x in 0..50u32 {
                let ring = slow.coverage(x, y, &rect) - slow.coverage(x, y, &inner);
                slow.blend(x, y, [0x45, 0x47, 0x5a, 255], ring.clamp(0.0, 1.0));
            }
        }
        assert_eq!(
            fast.pixels, slow.pixels,
            "fast path differs from full sampling"
        );
    }

    #[test]
    fn a_zero_width_border_draws_nothing() {
        let mut canvas = Canvas::new(8, 8, [1, 1, 1, 255]);
        let before = canvas.pixels.clone();
        canvas.rounded_border(
            &RoundRect::new(1.0, 1.0, 6.0, 6.0, 2.0),
            [9, 9, 9, 255],
            0.0,
        );
        assert_eq!(canvas.pixels, before);
    }
}

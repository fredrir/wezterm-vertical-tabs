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
    assert!(rect.distance(5.0, 2.0) < 0.0);
    assert!(rect.distance(0.1, 0.1) > 0.0, "the corner is still cut");
}

fn pixel(canvas: &Canvas, x: u32, y: u32) -> [u8; 4] {
    canvas.get(x, y)
}

/// Two passes blended the card under the stroke first, so the outer edge pixel carried the
/// card's colour outside the line; one pass mixes page and stroke alone there.
#[test]
fn a_bordered_card_has_no_fill_halo_outside_the_stroke() {
    let page = [0, 0, 0, 255];
    let card = [255, 255, 255, 255];
    let line = [255, 0, 0, 255];
    let mut canvas = Canvas::new(30, 30, page);
    // half-pixel offset: the outer edge splits pixel column 5 exactly in two
    let rect = RoundRect::new(5.5, 5.5, 20.0, 20.0, 0.0);
    canvas.rounded_card(&rect, card, Some((line, 1.0)));
    assert_eq!(
        pixel(&canvas, 5, 15),
        [128, 0, 0, 255],
        "page and stroke only"
    );
    assert_eq!(
        pixel(&canvas, 6, 15),
        [255, 128, 128, 255],
        "stroke and card only"
    );
    assert_eq!(pixel(&canvas, 15, 15), card);
    assert_eq!(pixel(&canvas, 3, 15), page);
}

#[test]
fn a_half_pixel_border_is_a_uniform_hairline() {
    let page = [0, 0, 0, 255];
    let card = [255, 255, 255, 255];
    let line = [255, 0, 0, 255];
    let mut canvas = Canvas::new(30, 30, page);
    let rect = RoundRect::new(5.0, 5.0, 20.0, 20.0, 4.0);
    canvas.rounded_card(&rect, card, Some((line, 0.5)));
    let top: Vec<[u8; 4]> = (10..20).map(|x| pixel(&canvas, x, 5)).collect();
    let left: Vec<[u8; 4]> = (10..20).map(|y| pixel(&canvas, 5, y)).collect();
    assert!(
        top.iter().all(|p| *p == top[0]),
        "one value along the top edge"
    );
    assert_eq!(top, left, "and the same value down the left edge");
    assert_eq!(top[0], [255, 128, 128, 255], "half stroke, half card");
}

#[test]
fn the_one_pass_card_matches_the_two_pass_one_away_from_the_stroke() {
    let page = [0x1e, 0x1e, 0x2e, 255];
    let card = [0x11, 0x11, 0x1b, 255];
    let line = [0x45, 0x47, 0x5a, 255];
    let rect = RoundRect::new(7.5, 4.25, 33.0, 21.0, 7.0);
    let mut one = Canvas::new(50, 32, page);
    one.rounded_card(&rect, card, Some((line, 1.5)));
    let mut two = Canvas::new(50, 32, page);
    two.rounded_rect(&rect, card);
    two.rounded_border(&rect, line, 1.5);
    assert_eq!(pixel(&one, 25, 15), pixel(&two, 25, 15), "the middle");
    assert_eq!(pixel(&one, 1, 1), pixel(&two, 1, 1), "the page");
    assert_eq!(
        pixel(&one, 25, 5),
        pixel(&two, 25, 5),
        "the stroke's own pixel"
    );
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

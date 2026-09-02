use std::io::Cursor;

use super::*;

#[test]
fn png_round_trips_as_rgba8() {
    let pixel = [1, 2, 3, 255];
    let png = Canvas::new(4, 3, pixel).to_png();
    let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
    let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut decoded).unwrap();
    assert_eq!((info.width, info.height), (4, 3));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    assert_eq!(&decoded[..info.buffer_size()], pixel.repeat(12));
}

/// A flat fill is what this image mostly is, so the encoder must compress it rather than writing
/// literal pixels -- otherwise a retina frame is tens of megabytes on every resize.
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
fn colours_parse_in_every_length_and_nothing_else_does() {
    assert_eq!(parse_colour("#1e1e2e"), Some([0x1e, 0x1e, 0x2e, 255]));
    assert_eq!(parse_colour("1e1e2e"), Some([0x1e, 0x1e, 0x2e, 255]));
    assert_eq!(parse_colour("#abc"), Some([0xaa, 0xbb, 0xcc, 255]));
    assert_eq!(parse_colour("#11223344"), Some([0x11, 0x22, 0x33, 0x44]));
    assert_eq!(parse_colour("#12345"), None);
    assert_eq!(parse_colour("#zzzzzz"), None);
    assert_eq!(
        parse_colour("a……"),
        None,
        "six UTF-8 bytes never become slice offsets"
    );
    assert_eq!(
        parse_colour("éééé"),
        None,
        "eight UTF-8 bytes are still not hex"
    );
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

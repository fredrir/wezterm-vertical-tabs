//! UI icons use their ink bounds; text retains the font baseline.
pub(super) fn top(
    glyph: Option<char>,
    baseline_top: f32,
    cell_height: f32,
    glyph_height: f32,
) -> f32 {
    match glyph {
        Some('\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}') => {
            (cell_height - glyph_height) / 2.
        }
        _ => baseline_top,
    }
}

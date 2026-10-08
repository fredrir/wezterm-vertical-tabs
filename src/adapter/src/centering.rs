//! Text in a two-row shape sits half a cell lower; overlays keep their own text in place.
use crate::termwindow::ui_host::Bounds;

/// Applies one shape, in paint order, to each row's centered pixel spans.
pub fn add(rows: &mut [Vec<(f32, f32)>], bounds: Bounds, stacked: bool, cell_width: f32) {
    if let Some((row, range)) = (!stacked).then(|| centered(bounds, cell_width)).flatten() {
        if let Some(ranges) = rows.get_mut(row) {
            ranges.push(range);
        }
    } else if let Some((covered_rows, covered)) = cover(bounds, stacked, cell_width) {
        for row in covered_rows {
            if let Some(ranges) = rows.get_mut(row) {
                occlude(ranges, covered);
            }
        }
    }
}

fn centered(bounds: Bounds, cell_width: f32) -> Option<(usize, (f32, f32))> {
    if bounds.height != 2. || bounds.y < 0. || bounds.y.fract() != 0. {
        return None;
    }
    Some((
        bounds.y as usize,
        (
            bounds.x * cell_width,
            (bounds.x + bounds.width) * cell_width,
        ),
    ))
}

/// Menus, forms and long hints cover the rows beneath; chips within a row do not.
fn cover(
    bounds: Bounds,
    stacked: bool,
    cell_width: f32,
) -> Option<(std::ops::Range<usize>, (f32, f32))> {
    if (bounds.height <= 2. && !stacked) || bounds.y < 0. || bounds.y.fract() != 0. {
        return None;
    }
    Some((
        bounds.y as usize..(bounds.y + bounds.height).ceil() as usize,
        (
            bounds.x * cell_width,
            (bounds.x + bounds.width) * cell_width,
        ),
    ))
}

/// Text under an overlay belongs to the overlay, not to the centered row it covers.
fn occlude(ranges: &mut Vec<(f32, f32)>, (left, right): (f32, f32)) {
    let mut kept = Vec::with_capacity(ranges.len() + 1);
    for &(start, end) in ranges.iter() {
        if right <= start || left >= end {
            kept.push((start, end));
            continue;
        }
        if start < left {
            kept.push((start, left));
        }
        if right < end {
            kept.push((right, end));
        }
    }
    *ranges = kept;
}

//! Colour-only fades over the frame the pane already shows (08-p7-spec.md): the phase table from
//! anim.lua, and the interpolation anim.rs used to do by rewriting SGR bytes.

use crate::frame::Cell;

type Rgb = [u8; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phase {
    pub ms: u64,
    pub ease: &'static str,
    /// `dir = "out"`: the frame fades toward the anchor instead of rising out of it.
    pub out: bool,
    pub stagger: u64,
    pub cap: u64,
    /// Collapse runs bottom-to-top, expand top-to-bottom.
    pub reverse: bool,
}

const fn phase(
    ms: u64,
    ease: &'static str,
    out: bool,
    stagger: u64,
    cap: u64,
    reverse: bool,
) -> Phase {
    Phase {
        ms,
        ease,
        out,
        stagger,
        cap,
        reverse,
    }
}

pub fn phase_named(name: &str) -> Option<Phase> {
    Some(match name {
        "collapse_out" => phase(160, "inOutQuad", true, 8, 80, true),
        "collapse_in" => phase(100, "outCubic", false, 8, 80, true),
        "expand_out" => phase(80, "inOutQuad", true, 12, 120, false),
        "expand_in" => phase(220, "outCubic", false, 12, 120, false),
        "hover" => phase(60, "linear", false, 0, 0, false),
        // The menu rises from the sidebar surface all at once; a staggered menu reads as a wipe.
        "popover_in" => phase(90, "outCubic", false, 0, 0, false),
        _ => return None,
    })
}

/// `linear` | `outCubic` | `inOutQuad`; anything else is linear.
pub fn ease(name: &str, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match name {
        "outCubic" => 1.0 - (1.0 - t).powi(3),
        "inOutQuad" => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }
        _ => t,
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn mix(anchor: Rgb, colour: Rgb, t: f32) -> Rgb {
    [
        lerp(anchor[0], colour[0], t),
        lerp(anchor[1], colour[1], t),
        lerp(anchor[2], colour[2], t),
    ]
}

/// Per-row start delays: `min(stagger × nth, cap)`, nth counted bottom-up when the phase reverses.
pub fn delays(phase: &Phase, animated: &[usize]) -> Vec<u64> {
    let n = animated.len();
    (0..n)
        .map(|i| {
            let nth = if phase.reverse { n - 1 - i } else { i } as u64;
            (phase.stagger * nth).min(phase.cap)
        })
        .collect()
}

/// The whole run's length: the last row's delay plus the phase.
pub fn total_ms(phase: &Phase, delays: &[u64]) -> u64 {
    delays.iter().copied().max().unwrap_or(0) + phase.ms
}

/// The frame at `elapsed_ms`: every animated cell's fg and bg between the anchor and its final
/// colour. Nothing but colour changes — glyphs, positions and bold are the final frame's.
pub fn frame_at(
    final_rows: &[Option<Vec<Cell>>],
    animated: &[usize],
    delays: &[u64],
    phase: &Phase,
    anchor: Rgb,
    elapsed_ms: u64,
) -> Vec<Option<Vec<Cell>>> {
    let mut out = final_rows.to_vec();
    for (k, &row) in animated.iter().enumerate() {
        let Some(cells) = out.get_mut(row).and_then(Option::as_mut) else {
            continue;
        };
        let local = elapsed_ms.saturating_sub(delays[k]) as f32 / phase.ms.max(1) as f32;
        let t = ease(phase.ease, local);
        let t = if phase.out { 1.0 - t } else { t };
        for cell in cells.iter_mut() {
            cell.fg = mix(anchor, cell.fg, t);
            cell.bg = mix(anchor, cell.bg, t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_matches_the_v1_curves() {
        assert_eq!(ease("linear", 0.25), 0.25);
        assert!((ease("outCubic", 0.5) - 0.875).abs() < 1e-6);
        assert!((ease("inOutQuad", 0.25) - 0.125).abs() < 1e-6);
        assert!((ease("inOutQuad", 0.75) - 0.875).abs() < 1e-6);
        assert_eq!(ease("nope", 0.3), 0.3);
    }

    #[test]
    fn collapse_staggers_bottom_up_and_caps() {
        let p = phase_named("collapse_in").unwrap();
        assert_eq!(delays(&p, &[0, 1, 2, 3]), vec![24, 16, 8, 0]);
        let e = phase_named("expand_in").unwrap();
        assert_eq!(delays(&e, &(0..20).collect::<Vec<_>>())[15], 120, "capped");
    }

    #[test]
    fn a_fade_changes_colour_only() {
        let red = [200, 40, 40];
        let bg = [30, 30, 46];
        let row = vec![Cell::new(Some('h'), red, bg), Cell::new(None, red, bg)];
        let rows = vec![Some(row.clone()), None];
        let p = phase_named("expand_in").unwrap();
        let mid = frame_at(&rows, &[0], &[0], &p, bg, 110);
        let cells = mid[0].as_ref().unwrap();
        assert_eq!(cells[0].ch, Some('h'));
        assert_eq!(cells[1].ch, None);
        assert_ne!(cells[0].fg, red, "mid-fade is between anchor and final");
        let end = frame_at(&rows, &[0], &[0], &p, bg, 220);
        assert_eq!(
            end[0].as_ref().unwrap()[0].fg,
            red,
            "the run lands on the final colour"
        );
        let start = frame_at(&rows, &[0], &[0], &p, bg, 0);
        assert_eq!(
            start[0].as_ref().unwrap()[0].fg,
            bg,
            "and starts at the anchor"
        );
        let out = phase_named("expand_out").unwrap();
        assert_eq!(
            frame_at(&rows, &[0], &[0], &out, bg, 0)[0]
                .as_ref()
                .unwrap()[0]
                .fg,
            red
        );
    }
}

use crate::color::Color;

pub use vtabs_protocol::v2::{
    ResolvedTheme as Theme, Scheme as Palette, ThemeOverrides as UserTheme,
};

const ACCENT_MIN: f64 = 3.0;
const ACCENT_VS_FG_MIN: f64 = 1.2;
const QUIET_TITLE_MIN: f64 = 5.0;
const DEFAULT_ELEVATION: f64 = 0.06;

const WHITE: Color = [255, 255, 255];
const BLACK: Color = [0, 0, 0];

/// The protocol's colour-valued theme surface, declared once for overlaying, validating and
/// accepting dynamic-space overrides. Keep the DTO declaration itself in `vtabs-protocol`; all
/// behavioural field walks expand this list.
macro_rules! theme_color_fields {
    ($visit:ident) => {
        $visit!(
            fg,
            bg,
            accent,
            private_accent,
            hover_bg,
            hover_fg,
            active_bg,
            active_fg,
            focus_bg,
            meta_fg,
            dim,
            title_idle,
            title_active,
            active_title_fg,
            pinned_fg,
            separator,
            border,
            border_idle,
            ghost_border_hover,
            new_tab_fg,
            close_fg,
            close_hover_fg,
            unseen_fg,
            drag_bg,
            drag_fg,
            scroll_fg,
            scroll_idle_fg,
            surface_raised,
            disabled_fg,
            popover_sel_bg,
            popover_sel_fg,
            popover_sel_hint,
        )
    };
}

macro_rules! theme_fraction_fields {
    ($visit:ident) => {
        $visit!(elevation, scrim)
    };
}

pub(crate) use theme_color_fields;
pub(crate) use theme_fraction_fields;

pub(crate) fn is_override_field(name: &str) -> bool {
    macro_rules! is_one_of {
        ($($field:ident),+ $(,)?) => {
            matches!(name, $(stringify!($field))|+)
        };
    }
    theme_color_fields!(is_one_of) || theme_fraction_fields!(is_one_of)
}

pub fn parse(color: &str) -> Option<Color> {
    let hex = color.strip_prefix('#')?;
    let nibble = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let pair = |hi: u8, lo: u8| Some(nibble(hi)? * 16 + nibble(lo)?);
    match hex.as_bytes() {
        [r, g, b] => Some([nibble(*r)? * 17, nibble(*g)? * 17, nibble(*b)? * 17]),
        [r1, r2, g1, g2, b1, b2] => Some([pair(*r1, *r2)?, pair(*g1, *g2)?, pair(*b1, *b2)?]),
        _ => None,
    }
}

fn first(candidates: &[Option<&str>]) -> Option<Color> {
    candidates.iter().flatten().find_map(|c| parse(c))
}

pub fn mix(a: Color, b: Color, t: f64) -> Color {
    let mut out = [0u8; 3];
    for i in 0..3 {
        out[i] = (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * t + 0.5).floor() as u8;
    }
    out
}

pub fn luminance(rgb: Color) -> f64 {
    fn channel(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

pub fn contrast(a: Color, b: Color) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Recesses the sidebar below the terminal surface. A proportional step toward black is hard to see
/// on an already-dark palette, so dark schemes amplify the same public elevation control.
fn recess(base: Color, elevation: f64) -> Color {
    let scale = if luminance(base) < 0.5 { 4.0 } else { 1.0 };
    mix(base, BLACK, (elevation * scale).clamp(0.0, 1.0))
}

/// Pushes `fg` toward `target` until it clears `min` against `ref`; never mixes past `target`.
fn ensure_contrast(fg: Color, reference: Color, target: Color, min: f64) -> Color {
    let min = min.min(contrast(target, reference));
    let mut out = fg;
    for i in 1..=10 {
        if contrast(out, reference) >= min {
            return out;
        }
        out = mix(fg, target, f64::from(i) / 10.0);
    }
    out
}

/// The popover surface, lowered in 0.01 steps until its text is no harder to read than the body.
fn raise(bg: Color, fg: Color, lift: impl Fn(f64) -> Color) -> Color {
    let ceiling = 4.5f64.min(0.95 * contrast(fg, bg));
    let mut out = lift(0.09);
    for step in (1..=9).rev() {
        out = lift(f64::from(step) / 100.0);
        if contrast(fg, out) >= ceiling {
            return out;
        }
    }
    out
}

/// Smallest fade that pushes the scrimmed layer under 2.6 against the page; a target, not a constant.
fn scrim_for(bg: Color, fg: Color) -> f64 {
    let mut pct = 30;
    while pct < 70 && contrast(mix(fg, bg, f64::from(pct) / 100.0), bg) > 2.6 {
        pct += 5;
    }
    f64::from(pct) / 100.0
}

fn accent_candidate(color: Option<&str>, bg: Color, fg: Color) -> Option<Color> {
    let rgb = first(&[color])?;
    (contrast(rgb, bg) >= ACCENT_MIN && contrast(rgb, fg) >= ACCENT_VS_FG_MIN).then_some(rgb)
}

fn ansi(palette: &Palette, one_based: usize) -> Option<&str> {
    palette.ansi.get(one_based - 1).map(String::as_str)
}

fn s(o: &Option<String>) -> Option<&str> {
    o.as_deref()
}

pub fn resolve(user: &UserTheme, palette: &Palette, private: bool) -> Theme {
    let base_bg = first(&[s(&palette.background), Some("#1e1e2e")]).unwrap();
    let fg = first(&[s(&user.fg), s(&palette.foreground), Some("#cdd6f4")]).unwrap();
    let bg = first(&[s(&user.bg)])
        .unwrap_or_else(|| recess(base_bg, user.elevation.unwrap_or(DEFAULT_ELEVATION)));

    // A 6% darken on a light scheme reads far louder than a 6% lighten on near-black.
    let k = if luminance(bg) < 0.5 { 1.0 } else { 0.6 };
    let lift = |t: f64| mix(bg, fg, t * k);

    let private_accent =
        first(&[s(&user.private_accent), ansi(palette, 6), Some("#cba6f7")]).unwrap();
    let mut accent = first(&[s(&user.accent)])
        .or_else(|| accent_candidate(s(&palette.cursor_bg), bg, fg))
        .or_else(|| accent_candidate(s(&palette.active_tab_bg), bg, fg))
        .unwrap_or_else(|| first(&[ansi(palette, 5), Some("#89b4fa")]).unwrap());
    if private {
        accent = private_accent;
    }
    let accent = ensure_contrast(accent, bg, fg, ACCENT_MIN);

    let hover_bg = first(&[s(&user.hover_bg)]).unwrap_or_else(|| lift(0.06));
    let active_bg = first(&[s(&user.active_bg)]).unwrap_or_else(|| mix(lift(0.12), accent, 0.12));
    let meta_fg = first(&[s(&user.meta_fg)])
        .unwrap_or_else(|| ensure_contrast(mix(fg, bg, 0.48), active_bg, fg, 3.5));
    let title_active = first(&[s(&user.title_active), s(&user.active_title_fg)])
        .unwrap_or_else(|| ensure_contrast(accent, active_bg, fg, 4.5));
    let raised = first(&[s(&user.surface_raised)]).unwrap_or_else(|| raise(bg, fg, lift));
    let scrim = user.scrim.unwrap_or_else(|| scrim_for(bg, fg));
    let scroll_fg =
        first(&[s(&user.scroll_fg)]).unwrap_or_else(|| ensure_contrast(lift(0.22), bg, fg, 2.0));
    let border =
        first(&[s(&user.border)]).unwrap_or_else(|| ensure_contrast(lift(0.18), bg, fg, 2.5));
    let border_idle =
        first(&[s(&user.border_idle)]).unwrap_or_else(|| ensure_contrast(lift(0.14), bg, fg, 2.0));
    let unseen = first(&[ansi(palette, 4)]);

    // A saturated fill is the one selection construction that clears 4.5 on every scheme; the ink is
    // pure black or white, a deliberate exception to the no-absolutes rule, confined to that one row.
    let sel_bg = first(&[s(&user.popover_sel_bg)]).unwrap_or(accent);
    let sel_fg = first(&[s(&user.popover_sel_fg)]).unwrap_or_else(|| {
        if contrast(WHITE, sel_bg) >= contrast(BLACK, sel_bg) {
            WHITE
        } else {
            BLACK
        }
    });

    Theme {
        bg,
        fg,
        dim: first(&[s(&user.dim)]).unwrap_or(meta_fg),
        accent,
        title_idle: first(&[s(&user.title_idle)]).unwrap_or_else(|| {
            if contrast(fg, bg) >= QUIET_TITLE_MIN {
                mix(fg, bg, 0.12)
            } else {
                fg
            }
        }),
        meta_fg,
        active_bg,
        active_fg: first(&[s(&user.active_fg)]).unwrap_or(fg),
        hover_bg,
        hover_fg: first(&[s(&user.hover_fg)]).unwrap_or(fg),
        focus_bg: first(&[s(&user.focus_bg)]).unwrap_or_else(|| mix(bg, accent, 0.25)),
        pinned_fg: first(&[s(&user.pinned_fg)]).unwrap_or(meta_fg),
        separator: first(&[s(&user.separator)]).unwrap_or_else(|| lift(0.10)),
        border,
        border_idle,
        // a luminance-only step off border_idle is invisible on a 1-cell stroke, so the hover takes hue
        ghost_border_hover: first(&[s(&user.ghost_border_hover)])
            .unwrap_or_else(|| ensure_contrast(mix(border, accent, 0.5), bg, accent, 2.8)),
        new_tab_fg: first(&[s(&user.new_tab_fg)]).unwrap_or_else(|| mix(fg, bg, 0.30)),
        close_fg: first(&[s(&user.close_fg)])
            .unwrap_or_else(|| ensure_contrast(mix(fg, bg, 0.55), active_bg, fg, 3.0)),
        close_hover_fg: ensure_contrast(
            first(&[s(&user.close_hover_fg), ansi(palette, 2), Some("#f38ba8")]).unwrap(),
            active_bg,
            fg,
            3.0,
        ),
        unseen_fg: first(&[s(&user.unseen_fg)])
            .or_else(|| unseen.filter(|&u| contrast(u, bg) >= ACCENT_MIN))
            .unwrap_or(accent),
        private_accent,
        drag_bg: first(&[s(&user.drag_bg)]).unwrap_or_else(|| mix(bg, accent, 0.35)),
        drag_fg: first(&[s(&user.drag_fg)]).unwrap_or(fg),
        scroll_fg,
        scroll_idle_fg: first(&[s(&user.scroll_idle_fg)])
            .unwrap_or_else(|| mix(scroll_fg, bg, 0.55)),
        title_active,
        active_title_fg: title_active,
        // Render draws the accent bar only when the tinted title is not distinct enough on its own.
        title_active_contrast: contrast(title_active, active_bg),
        content_bg: base_bg,
        surface_raised: raised,
        scrim,
        disabled_fg: first(&[s(&user.disabled_fg)]).unwrap_or_else(|| mix(meta_fg, raised, 0.45)),
        popover_sel_bg: sel_bg,
        popover_sel_fg: sel_fg,
        popover_sel_hint: first(&[s(&user.popover_sel_hint)])
            .unwrap_or_else(|| ensure_contrast(mix(sel_fg, sel_bg, 0.40), sel_bg, sel_fg, 3.0)),
    }
}

/// Applies a hook's partial answer over the raw user/space theme before the single resolver runs.
pub fn overlay(base: &UserTheme, hook: &UserTheme) -> UserTheme {
    let mut merged = base.clone();
    macro_rules! replace {
        ($($field:ident),+ $(,)?) => {
            $(if hook.$field.is_some() {
                merged.$field = hook.$field.clone();
            })+
        };
    }
    theme_color_fields!(replace);
    theme_fraction_fields!(replace);
    merged
}

/// Hook results are executable-extension output, not forgiving user configuration: reject a whole
/// generation rather than silently substituting a fallback for a malformed colour or fraction.
pub fn valid_overrides(user: &UserTheme) -> bool {
    macro_rules! valid_colors {
        ($($field:ident),+ $(,)?) => {
            [$(user.$field.as_deref()),+]
                .into_iter()
                .flatten()
                .all(|color| parse(color).is_some())
        };
    }
    macro_rules! valid_fractions {
        ($($field:ident),+ $(,)?) => {
            [$(user.$field),+]
                .into_iter()
                .flatten()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        };
    }
    theme_color_fields!(valid_colors) && theme_fraction_fields!(valid_fractions)
}

#[cfg(test)]
mod field_manifest_tests {
    #[test]
    fn behavioural_field_walks_match_the_protocol_manifest() {
        macro_rules! names {
            ($($field:ident),+ $(,)?) => {
                &[$(stringify!($field)),+]
            };
        }
        assert_eq!(
            theme_color_fields!(names),
            vtabs_protocol::v2::THEME_COLOR_FIELDS
        );
        assert_eq!(
            theme_fraction_fields!(names),
            vtabs_protocol::v2::THEME_FRACTION_FIELDS
        );
    }
}

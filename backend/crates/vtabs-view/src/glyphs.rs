use std::collections::BTreeMap;

use crate::text::width;

/// name, glyph, ASCII fallback, substitution group
const CHROME: &[(&str, &str, &str, &str)] = &[
    ("chamfer_top", "▙", " ", "block"),
    ("chamfer_bottom", "▛", " ", "block"),
    ("scroll", "▐", "|", "block"),
    ("active", "▎", "|", "bar"),
    ("unseen", "•", "*", "marks"),
    ("focus", "›", ">", "marks"),
    ("ellipsis", "…", "...", "marks"),
    ("meta_sep", "  ", "  ", "marks"),
    ("toggle_left", "❮", "<", "toggle"),
    ("toggle_right", "❯", ">", "toggle"),
    ("frame_tl", "╭", "+", "ghost_frame"),
    ("frame_tr", "╮", "+", "ghost_frame"),
    ("frame_bl", "╰", "+", "ghost_frame"),
    ("frame_br", "╯", "+", "ghost_frame"),
    ("frame_dash", "╌", "-", "ghost_frame"),
    ("frame_dash_v", "╎", "|", "ghost_frame"),
    ("rule", "─", "-", "marks"),
    ("hint_up", "↑", "^", "hints"),
    ("hint_down", "↓", "v", "hints"),
    ("hint_left", "←", "<", "hints"),
    ("hint_right", "→", ">", "hints"),
];

/// East Asian Ambiguous: width flips with unicode_version / treat_east_asian_ambiguous_width_as_wide.
const AMBIGUOUS: &[char] = &[
    '\u{258e}', '\u{2022}', '\u{2026}', '\u{00b7}', '\u{256d}', '\u{256e}', '\u{256f}', '\u{2570}',
    '\u{2500}', '\u{2190}', '\u{2191}', '\u{2192}', '\u{2193}',
];

/// Icon keys with a hard ASCII backstop when their configured glyph is not one cell wide.
const ICON_BACKSTOP: &[(&str, &str)] = &[
    ("close", "x"),
    ("pinned", "*"),
    ("private", "~"),
    ("new_tab", "+"),
    ("strip_new_tab", "+"),
    ("settings", "*"),
    ("search", "/"),
];

fn chrome(key: &str) -> Option<(&'static str, &'static str, &'static str)> {
    CHROME
        .iter()
        .find(|(name, ..)| *name == key)
        .map(|&(_, glyph, fallback, group)| (glyph, fallback, group))
}

fn ambiguous(s: &str) -> bool {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => AMBIGUOUS.contains(&c),
        _ => false,
    }
}

pub struct Resolved {
    pub glyphs: BTreeMap<String, String>,
    pub corners: &'static str,
    /// True when a glyph had to fall back over width; Lua warns once on it.
    pub narrowed: bool,
}

/// Resolves the chrome glyphs against the window's effective config; `base` is icons::resolve
/// output, whose process icons pass through untouched.
pub fn resolve(
    base: &BTreeMap<String, String>,
    custom_block_glyphs: bool,
    wide_ambiguous: bool,
) -> Resolved {
    let mut out = base.clone();
    for &(key, glyph, ..) in CHROME {
        out.entry(key.to_string())
            .or_insert_with(|| glyph.to_string());
    }
    let mut corners = "chamfer";
    let mut narrowed = false;

    let fall_back = |out: &mut BTreeMap<String, String>, key: &str| {
        if let Some((_, fallback, _)) = chrome(key)
            && out[key] != fallback
        {
            out.insert(key.to_string(), fallback.to_string());
        }
    };
    let fall_back_group = |out: &mut BTreeMap<String, String>, group: &str| {
        for &(key, _, fallback, g) in CHROME {
            if g == group && out[key] != fallback {
                out.insert(key.to_string(), fallback.to_string());
            }
        }
    };

    if !custom_block_glyphs {
        corners = "square";
        fall_back_group(&mut out, "block");
        fall_back_group(&mut out, "bar");
    }

    // only this flag selects ambiguous width; unicode_version alone never does
    if wide_ambiguous {
        for &(key, ..) in CHROME {
            if ambiguous(&out[key]) {
                fall_back(&mut out, key);
            }
        }
        fall_back_group(&mut out, "ghost_frame");
    }

    for &(key, ..) in CHROME {
        if width(&out[key]) != 1 {
            fall_back(&mut out, key);
            narrowed = true;
        }
    }
    let ghost_intact = CHROME
        .iter()
        .filter(|(.., g)| *g == "ghost_frame")
        .all(|&(key, glyph, ..)| out[key] == glyph);
    if !ghost_intact {
        fall_back_group(&mut out, "ghost_frame");
    }
    for &(key, backstop) in ICON_BACKSTOP {
        if out.get(key).is_some_and(|g| width(g) != 1) {
            out.insert(key.to_string(), backstop.to_string());
            narrowed = true;
        }
    }
    if out["chamfer_top"] == chrome("chamfer_top").unwrap().1 {
        corners = "square";
    }

    Resolved {
        glyphs: out,
        corners,
        narrowed,
    }
}

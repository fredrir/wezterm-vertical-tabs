//! Process and chrome icon defaults. Glyphs are the real `wezterm.nerdfonts` values extracted
//! from WezTerm 20260826.

use std::collections::BTreeMap;

use crate::lua_pattern;

/// key, glyph
const DEFAULTS: &[(&str, &str)] = &[
    ("default", "\u{F07B7}"),
    ("zsh", "\u{F07B7}"),
    ("bash", "\u{F07B7}"),
    ("fish", "\u{F023A}"),
    ("nu", "\u{F07B7}"),
    ("sh", "\u{F07B7}"),
    ("cmd.exe", "\u{EBC4}"),
    ("pwsh.exe", "\u{EBC7}"),
    ("powershell.exe", "\u{EBC7}"),
    ("nvim", "\u{E6AE}"),
    ("vim", "\u{E62B}"),
    ("vi", "\u{E62B}"),
    ("hx", "\u{F0B0F}"),
    ("ssh", "\u{F0318}"),
    ("mosh", "\u{F0318}"),
    ("docker", "\u{F0868}"),
    ("git", "\u{E702}"),
    ("lazygit", "\u{E702}"),
    ("node", "\u{E718}"),
    ("bun", "\u{F031E}"),
    ("deno", "\u{F031E}"),
    ("python", "\u{E73C}"),
    ("python3", "\u{E73C}"),
    ("cargo", "\u{E7A8}"),
    ("rustc", "\u{E7A8}"),
    // dev_go (U+E724) is a blank glyph in both Nerd Fonts; seti_go actually draws something
    ("go", "\u{E627}"),
    ("make", "\u{EB6D}"),
    ("htop", "\u{F012A}"),
    ("btop", "\u{F012A}"),
    ("top", "\u{F012A}"),
    ("tmux", "\u{EBC8}"),
    ("claude", "\u{F06A9}"),
    ("mux", "\u{F0318}"),
    ("pinned", "\u{F0403}"),
    ("private", "\u{F05F9}"),
    // U+2716 is in no monospace cmap we checked; the thick Material close is in-font
    ("close", "\u{F1398}"),
    ("new_tab", "\u{EA60}"),
    // The strip trio is uniform and light; ⚙ is the recorded exception to the in-font rule
    ("strip_new_tab", "+"),
    ("settings", "⚙"),
    ("search", "\u{EA6D}"),
    ("unseen", "\u{F09DE}"),
    ("focus", "›"),
    ("active", "▎"),
    ("scroll", "▐"),
];

pub struct IconSet {
    pub map: BTreeMap<String, String>,
    /// Lua-pattern keys from icon_map, in sorted-key order.
    patterns: Vec<(String, String)>,
}

fn is_pattern(key: &str) -> bool {
    key.bytes()
        .any(|b| matches!(b, b'^' | b'$' | b'*' | b'+' | b'?' | b'['))
}

/// Merges user overrides once; patterns are kept in a stable order.
pub fn resolve(icon_map: &BTreeMap<String, String>) -> IconSet {
    let mut map: BTreeMap<String, String> = DEFAULTS
        .iter()
        .map(|&(key, glyph)| (key.to_string(), glyph.to_string()))
        .collect();
    for (key, value) in icon_map {
        map.insert(key.clone(), value.clone());
    }
    let patterns = icon_map
        .iter()
        .filter(|(key, _)| is_pattern(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    IconSet { map, patterns }
}

fn process_key(foreground: &str) -> Option<String> {
    if foreground.is_empty() {
        return None;
    }
    let base = crate::strings::basename(foreground);
    Some(base.strip_prefix('-').unwrap_or(base).to_string())
}

/// Picks an icon for a pane from its foreground process name.
pub fn for_process<'a>(foreground: &str, icons: &'a IconSet) -> &'a str {
    if let Some(key) = process_key(foreground) {
        if let Some(icon) = icons.map.get(&key) {
            return icon;
        }
        for (pattern, icon) in &icons.patterns {
            if lua_pattern::matches(&key, pattern) {
                return icon;
            }
        }
    }
    &icons.map["default"]
}

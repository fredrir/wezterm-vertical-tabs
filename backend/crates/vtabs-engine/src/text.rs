//! Port of util.lua's width/truncate/pad_right/sanitize/shorten_path; parity is pinned by
//! tests/text_parity.rs against a Lua-exported fixture.

use crate::strings::graphemes;
use unicode_width::UnicodeWidthStr;

/// Display columns as WezTerm counts them: controls are zero, ambiguous is narrow.
pub fn width(s: &str) -> usize {
    let visible = |c: char| c >= ' ' && c != '\x7f' && !('\u{80}'..='\u{9f}').contains(&c);
    if s.chars().all(visible) {
        UnicodeWidthStr::width(s)
    } else {
        let filtered: String = s.chars().filter(|&c| visible(c)).collect();
        UnicodeWidthStr::width(filtered.as_str())
    }
}

/// Truncates `s` to `max` display columns, appending `ellipsis` when cut.
pub fn truncate(s: &str, max: usize, ellipsis: &str) -> String {
    if max == 0 {
        return String::new();
    }
    if width(s) <= max {
        return s.to_string();
    }
    let budget = match max.checked_sub(width(ellipsis)) {
        Some(b) if b > 0 => b,
        _ => return String::new(),
    };
    let mut out = String::new();
    let mut used = 0;
    for grapheme in graphemes(s) {
        let w = width(grapheme);
        if used + w > budget {
            break;
        }
        out.push_str(grapheme);
        used += w;
    }
    out + ellipsis
}

pub fn pad_right(s: &str, cols: usize) -> String {
    let w = width(s);
    if w >= cols {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(cols - w))
}

/// Elides middle path components to fit `budget`; the basename survives longest.
pub fn shorten_path(path: &str, budget: usize, ellipsis: &str) -> String {
    if path.is_empty() || budget == 0 {
        return String::new();
    }
    if width(path) <= budget {
        return path.to_string();
    }
    let sep = if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    };
    let mut parts: Vec<String> = path
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    let mut lead = String::new();
    let drive = parts.first().is_some_and(|p| {
        p.len() == 2 && p.ends_with(':') && p.chars().next().unwrap().is_ascii_alphabetic()
    });
    if drive {
        lead = format!("{}{sep}", parts.remove(0));
    } else if path.starts_with(['/', '\\']) {
        lead = sep.to_string();
    }
    if parts.len() <= 1 {
        let only = parts.first().map(String::as_str).unwrap_or("");
        return format!(
            "{lead}{}",
            truncate(only, budget.saturating_sub(width(&lead)), ellipsis)
        );
    }
    let joined = |parts: &[String]| format!("{lead}{}", parts.join(&sep.to_string()));
    // Leftmost first, and never the marker or the basename.
    let from = if parts[0] == "~" || parts[0] == ".." {
        1
    } else {
        0
    };
    for i in from..parts.len() - 1 {
        if width(&joined(&parts)) <= budget {
            break;
        }
        parts[i] = truncate(&parts[i], 1, "");
    }
    let out = joined(&parts);
    if width(&out) <= budget {
        return out;
    }
    let base = parts.last().unwrap();
    match budget.checked_sub(width(ellipsis) + 1) {
        Some(room) if room >= 1 => format!("{ellipsis}{sep}{}", truncate(base, room, ellipsis)),
        _ => truncate(base, budget, ellipsis),
    }
}

#[cfg(test)]
mod tests {
    use super::{truncate, width};

    #[test]
    fn emoji_sequences_use_their_terminal_width_and_are_never_split() {
        assert_eq!(width("👩‍💻"), 2);
        assert_eq!(truncate("A👩‍💻B", 3, ""), "A👩‍💻");
    }
}

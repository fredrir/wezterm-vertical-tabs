//! Sanitisation and process-name helpers; the model boundary is the one place strings are cleaned.

use unicode_segmentation::UnicodeSegmentation;

/// User-perceived characters. Editing and truncation must not split a combining sequence or emoji.
pub fn graphemes(value: &str) -> impl DoubleEndedIterator<Item = &str> {
    UnicodeSegmentation::graphemes(value, true)
}

pub fn grapheme_count(value: &str) -> usize {
    graphemes(value).count()
}

pub fn pop_grapheme(value: &mut String) -> bool {
    let Some((at, _)) = UnicodeSegmentation::grapheme_indices(value.as_str(), true).next_back()
    else {
        return false;
    };
    value.truncate(at);
    true
}

/// Returns valid UTF-8 with no control characters or bidi overrides, whatever bytes went in.
pub fn sanitize(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            let keep = c >= ' '
                && c != '\x7f'
                && !('\u{80}'..='\u{9f}').contains(&c)
                && !('\u{202a}'..='\u{202e}').contains(&c);
            if keep {
                out.push(c);
            }
        }
    }
    out
}

pub fn basename(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|base| !base.is_empty())
        .unwrap_or(path)
}

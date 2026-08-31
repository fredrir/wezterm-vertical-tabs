//! Sanitisation and process-name helpers; the model boundary is the one place strings are cleaned.

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

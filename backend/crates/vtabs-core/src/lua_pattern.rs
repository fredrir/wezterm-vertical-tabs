//! Match-only subset of Lua patterns, for `icon_map` pattern keys: classes, sets,
//! quantifiers, anchors. No captures, no %b, no %f — those keys simply never match.

fn class_match(b: u8, class: u8) -> bool {
    let res = match class.to_ascii_lowercase() {
        b'a' => b.is_ascii_alphabetic(),
        b'c' => b.is_ascii_control(),
        b'd' => b.is_ascii_digit(),
        b'g' => b.is_ascii_graphic(),
        b'l' => b.is_ascii_lowercase(),
        b'p' => b.is_ascii_punctuation(),
        b's' => b.is_ascii_whitespace(),
        b'u' => b.is_ascii_uppercase(),
        b'w' => b.is_ascii_alphanumeric(),
        b'x' => b.is_ascii_hexdigit(),
        _ => return b == class,
    };
    if class.is_ascii_uppercase() {
        !res
    } else {
        res
    }
}

/// Length of the single pattern item starting at `p[pi]`, or None on a malformed pattern.
fn item_len(p: &[u8], pi: usize) -> Option<usize> {
    match p.get(pi)? {
        b'%' => {
            p.get(pi + 1)?;
            Some(2)
        }
        b'[' => {
            let mut i = pi + 1;
            if p.get(i) == Some(&b'^') {
                i += 1;
            }
            if p.get(i) == Some(&b']') {
                i += 1;
            }
            loop {
                match p.get(i)? {
                    b'%' => i += 2,
                    b']' => return Some(i + 1 - pi),
                    _ => i += 1,
                }
            }
        }
        _ => Some(1),
    }
}

fn single_match(s: &[u8], si: usize, p: &[u8], pi: usize, ilen: usize) -> bool {
    let Some(&b) = s.get(si) else { return false };
    match p[pi] {
        b'.' => true,
        b'%' => class_match(b, p[pi + 1]),
        b'[' => set_match(b, &p[pi..pi + ilen]),
        lit => b == lit,
    }
}

fn set_match(b: u8, set: &[u8]) -> bool {
    let inner = &set[1..set.len() - 1];
    let (negate, inner) = match inner.first() {
        Some(b'^') => (true, &inner[1..]),
        _ => (false, inner),
    };
    let mut i = 0;
    let mut found = false;
    while i < inner.len() {
        if inner[i] == b'%' && i + 1 < inner.len() {
            if class_match(b, inner[i + 1]) {
                found = true;
            }
            i += 2;
        } else if i + 2 < inner.len() && inner[i + 1] == b'-' && inner[i + 2] != b']' {
            if inner[i] <= b && b <= inner[i + 2] {
                found = true;
            }
            i += 3;
        } else {
            if inner[i] == b {
                found = true;
            }
            i += 1;
        }
    }
    found != negate
}

fn match_here(s: &[u8], si: usize, p: &[u8], pi: usize) -> bool {
    if pi >= p.len() {
        return true;
    }
    if p[pi] == b'$' && pi + 1 == p.len() {
        return si == s.len();
    }
    let Some(ilen) = item_len(p, pi) else {
        return false;
    };
    let next = pi + ilen;
    match p.get(next) {
        Some(b'?') => {
            if single_match(s, si, p, pi, ilen) && match_here(s, si + 1, p, next + 1) {
                return true;
            }
            match_here(s, si, p, next + 1)
        }
        Some(b'*') => max_expand(s, si, p, pi, ilen, next + 1, 0),
        Some(b'+') => max_expand(s, si, p, pi, ilen, next + 1, 1),
        Some(b'-') => {
            let mut si = si;
            loop {
                if match_here(s, si, p, next + 1) {
                    return true;
                }
                if single_match(s, si, p, pi, ilen) {
                    si += 1;
                } else {
                    return false;
                }
            }
        }
        _ => single_match(s, si, p, pi, ilen) && match_here(s, si + 1, p, next),
    }
}

fn max_expand(
    s: &[u8],
    si: usize,
    p: &[u8],
    pi: usize,
    ilen: usize,
    cont: usize,
    min: usize,
) -> bool {
    let mut count = 0;
    while single_match(s, si + count, p, pi, ilen) {
        count += 1;
    }
    while count + 1 > min {
        if match_here(s, si + count, p, cont) {
            return true;
        }
        if count == 0 {
            break;
        }
        count -= 1;
    }
    count >= min && match_here(s, si + count, p, cont)
}

/// True when `pattern` matches anywhere in `s`, as Lua's `s:match(pattern)` truthiness.
pub fn matches(s: &str, pattern: &str) -> bool {
    let (s, p) = (s.as_bytes(), pattern.as_bytes());
    // captures, balanced match and frontier are not supported: such a key never matches
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            b'%' if matches!(p.get(i + 1), Some(b'b') | Some(b'f') | Some(b'0'..=b'9')) => {
                return false;
            }
            b'(' | b')' => return false,
            b'%' => i += 2,
            _ => i += 1,
        }
    }
    let (anchored, p) = match p.first() {
        Some(b'^') => (true, &p[1..]),
        _ => (false, p),
    };
    if anchored {
        return match_here(s, 0, p, 0);
    }
    (0..=s.len()).any(|si| match_here(s, si, p, 0))
}

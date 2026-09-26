pub(crate) fn reveal(scroll: usize, at: usize, end: impl Fn(usize) -> usize) -> usize {
    let mut scroll = scroll.min(at);
    while scroll < at && end(scroll) <= at {
        scroll += 1;
    }
    scroll
}

pub(crate) fn reveal_rows(scroll: usize, at: usize, rows: usize) -> usize {
    if at < scroll {
        at
    } else if at >= scroll + rows {
        at + 1 - rows
    } else {
        scroll
    }
}

pub(crate) fn scroll_to(scroll: usize, at: usize, len: usize, rows: usize) -> usize {
    let rows = rows.max(1);
    reveal_rows(scroll, at, rows).min(len.saturating_sub(rows))
}

pub(crate) fn offset(current: usize, delta: i32, len: usize) -> usize {
    if delta >= 0 {
        current
            .saturating_add(delta as usize)
            .min(len.saturating_sub(1))
    } else {
        current.saturating_sub(delta.unsigned_abs() as usize)
    }
}

pub(crate) fn next_enabled<T>(
    items: &[T],
    current: usize,
    delta: isize,
    enabled: impl Fn(&T) -> bool,
) -> usize {
    if items.is_empty() {
        return 0;
    }
    for step in 1..=items.len() {
        let i =
            (current as isize + delta * step as isize).rem_euclid(items.len() as isize) as usize;
        if enabled(&items[i]) {
            return i;
        }
    }
    current.min(items.len() - 1)
}

pub(crate) fn cycle<T: PartialEq + Clone>(
    ids: &[T],
    current: Option<&T>,
    backwards: bool,
) -> Option<T> {
    let len = ids.len();
    let at = current.and_then(|current| ids.iter().position(|id| id == current));
    let index = match (at, backwards) {
        _ if len == 0 => return None,
        (Some(at), false) => (at + 1) % len,
        (Some(at), true) => (at + len - 1) % len,
        (None, false) => 0,
        (None, true) => len - 1,
    };
    Some(ids[index].clone())
}

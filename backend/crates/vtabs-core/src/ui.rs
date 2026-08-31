//! Per-pane pointer state the backend owns now that it paints: what input.lua kept in
//! `store.hover`, `pending_close`/`pending_menu`, `store.last_click` and `store.drag`.
//! Data only — `vtabs-input::resolve` reads it, `vtabs-runtime` ages it.

/// Times are `util.now_ms()` milliseconds, so the Lua constants port unchanged.
pub type Ms = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hover {
    pub x: i64,
    pub y: i64,
    pub at: Ms,
}

/// A press that only acts if the release lands on the same target again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmKind {
    Close,
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armed {
    pub kind: ArmKind,
    pub tab_id: i64,
    pub row: i64,
    pub col: i64,
    pub at: Ms,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastClick {
    pub key: String,
    pub at: Ms,
}

/// The press this process is holding. The drag it arms may be carried by another backend
/// process once the pointer leaves this pane, which is why the model mirrors it back (§1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PressDrag {
    pub tab_id: i64,
    pub origin_x: i64,
    pub origin_y: i64,
    pub began: Ms,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiState {
    pub hover: Option<Hover>,
    pub armed: Option<Armed>,
    pub last_click: Option<LastClick>,
    /// Set by the wheel so the list moves before the model round-trips; cleared when it lands.
    pub scroll: Option<i64>,
    pub user_scrolled: bool,
    pub drag: Option<PressDrag>,
}

impl UiState {
    pub fn set_hover(&mut self, x: i64, y: i64, at: Ms) {
        self.hover = Some(Hover { x, y, at });
    }

    /// `input.tick`: hover older than `timeout_ms` is stale pointer state, not a hover.
    pub fn expire_hover(&mut self, now: Ms, timeout_ms: u64) -> bool {
        if timeout_ms == 0 {
            return false;
        }
        if self
            .hover
            .is_some_and(|h| now.saturating_sub(h.at) > timeout_ms)
        {
            self.hover = None;
            return true;
        }
        false
    }

    /// `hit.double_click`: the same target twice inside the window, and the match consumes the first.
    pub fn double_click(&mut self, key: &str, now: Ms, window_ms: u64) -> bool {
        let hit = self
            .last_click
            .as_ref()
            .is_some_and(|last| last.key == key && now.saturating_sub(last.at) <= window_ms);
        self.last_click = if hit {
            None
        } else {
            Some(LastClick {
                key: key.to_string(),
                at: now,
            })
        };
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_click_on_the_same_target_inside_the_window_is_a_double() {
        let mut ui = UiState::default();
        assert!(!ui.double_click("tab:1", 0, 300));
        assert!(ui.double_click("tab:1", 200, 300));
        // the match consumed the first, so a third click starts over
        assert!(!ui.double_click("tab:1", 250, 300));
    }

    #[test]
    fn a_different_target_or_a_late_click_is_not() {
        let mut ui = UiState::default();
        assert!(!ui.double_click("tab:1", 0, 300));
        assert!(!ui.double_click("tab:2", 50, 300), "another target");
        let mut late = UiState::default();
        assert!(!late.double_click("space", 0, 300));
        assert!(!late.double_click("space", 400, 300), "past the window");
    }

    #[test]
    fn hover_expires_only_once_and_never_with_the_timeout_off() {
        let mut ui = UiState::default();
        ui.set_hover(3, 4, 100);
        assert!(!ui.expire_hover(150, 120), "still fresh");
        assert!(ui.expire_hover(300, 120), "gone");
        assert!(!ui.expire_hover(400, 120), "and stays gone");

        let mut off = UiState::default();
        off.set_hover(3, 4, 0);
        assert!(!off.expire_hover(999_999, 0));
        assert!(off.hover.is_some());
    }
}

//! Per-pane pointer state the backend owns now that it paints: what input.lua kept in
//! `store.hover`, `pending_close`/`pending_menu`, `store.last_click` and `store.drag`.
//! Data only — `vtabs-engine::interaction` reads it, `vtabs-runtime` ages it.

use crate::strings::{grapheme_count, pop_grapheme};

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

/// The press this process is holding
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

/// The settings screen's local state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsUi {
    /// 1-based index into `model.groups`.
    pub group: i64,
    /// 1-based index into the rows the filter left showing, caveat lines included.
    pub focus: i64,
    pub scroll: i64,
    pub filter: String,
    pub filtering: bool,
}

impl Default for SettingsUi {
    fn default() -> Self {
        SettingsUi {
            group: 1,
            focus: 1,
            scroll: 0,
            filter: String::new(),
            filtering: false,
        }
    }
}

impl SettingsUi {
    pub fn type_filter(&mut self, key: &str) {
        match key {
            "escape" => {
                self.filtering = false;
                self.filter.clear();
            }
            "enter" => self.filtering = false,
            "backspace" => {
                pop_grapheme(&mut self.filter);
            }
            _ if grapheme_count(key) == 1 => self.filter.push_str(key),
            _ => {}
        }
    }
}

use serde::Serialize;

use crate::b64;
use crate::types::Mods;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    Ready {
        cols: u16,
        rows: u16,
        /// The backend's own server pane id, so Lua can `kill` it by id through a sibling.
        pane: u64,
        /// The inbox session offered for this ready; absent where no transport root was given.
        #[serde(skip_serializing_if = "Option::is_none")]
        transport: Option<Transport>,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Key {
        key: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        mods: Vec<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        raw: Option<String>,
        /// True once the backend already wrote `raw` into the content pane server-side.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        delivered: bool,
    },
    Focus {
        #[serde(rename = "in")]
        focused: bool,
    },
    Paste {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        dropped: Option<&'static str>,
    },
    /// `echo` is the ping's own `n`; the monotonic `n` every event carries is added on emit.
    Pong {
        #[serde(skip_serializing_if = "Option::is_none")]
        echo: Option<u64>,
    },
    /// `seq` names the one inbox message a gap swallowed (`what: "message"`).
    Dropped {
        what: &'static str,
        reason: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        seq: Option<u32>,
    },
    TransportReady {
        session: String,
    },
    TransportRefused {
        session: String,
        why: &'static str,
    },
    Intent {
        #[serde(flatten)]
        intent: Intent,
    },
    /// Rust's resolved base, passed to the host hook while the publication remains staged.
    ThemeHookRequest {
        theme: crate::payload::ResolvedTheme,
    },
    /// The effective theme Rust committed, for host chrome and the Zen companion.
    ThemeResolved {
        theme: crate::payload::ResolvedTheme,
    },
    /// All sticky-cache misses for the staged publication. The host executes its Lua route hook
    /// and answers once with a `space_route_hook_result` before anything is published.
    SpaceRouteHookRequest {
        window_id: i64,
        tabs: Vec<crate::payload::SpaceRouteHookFact>,
    },
    /// The complete topology and active raw theme layer committed atomically.
    SpacesResolved {
        window_id: i64,
        #[serde(flatten)]
        resolution: Box<crate::payload::SpaceResolution>,
    },
    /// One canonical Rust commit. Lua applies only this path to its host-side config, then writes
    /// the already-final JSON body if persistence is enabled.
    SettingsCommit {
        path: Vec<String>,
        change: SettingsChange,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        derived: Vec<SettingsPatch>,
        mode: SettingsApplyMode,
        persistence_json: String,
    },
    /// The complete paste-ready Lua snippet, rendered by Rust from the canonical changed set.
    SettingsCopy {
        lua: String,
    },
    MenuRefused {
        #[serde(skip_serializing_if = "Option::is_none")]
        why: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        level: Option<&'static str>,
    },
    /// What a `kill`, `rescue` or `adjust` did on the server: `detail` is the count moved, the
    /// pane still owed its focus, or the error; `cols` is this pane's own width once an `adjust`
    /// has run, the server's word on where the split landed.
    Cli {
        op: &'static str,
        ok: bool,
        #[serde(skip_serializing_if = "String::is_empty")]
        detail: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cols: Option<u16>,
    },
}

/// The inbox session `ready` offers: a directory basename under the root Lua chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transport {
    pub inbox: String,
}

/// A renderer-produced action. Each variant carries only the fields that action can consume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "a", rename_all = "snake_case")]
pub enum Intent {
    PressCard {
        tab_id: i64,
        x: i64,
        y: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<CardPart>,
    },
    DragTo {
        x: i64,
        y: i64,
        slot: i64,
        outside: bool,
    },
    DragEnd {
        outside: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        slot: Option<i64>,
    },
    RequestClose {
        tab_id: i64,
        row: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        col: Option<i64>,
        from_key: bool,
    },
    TogglePin {
        tab_id: i64,
    },
    OpenMenu {
        tab_id: i64,
        row: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        col: Option<i64>,
    },
    NewTab,
    Strip {
        button_id: String,
    },
    Footer {
        index: i64,
    },
    SwitchSpace {
        space_id: String,
    },
    WheelTab {
        dy: i64,
    },
    SetScroll {
        top: i64,
        user: bool,
    },
    SetFocusIndex {
        index: i64,
    },
    ActivateTab {
        tab_id: i64,
    },
    BlurSidebar,
    MenuPick {
        item_id: String,
    },
    MenuBack,
    MenuClosed,
    RenameCommit {
        text: String,
    },
    RenameTab {
        tab_id: i64,
    },
    MoveTab {
        tab_id: i64,
        slot: i64,
        focus_index: i64,
    },
    SetRailReserve {
        cols: i64,
    },
    NudgeOption {
        key: String,
        delta: i64,
    },
    ActivateOption {
        key: String,
    },
    ResetOption {
        key: String,
    },
    SettingsCopy,
    EditKey {
        key: String,
    },
    RecordChord {
        key: String,
        mods: Vec<Modifier>,
    },
    CloseSettings,
}

/// One inventory drives Intent::name and the generated Lua contract. A serialization test keeps
/// serde's snake_case tags tied to it.
macro_rules! intent_contract {
    ($visit:ident) => {
        $visit!(
            Intent::PressCard { .. } => "press_card",
            Intent::DragTo { .. } => "drag_to",
            Intent::DragEnd { .. } => "drag_end",
            Intent::RequestClose { .. } => "request_close",
            Intent::TogglePin { .. } => "toggle_pin",
            Intent::OpenMenu { .. } => "open_menu",
            Intent::NewTab => "new_tab",
            Intent::Strip { .. } => "strip",
            Intent::Footer { .. } => "footer",
            Intent::SwitchSpace { .. } => "switch_space",
            Intent::WheelTab { .. } => "wheel_tab",
            Intent::SetScroll { .. } => "set_scroll",
            Intent::SetFocusIndex { .. } => "set_focus_index",
            Intent::ActivateTab { .. } => "activate_tab",
            Intent::BlurSidebar => "blur_sidebar",
            Intent::MenuPick { .. } => "menu_pick",
            Intent::MenuBack => "menu_back",
            Intent::MenuClosed => "menu_closed",
            Intent::RenameCommit { .. } => "rename_commit",
            Intent::RenameTab { .. } => "rename_tab",
            Intent::MoveTab { .. } => "move_tab",
            Intent::SetRailReserve { .. } => "set_rail_reserve",
            Intent::NudgeOption { .. } => "nudge_option",
            Intent::ActivateOption { .. } => "activate_option",
            Intent::ResetOption { .. } => "reset_option",
            Intent::SettingsCopy => "settings_copy",
            Intent::EditKey { .. } => "edit_key",
            Intent::RecordChord { .. } => "record_chord",
            Intent::CloseSettings => "close_settings",
        )
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardPart {
    Pad,
    Title,
    Meta,
    Gap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
}

/// A removal is distinct from setting JSON null; optional argument bags cannot express that safely.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SettingsChange {
    Set { value: serde_json::Value },
    Remove,
}

/// A secondary setting changed by Rust's cross-field normalization policy.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettingsPatch {
    pub path: Vec<String>,
    #[serde(flatten)]
    pub change: SettingsChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsApplyMode {
    Instant,
    Override,
    Reload,
}

pub fn modifiers(mods: Mods) -> Vec<Modifier> {
    let mut list = Vec::new();
    if mods.shift {
        list.push(Modifier::Shift);
    }
    if mods.ctrl {
        list.push(Modifier::Ctrl);
    }
    if mods.alt {
        list.push(Modifier::Alt);
    }
    list
}

impl Intent {
    pub const NAMES: &'static [&'static str] = {
        macro_rules! names {
            ($($pattern:pat => $name:literal),+ $(,)?) => {
                &[$($name),+]
            };
        }
        intent_contract!(names)
    };

    pub fn name(&self) -> &'static str {
        macro_rules! match_name {
            ($($pattern:pat => $name:literal),+ $(,)?) => {
                match self {
                    $($pattern => $name),+
                }
            };
        }
        intent_contract!(match_name)
    }
}

impl Event {
    /// `ready` from a pane that knows its server id and, with `inbox`, offers a session there.
    pub fn ready_at(cols: u16, rows: u16, pane: u64, inbox: Option<String>) -> Self {
        Event::Ready {
            cols,
            rows,
            pane,
            transport: inbox.map(|inbox| Transport { inbox }),
        }
    }

    pub fn dropped(what: &'static str, reason: &'static str) -> Self {
        Event::Dropped {
            what,
            reason,
            seq: None,
        }
    }

    /// The one inbox message a gap cost, once its grace ran out.
    pub fn dropped_message(seq: u32) -> Self {
        Event::Dropped {
            what: "message",
            reason: "gap",
            seq: Some(seq),
        }
    }

    pub fn intent(intent: Intent) -> Self {
        Event::Intent { intent }
    }

    pub fn paste(data: Option<Vec<u8>>) -> Self {
        match data {
            Some(bytes) => Event::Paste {
                data: Some(b64(&bytes)),
                dropped: None,
            },
            None => Event::Paste {
                data: None,
                dropped: Some("size"),
            },
        }
    }

    pub fn key(name: String, mods: Mods, raw: &[u8]) -> Self {
        Event::Key {
            key: name,
            mods: mods_list(mods),
            raw: (!raw.is_empty()).then(|| b64(raw)),
            delivered: false,
        }
    }

    /// The same key, marked as already written into the content pane by the backend.
    pub fn delivered(mut self) -> Self {
        if let Event::Key { delivered, .. } = &mut self {
            *delivered = true;
        }
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("event serializes")
    }
}

pub fn mods_list(mods: Mods) -> Vec<&'static str> {
    let mut list = Vec::new();
    if mods.shift {
        list.push("shift");
    }
    if mods.ctrl {
        list.push("ctrl");
    }
    if mods.alt {
        list.push("alt");
    }
    list
}

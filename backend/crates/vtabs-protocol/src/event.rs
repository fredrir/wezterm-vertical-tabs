use serde::Serialize;

use crate::b64;
use crate::types::Mods;

pub use crate::limits::VERSION;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    Ready {
        v: u8,
        cols: u16,
        rows: u16,
        paints: bool,
        caps: Vec<&'static str>,
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
    Dropped {
        what: &'static str,
        reason: &'static str,
    },
    Intent {
        #[serde(flatten)]
        intent: Intent,
    },
    /// Rust's resolved base, passed to the host hook while this generation remains unpublished.
    ThemeHookRequest {
        generation: u64,
        theme: crate::v2::ResolvedTheme,
    },
    /// The effective theme Rust committed, for host chrome and the Zen companion.
    ThemeResolved {
        #[serde(skip_serializing_if = "Option::is_none")]
        generation: Option<u64>,
        theme: crate::v2::ResolvedTheme,
    },
    /// All sticky-cache misses for one atomic generation. The host executes its Lua route hook and
    /// answers once with a `space_route_hook_result`; nothing from this generation is published in
    /// between.
    SpaceRouteHookRequest {
        generation: u64,
        window_id: i64,
        tabs: Vec<crate::v2::SpaceRouteHookFact>,
    },
    /// The complete topology and active raw theme layer committed for one atomic generation.
    SpacesResolved {
        generation: u64,
        window_id: i64,
        #[serde(flatten)]
        resolution: Box<crate::v2::SpaceResolution>,
    },
    /// One canonical Rust commit. Lua applies only this path to its host-side config, then writes
    /// the already-final JSON body if persistence is enabled.
    SettingsCommit {
        settings_rev: u64,
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
    /// Legacy compatibility envelope for clients without typed intents. The args are boxed because
    /// every variant sets only a handful of this wide union.
    Do {
        a: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<DoId>,
        #[serde(skip_serializing_if = "DoArgs::is_empty")]
        args: Box<DoArgs>,
    },
    Note {
        k: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        why: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<i64>,
        /// The level the note is about, so Lua picks its own fallback rather than being told one.
        #[serde(skip_serializing_if = "Option::is_none")]
        a: Option<&'static str>,
    },
    /// What a `kill` or `rescue` did on the server: `detail` is the count moved, or the error.
    Cli {
        op: &'static str,
        ok: bool,
        #[serde(skip_serializing_if = "String::is_empty")]
        detail: String,
    },
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

impl CardPart {
    fn name(self) -> &'static str {
        match self {
            Self::Pad => "pad",
            Self::Title => "title",
            Self::Meta => "meta",
            Self::Gap => "gap",
        }
    }
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

impl Modifier {
    fn name(self) -> &'static str {
        match self {
            Self::Shift => "shift",
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
        }
    }
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

    fn legacy_name(&self) -> &'static str {
        match self {
            Intent::ActivateTab { .. } => "activate_tab_by_id",
            _ => self.name(),
        }
    }

    /// The single compatibility mapping for clients that have not advertised `typed_intents`.
    pub fn downgrade(&self) -> Event {
        let name = self.legacy_name();
        match self {
            Intent::PressCard { tab_id, x, y, part } => Event::do_tab(name, *tab_id).with(|args| {
                args.x = Some(*x);
                args.y = Some(*y);
                args.part = part.map(CardPart::name);
            }),
            Intent::DragTo {
                x,
                y,
                slot,
                outside,
            } => Event::do_(name).with(|args| {
                args.x = Some(*x);
                args.y = Some(*y);
                args.slot = Some(*slot);
                args.outside = Some(*outside);
            }),
            Intent::DragEnd { outside, slot } => Event::do_(name).with(|args| {
                args.outside = Some(*outside);
                args.slot = *slot;
            }),
            Intent::RequestClose {
                tab_id,
                row,
                col,
                from_key,
            } => Event::do_tab(name, *tab_id).with(|args| {
                args.row = Some(*row);
                args.col = *col;
                args.from_key = from_key.then_some(true);
            }),
            Intent::TogglePin { tab_id } => Event::do_tab(name, *tab_id),
            Intent::OpenMenu { tab_id, row, col } => Event::do_tab(name, *tab_id).with(|args| {
                args.row = Some(*row);
                args.col = *col;
            }),
            Intent::NewTab => Event::do_(name),
            Intent::Strip { button_id } => Event::do_named(name, button_id.clone()),
            Intent::Footer { index } => Event::do_(name).with(|args| args.index = Some(*index)),
            Intent::SwitchSpace { space_id } => Event::do_named(name, space_id.clone()),
            Intent::WheelTab { dy } => Event::do_(name).with(|args| args.dy = Some(*dy)),
            Intent::SetScroll { top, user } => Event::do_(name).with(|args| {
                args.top = Some(*top);
                args.user = Some(*user);
            }),
            Intent::SetFocusIndex { index } => {
                Event::do_(name).with(|args| args.index = Some(*index))
            }
            Intent::ActivateTab { tab_id } => Event::do_tab(name, *tab_id),
            Intent::BlurSidebar => Event::do_(name),
            Intent::MenuPick { item_id } => {
                Event::do_(name).with(|args| args.id = Some(item_id.clone()))
            }
            Intent::MenuBack => Event::do_(name),
            Intent::MenuClosed => Event::do_(name),
            Intent::RenameCommit { text } => {
                Event::do_(name).with(|args| args.text = Some(text.clone()))
            }
            Intent::RenameTab { tab_id } => Event::do_tab(name, *tab_id),
            Intent::MoveTab {
                tab_id,
                slot,
                focus_index,
            } => Event::do_tab(name, *tab_id).with(|args| {
                args.slot = Some(*slot);
                args.focus_index = Some(*focus_index);
            }),
            Intent::SetRailReserve { cols } => {
                Event::do_(name).with(|args| args.cols = Some(*cols))
            }
            Intent::NudgeOption { key, delta } => Event::do_(name).with(|args| {
                args.key = Some(key.clone());
                args.delta = Some(*delta);
            }),
            Intent::ActivateOption { key } => {
                Event::do_(name).with(|args| args.key = Some(key.clone()))
            }
            Intent::ResetOption { key } => {
                Event::do_(name).with(|args| args.key = Some(key.clone()))
            }
            Intent::SettingsCopy => Event::do_(name),
            Intent::EditKey { key } => Event::do_(name).with(|args| args.key = Some(key.clone())),
            Intent::RecordChord { key, mods } => Event::do_(name).with(|args| {
                args.key = Some(key.clone());
                args.mods = mods.iter().map(|modifier| modifier.name()).collect();
            }),
            Intent::CloseSettings => Event::do_(name),
        }
    }
}

/// A `do` target: tab ids are numbers, strip buttons are their string id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum DoId {
    Tab(i64),
    Name(String),
}

/// Every argument any `do` action carries; each event sets only the ones its handler reads.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct DoArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dy: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cols: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inside: Option<bool>,
    /// The rename buffer Rust owns, handed back whole on commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The settings verbs' target: an option key, or the raw key name for `edit_key`/`record_chord`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<i64>,
    /// `record_chord` only; settings_model.lua joins the list with `|`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<&'static str>,
}

impl DoArgs {
    pub fn is_empty(&self) -> bool {
        *self == DoArgs::default()
    }
}

impl Event {
    pub fn ready(cols: u16, rows: u16) -> Self {
        Event::Ready {
            v: VERSION,
            cols,
            rows,
            paints: true,
            caps: vec![
                "atomic_sync",
                "typed_intents",
                "theme_hooks",
                "settings_document",
                "spaces_policy",
            ],
        }
    }

    pub fn intent(intent: Intent) -> Self {
        Event::Intent { intent }
    }

    fn do_(a: &'static str) -> Self {
        Event::Do {
            a,
            id: None,
            args: Box::default(),
        }
    }

    fn do_tab(a: &'static str, id: i64) -> Self {
        Event::Do {
            a,
            id: Some(DoId::Tab(id)),
            args: Box::default(),
        }
    }

    /// A verb aimed at something Lua names by string: a strip button, a space.
    fn do_named(a: &'static str, id: String) -> Self {
        Event::Do {
            a,
            id: Some(DoId::Name(id)),
            args: Box::default(),
        }
    }

    fn with(mut self, set: impl FnOnce(&mut DoArgs)) -> Self {
        if let Event::Do { args, .. } = &mut self {
            set(args.as_mut());
        }
        self
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
        }
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

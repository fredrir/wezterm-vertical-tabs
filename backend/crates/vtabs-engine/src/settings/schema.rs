use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use super::value::{Value, get_path, set_path, table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Any,
    Boolean,
    Number,
    String,
    Table,
    List,
    Enum,
    Function,
}

/// How a committed settings value reaches a running WezTerm instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApplyMode {
    #[default]
    Instant,
    Override,
    Reload,
}

impl ApplyMode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Override => "override",
            Self::Reload => "reload",
        }
    }
}

impl Kind {
    pub const fn lua_name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::String => "string",
            Self::Table => "table",
            Self::List => "list",
            Self::Enum => "enum",
            Self::Function => "function",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Descriptor {
    pub key: &'static str,
    pub kind: Kind,
    pub default: Option<Value>,
    pub integer: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub allowed: Vec<Value>,
    pub list_of: Option<Kind>,
    pub container: bool,
    pub open: bool,
    pub docs: Option<bool>,
    pub shown: Option<&'static str>,
    pub label: Option<&'static str>,
    pub group: Option<&'static str>,
    pub values: Option<&'static str>,
    pub help: Option<&'static str>,
    /// WezTerm configuration key affected by this setting, when there is one.
    pub host_key: Option<&'static str>,
    /// Additional exact setting paths sharing this descriptor's host/apply policy. This keeps
    /// open-table children such as `frame.margin` out of the validation descriptor list.
    pub policy_paths: Vec<&'static str>,
    pub apply_mode: ApplyMode,
}

impl Descriptor {
    pub fn new(key: &'static str, kind: Kind) -> Self {
        Self {
            key,
            kind,
            default: None,
            integer: false,
            min: None,
            max: None,
            allowed: Vec::new(),
            list_of: None,
            container: false,
            open: false,
            docs: None,
            shown: None,
            label: None,
            group: None,
            values: None,
            help: None,
            host_key: None,
            policy_paths: Vec::new(),
            apply_mode: ApplyMode::Instant,
        }
    }

    fn with_default(mut self, value: impl Into<Value>) -> Self {
        self.default = Some(value.into());
        self
    }

    fn integer(mut self) -> Self {
        self.integer = true;
        self
    }

    fn min(mut self, value: impl Into<f64>) -> Self {
        self.min = Some(value.into());
        self
    }

    fn max(mut self, value: impl Into<f64>) -> Self {
        self.max = Some(value.into());
        self
    }

    fn allowed(mut self, values: Vec<Value>) -> Self {
        self.allowed = values;
        self
    }

    fn list_of(mut self, kind: Kind) -> Self {
        self.list_of = Some(kind);
        self
    }

    fn container(mut self) -> Self {
        self.container = true;
        self
    }

    fn open(mut self) -> Self {
        self.open = true;
        self
    }

    fn docs(mut self, value: bool) -> Self {
        self.docs = Some(value);
        self
    }

    fn shown(mut self, value: &'static str) -> Self {
        self.shown = Some(value);
        self
    }

    fn label(mut self, value: &'static str) -> Self {
        self.label = Some(value);
        self
    }

    fn group(mut self, value: &'static str) -> Self {
        self.group = Some(value);
        self
    }

    fn values(mut self, value: &'static str) -> Self {
        self.values = Some(value);
        self
    }

    fn help(mut self, value: &'static str) -> Self {
        self.help = Some(value);
        self
    }

    fn host_key(mut self, value: &'static str) -> Self {
        self.host_key = Some(value);
        self
    }

    fn policy_paths(mut self, values: Vec<&'static str>) -> Self {
        self.policy_paths = values;
        self
    }

    fn apply_mode(mut self, value: ApplyMode) -> Self {
        self.apply_mode = value;
        self
    }
}

macro_rules! values {
    ($($value:expr),* $(,)?) => {
        vec![$(Value::from($value)),*]
    };
}

fn build_options() -> Vec<Descriptor> {
    vec![
        Descriptor::new("width", Kind::Number)
            .with_default(28)
            .integer()
            .min(8)
            .label("Width")
            .group("layout")
            .help("sidebar width in cells"),
        Descriptor::new("dim_inactive_panes", Kind::Boolean)
            .with_default(false)
            .host_key("inactive_pane_hsb")
            .apply_mode(ApplyMode::Override)
            .label("Dim inactive panes")
            .group("layout")
            .help("let wezterm dim whichever pane is idle, sidebar included"),
        Descriptor::new("position", Kind::Enum)
            .with_default("left")
            .allowed(values!["left", "right"])
            .label("Position")
            .group("layout")
            .help("which side the sidebar sits on"),
        Descriptor::new("collapsed", Kind::Enum)
            .with_default("rail")
            .allowed(values!["rail", "hidden"])
            .label("Collapsed")
            .group("layout")
            .help("what the toggle collapses to"),
        Descriptor::new("rail_width", Kind::Number)
            .with_default(5)
            .integer()
            .min(3)
            .label("Rail width")
            .group("layout")
            .help("rail width in cells; widened to the macOS button reserve"),
        Descriptor::new("rail_titlebar", Kind::Enum)
            .with_default("widen")
            .allowed(values!["widen", "band", "none"])
            .label("Rail title bar")
            .group("layout")
            .help("macOS window buttons: widen the rail, band the window, or neither"),
        Descriptor::new("hide_native_tab_bar", Kind::Boolean)
            .with_default(true)
            .apply_mode(ApplyMode::Reload)
            .label("Hide native tab bar")
            .group("layout")
            .help("sets `enable_tab_bar = false`"),
        Descriptor::new("poll_ms", Kind::Number)
            .with_default(500)
            .integer()
            .min(50)
            .label("Poll interval")
            .group("behaviour")
            .help("upper bound for `status_update_interval`; drives the refresh"),
        Descriptor::new("padding", Kind::Table)
            .container()
            .shown("`{ top = 1, left = 2, right = 2, bottom = 1 }`")
            .label("Padding")
            .group("layout")
            .values("`{ top, left, right, bottom }`")
            .help("balanced padding inside the sidebar; two columns match one row at a terminal cell's aspect ratio"),
        Descriptor::new("padding.top", Kind::Number)
            .with_default(1)
            .integer()
            .min(0)
            .docs(false)
            .label("Padding top")
            .group("layout"),
        Descriptor::new("padding.left", Kind::Number)
            .with_default(2)
            .integer()
            .min(0)
            .docs(false)
            .label("Padding left")
            .group("layout"),
        Descriptor::new("padding.right", Kind::Number)
            .with_default(2)
            .integer()
            .min(0)
            .docs(false)
            .label("Padding right")
            .group("layout"),
        Descriptor::new("padding.bottom", Kind::Number)
            .with_default(1)
            .integer()
            .min(0)
            .docs(false)
            .label("Padding bottom")
            .group("layout"),
        Descriptor::new("settings", Kind::Any)
            .with_default(true)
            .open()
            .label("Settings page")
            .group("behaviour")
            .values("`true` \\| `false` \\| table")
            .help("the settings page; a table takes `{ persist, path }`, and it is a normal tab running this plugin's backend in its settings role"),
        Descriptor::new("edge_to_edge", Kind::Enum)
            .with_default("sides")
            .allowed(values![true, "sides", false])
            .host_key("window_padding")
            .apply_mode(ApplyMode::Reload)
            .label("Edge to edge")
            .group("layout")
            .values("`true` \\| `\"sides\"` \\| `false`")
            .help("zero wezterm's window padding; `\"sides\"` keeps the content pane's top and bottom half-cell"),
        Descriptor::new("tab_height", Kind::Enum)
            .with_default("card")
            .allowed(values!["card", "row", "tall"])
            .label("Card height")
            .group("cards")
            .values("`\"card\"` \\| `\"row\"` \\| `\"tall\"`")
            .help("painted rows per tab: content plus a blank pad row each side, one more with `meta` on"),
        Descriptor::new("meta", Kind::Enum)
            .with_default(false)
            .allowed(values!["auto", "cwd", "process", false])
            .label("Meta line")
            .group("cards")
            .values("`\"auto\"` \\| `\"cwd\"` \\| `\"process\"` \\| `false`")
            .help("what the second card row shows"),
        Descriptor::new("meta_sep", Kind::String)
            .with_default("  ")
            .label("Meta separator")
            .group("cards")
            .help("between the process and the path on the meta line"),
        Descriptor::new("row_gap", Kind::Number)
            .with_default(0)
            .integer()
            .min(0)
            .label("Row gap")
            .group("cards")
            .help("blank rows after each card; part of the card's click target"),
        Descriptor::new("new_tab_button", Kind::Enum)
            .with_default("ghost")
            .allowed(values!["ghost", "row", false])
            .label("New tab button")
            .group("chrome")
            .values("`\"ghost\"` \\| `\"row\"` \\| `false`")
            .help("how the new-tab affordance is drawn"),
        Descriptor::new("new_tab_label", Kind::String)
            .with_default("New tab")
            .label("New tab label")
            .group("chrome")
            .help("label inside the new-tab card"),
        Descriptor::new("corners", Kind::Enum)
            .with_default("chamfer")
            .allowed(values!["chamfer", "square"])
            .label("Corners")
            .group("cards")
            .help("card corner treatment"),
        Descriptor::new("frame", Kind::Any)
            .with_default(false)
            .open()
            .host_key("window_padding")
            .policy_paths(vec!["frame.zen", "frame.margin", "frame.inset"])
            .apply_mode(ApplyMode::Override)
            .label("Frame")
            .group("layout")
            .values("`\"zen\"` \\| `false` \\| table")
            .help("inset the sidebar, leaving a gutter column in the content colour"),
        Descriptor::new("titlebar", Kind::Enum)
            .with_default("auto")
            .allowed(values!["auto", "integrate", "plain", "macos"])
            .host_key("window_decorations")
            .apply_mode(ApplyMode::Reload)
            .label("Title bar")
            .group("chrome")
            .help("reserve cells for the macOS traffic lights; `\"macos\"` forces the reserve anywhere"),
        Descriptor::new("context", Kind::Enum)
            .with_default("popover")
            .allowed(values!["popover", false])
            .label("Context menu")
            .group("behaviour")
            .values("`\"popover\"` \\| `false`")
            .help("right-click behaviour"),
        Descriptor::new("popover", Kind::Table)
            .container()
            .shown("`{ width = \"auto\", follow_pointer = true, fade_ms = 90, overflow = \"clip\" }`")
            .label("Popover")
            .group("behaviour")
            .values("see below")
            .help("the action menu's width, hover, fade and overflow"),
        Descriptor::new("popover.width", Kind::Any)
            .with_default("auto")
            .docs(false)
            .label("Popover width")
            .group("behaviour"),
        Descriptor::new("popover.follow_pointer", Kind::Boolean)
            .with_default(true)
            .docs(false)
            .label("Popover follows the pointer")
            .group("behaviour"),
        Descriptor::new("popover.fade_ms", Kind::Number)
            .with_default(90)
            .integer()
            .min(0)
            .max(400)
            .docs(false)
            .label("Popover fade")
            .group("behaviour"),
        Descriptor::new("popover.overflow", Kind::Enum)
            .with_default("clip")
            .allowed(values!["clip", "grow"])
            .docs(false)
            .label("Popover overflow")
            .group("behaviour"),
        Descriptor::new("strip_actions", Kind::List)
            .with_default(Value::List(values!["toggle_sidebar", "new_tab", "open_settings"]))
            .allowed(values!["toggle_sidebar", "new_tab", "open_settings", "search"])
            .list_of(Kind::Enum)
            .shown("`{ \"toggle_sidebar\", \"new_tab\", \"open_settings\" }`")
            .label("Strip actions")
            .group("chrome")
            .values("`\"toggle_sidebar\"` \\| `\"new_tab\"` \\| `\"open_settings\"` \\| `\"search\"` \\| `{ id, icon, on_click }`")
            .help("icon buttons in the top strip, in the order given"),
        Descriptor::new("toggle_button", Kind::Boolean)
            .with_default(true)
            .label("Toggle button")
            .group("chrome")
            .help("draw the collapse glyph in the top strip"),
        Descriptor::new("close_button", Kind::Enum)
            .with_default("hover")
            .allowed(values!["hover", "always", "never"])
            .label("Close button")
            .group("cards")
            .help("when the close glyph is shown"),
        Descriptor::new("confirm_close", Kind::Boolean)
            .with_default(true)
            .label("Confirm close")
            .group("behaviour")
            .help("ask in the sidebar before closing a tab that looks busy; a mux pane reports no process, so it always asks"),
        Descriptor::new("debug", Kind::Boolean)
            .with_default(false)
            .label("Debug logging")
            .group("behaviour")
            .help("log backend events and hit rows"),
        Descriptor::new("show_index", Kind::Boolean)
            .with_default(false)
            .label("Show index")
            .group("cards")
            .help("prefix the tab index; on the meta line with 2-row cards"),
        Descriptor::new("pinned_style", Kind::Enum)
            .with_default("dense")
            .allowed(values!["dense", "compact", "full"])
            .label("Pinned style")
            .group("cards")
            .help("how pinned entries are drawn"),
        Descriptor::new("separator", Kind::Enum)
            .with_default("gap")
            .allowed(values!["rule", "gap", "none"])
            .label("Separator")
            .group("cards")
            .help("between the pinned block and the rest"),
        Descriptor::new("scroll_indicator", Kind::Enum)
            .with_default("auto")
            .allowed(values!["auto", "always", "never"])
            .label("Scroll indicator")
            .group("chrome")
            .values("`\"auto\"` \\| `\"always\"` \\| `\"never\"`")
            .help("right-edge thumb when tabs overflow"),
        Descriptor::new("wheel", Kind::Enum)
            .with_default("scroll")
            .allowed(values!["scroll", "switch"])
            .label("Wheel")
            .group("behaviour")
            .help("what the wheel does over the sidebar"),
        Descriptor::new("tear_off", Kind::Enum)
            .with_default(true)
            .allowed(values![true, false])
            .label("Tear off")
            .group("behaviour")
            .values("`true` \\| `false`")
            .help("drop on the inner edge to move a tab to a new window"),
        Descriptor::new("adopt", Kind::Enum)
            .with_default("auto")
            .allowed(values!["auto", true, false])
            .label("Adopt backend panes")
            .group("identity")
            .values("`\"auto\"` \\| `true` \\| `false`")
            .help("take over an unmapped pane carrying the title marker"),
        Descriptor::new("window_title", Kind::Boolean)
            .with_default(true)
            .label("Window title")
            .group("chrome")
            .help("title the window after the content pane"),
        Descriptor::new("hover", Kind::Enum)
            .with_default("follow")
            .allowed(values!["follow", "press"])
            .host_key("pane_focus_follows_mouse")
            .apply_mode(ApplyMode::Override)
            .label("Hover")
            .group("behaviour")
            .help("when the sidebar holds focus"),
        Descriptor::new("hover_highlight", Kind::Boolean)
            .with_default(true)
            .label("Hover highlight")
            .group("behaviour")
            .help("light the row under the pointer; `false` also stops mouse-motion reports and the menu following the pointer"),
        Descriptor::new("hover_timeout_ms", Kind::Number)
            .with_default(6000)
            .integer()
            .min(0)
            .label("Hover timeout")
            .group("behaviour")
            .help("clear the hover highlight after inactivity; `0` = never"),
        Descriptor::new("tooltip", Kind::Enum)
            .with_default("auto")
            .allowed(values!["auto", "on", "off"])
            .label("Tooltip")
            .group("behaviour")
            .values("`\"auto\"` \\| `\"on\"` \\| `\"off\"`")
            .help("hover tooltip; `auto` needs `hover = \"follow\"`"),
        Descriptor::new("tooltip_delay_ms", Kind::Number)
            .with_default(600)
            .integer()
            .min(0)
            .label("Tooltip delay")
            .group("behaviour")
            .help("hover dwell; effective delay is `max(this, poll_ms)`"),
        Descriptor::new("double_click_ms", Kind::Number)
            .with_default(400)
            .integer()
            .min(0)
            .label("Double click")
            .group("behaviour")
            .help("double-click window on empty space"),
        Descriptor::new("animations", Kind::Enum)
            .with_default("auto")
            .allowed(values!["auto", "on", "off"])
            .label("Animations")
            .group("behaviour")
            .values("`\"auto\"` \\| `\"on\"` \\| `\"off\"`")
            .help("colour fades; the width still changes in one step"),
        Descriptor::new("animation", Kind::Table)
            .container()
            .docs(false)
            .label("Animation")
            .group("behaviour"),
        Descriptor::new("animation.fps", Kind::Number)
            .with_default(30)
            .integer()
            .min(15)
            .max(60)
            .label("Animation fps")
            .group("behaviour")
            .help("backend frame rate for a fade"),
        Descriptor::new("animation.expand_ms", Kind::Number)
            .with_default(220)
            .integer()
            .min(0)
            .label("Expand duration")
            .group("behaviour")
            .help("expand fade duration"),
        Descriptor::new("animation.collapse_ms", Kind::Number)
            .with_default(160)
            .integer()
            .min(0)
            .label("Collapse duration")
            .group("behaviour")
            .help("collapse fade duration"),
        Descriptor::new("animation.hover", Kind::Boolean)
            .with_default(false)
            .label("Animate hover")
            .group("behaviour")
            .help("animate hover transitions"),
        Descriptor::new("ellipsis", Kind::String)
            .with_default("…")
            .label("Ellipsis")
            .group("cards")
            .help("used when truncating titles"),
        Descriptor::new("icons", Kind::Boolean)
            .with_default(true)
            .label("Icons")
            .group("cards")
            .help("show process icons"),
        Descriptor::new("icon_map", Kind::Table)
            .with_default(table([]))
            .open()
            .label("Icon overrides")
            .group("cards")
            .values("table")
            .help("process name to glyph; also overrides UI glyphs"),
        Descriptor::new("title", Kind::Function)
            .label("Title hook")
            .group("hooks")
            .values("`fun(tab, pane): string`")
            .help("custom title: `fun(tab, pane): string`"),
        Descriptor::new("domain", Kind::String)
            .with_default("CurrentPaneDomain")
            .label("Domain")
            .group("identity")
            .help("domain the sidebar pane is spawned in"),
        Descriptor::new("skip_close_confirmation", Kind::Boolean)
            .with_default(true)
            .apply_mode(ApplyMode::Reload)
            .label("Skip close confirmation")
            .group("behaviour")
            .help("add `wez-vtabs` to the skip list"),
        Descriptor::new("private", Kind::Table)
            .container()
            .docs(false)
            .label("Private")
            .group("behaviour"),
        Descriptor::new("private.env", Kind::Table)
            .with_default(table([("HISTFILE", Value::from("")), ("VTABS_PRIVATE", Value::from("1")), ("fish_private_mode", Value::from("1"))]))
            .open()
            .shown("`{ HISTFILE = \"\", fish_private_mode = \"1\", VTABS_PRIVATE = \"1\" }`")
            .label("Private env")
            .group("behaviour")
            .values("table")
            .help("env for shells in private windows"),
        Descriptor::new("keys", Kind::Any)
            .with_default(table([]))
            .open()
            .label("Key overrides")
            .group("behaviour")
            .values("table \\| `false`")
            .help("key overrides; `false` disables all defaults"),
        Descriptor::new("theme", Kind::Table)
            .container()
            .open()
            .shown("`{ elevation = 0.06, split = \"auto\" }`")
            .label("Theme")
            .group("theme")
            .values("see Theme")
            .help("colour overrides"),
        Descriptor::new("theme.elevation", Kind::Number)
            .with_default(0.06)
            .min(0)
            .max(0.3)
            .docs(false)
            .label("Elevation")
            .group("theme"),
        Descriptor::new("theme.split", Kind::String)
            .with_default("auto")
            .docs(false)
            .host_key("colors_split")
            .apply_mode(ApplyMode::Override)
            .label("Split divider")
            .group("theme"),
        Descriptor::new("hooks", Kind::Table)
            .with_default(table([]))
            .container()
            .docs(false)
            .label("Hooks")
            .group("hooks"),
        Descriptor::new("hooks.filter", Kind::Function)
            .label("Filter hook")
            .group("hooks")
            .values("`fun(tab, mux_window): boolean`")
            .help("hide tabs: `fun(tab, mux_window): boolean`"),
        Descriptor::new("hooks.footer", Kind::Function)
            .label("Footer hook")
            .group("hooks")
            .values("`fun(mux_window): rows`")
            .help("sticky bottom rows: `fun(mux_window): (string|FooterEntry)[]`"),
        Descriptor::new("hooks.theme", Kind::Function)
            .label("Theme hook")
            .group("hooks")
            .values("`fun(window, theme): theme`")
            .help("per-window theme: `fun(window, theme): theme`"),
        Descriptor::new("hooks.route", Kind::Function)
            .label("Route hook")
            .group("hooks")
            .values("`fun(meta): space_id`")
            .help("route a tab to a space before the rules run; an undeclared id makes a dynamic space"),
        Descriptor::new("spaces", Kind::List)
            .with_default(Value::List(values![]))
            .list_of(Kind::Table)
            .shown("`{}`")
            .label("Spaces")
            .group("spaces")
            .values("list of `{ id, name, icon, theme, match }`")
            .help("per-window tab groups, each with its own look; the first is the default"),
        Descriptor::new("backend", Kind::Table)
            .container()
            .docs(false)
            .label("Backend")
            .group("backend"),
        Descriptor::new("backend.path", Kind::Any)
            .apply_mode(ApplyMode::Reload)
            .label("Backend path")
            .group("backend")
            .values("string \\| string[] \\| table \\| `fun(domain, host)`")
            .help("`wez-vtabs` paths; the machine that runs the split execs the first one it has"),
        Descriptor::new("backend.repo", Kind::String)
            .with_default("fredrir/wezterm-vertical-tabs")
            .apply_mode(ApplyMode::Reload)
            .label("Backend repo")
            .group("backend")
            .help("GitHub repo used for release downloads"),
        Descriptor::new("backend.build", Kind::Boolean)
            .with_default(true)
            .label("Backend build")
            .group("backend")
            .help("fall back to `cargo build` when no release matches"),
        Descriptor::new("backend.uservar", Kind::String)
            .with_default("vtabs")
            .apply_mode(ApplyMode::Reload)
            .label("Backend user var")
            .group("backend")
            .help("user var name used by the backend"),
        Descriptor::new("backend.env", Kind::Table)
            .with_default(table([]))
            .open()
            .label("Backend env")
            .group("backend")
            .values("table")
            .help("env for the sidebar process; `VTABS_*` keys the plugin sets win"),
        Descriptor::new("backend.inbox", Kind::Boolean)
            .with_default(true)
            .label("Backend inbox")
            .group("backend")
            .help("offer the inbox transport to same-machine mux sidebars; `false` keeps every pane on stdin"),
    ]
}

static OPTIONS: LazyLock<Vec<Descriptor>> = LazyLock::new(build_options);

pub fn options() -> &'static [Descriptor] {
    &OPTIONS
}

/// Stable identity for every descriptor fact shared with the boot normalizer's generated Lua
/// mirror. FNV-1a is an identity checksum, not a security primitive.
pub fn identity() -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in format!("{:#?}", options()).bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn by_key(key: &str) -> Option<&'static Descriptor> {
    options().iter().find(|option| option.key == key)
}

/// Returns the descriptor that owns an exact setting path's live-application policy. Open
/// containers may register only the few children that affect a host setting; unrelated children
/// deliberately keep the default instant policy.
pub fn policy_for(key: &str) -> Option<&'static Descriptor> {
    by_key(key).or_else(|| {
        options()
            .iter()
            .find(|option| option.policy_paths.contains(&key))
    })
}

pub fn is_open(key: &str) -> bool {
    let mut prefix = String::new();
    for part in key.split('.') {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(part);
        if prefix != key && by_key(&prefix).is_some_and(|option| option.open) {
            return true;
        }
    }
    false
}

pub fn defaults() -> Value {
    let mut root = Value::Table(BTreeMap::new());
    for option in options() {
        if let Some(value) = &option.default {
            set_path(&mut root, option.key, value.clone());
        }
    }
    root
}

/// Checks a value's kind, enumeration, and numeric bounds.
pub fn canonical_value(option: &Descriptor, value: &Value) -> Option<Value> {
    accepts(option, value).then_some(value.clone())
}

fn accepts(option: &Descriptor, value: &Value) -> bool {
    match option.kind {
        Kind::Any => true,
        Kind::Boolean => matches!(value, Value::Bool(_)),
        Kind::String => matches!(value, Value::String(_)),
        Kind::Table => matches!(value, Value::Table(_)),
        Kind::Function => false,
        Kind::Enum => option.allowed.contains(value),
        Kind::Number => match value {
            Value::Number(number) => {
                number.is_finite()
                    && (!option.integer || number.fract() == 0.0)
                    && option.min.is_none_or(|min| *number >= min)
                    && option.max.is_none_or(|max| *number <= max)
            }
            _ => false,
        },
        Kind::List => match value {
            Value::List(items) => items.iter().all(|item| match option.list_of {
                Some(Kind::Enum) => {
                    option.allowed.contains(item)
                        || matches!(item, Value::Table(entry) if matches!(entry.get("id"), Some(Value::String(_))))
                }
                Some(Kind::Table) => matches!(item, Value::Table(_)),
                _ => false,
            }),
            _ => false,
        },
    }
}

pub fn validate_schema() -> Result<(), String> {
    let mut keys = BTreeSet::new();
    let mut policy_paths = BTreeSet::new();
    for option in options() {
        if !keys.insert(option.key) {
            return Err(format!("duplicate descriptor {}", option.key));
        }
        if option.label.is_none() || option.group.is_none() {
            return Err(format!("{} needs a label and group", option.key));
        }
        if option.kind == Kind::Enum && option.allowed.is_empty() {
            return Err(format!("{} has no enum values", option.key));
        }
        if let Some(default) = &option.default
            && canonical_value(option, default).as_ref() != Some(default)
        {
            return Err(format!("{} has an invalid default", option.key));
        }
        if option.apply_mode == ApplyMode::Override && option.host_key.is_none() {
            return Err(format!(
                "{} uses host override mode without a host key",
                option.key
            ));
        }
        if !option.policy_paths.is_empty() && option.host_key.is_none() {
            return Err(format!(
                "{} has policy paths without a host key",
                option.key
            ));
        }
        for path in &option.policy_paths {
            if *path == option.key || by_key(path).is_some() || !policy_paths.insert(*path) {
                return Err(format!("{} has duplicate policy path {path}", option.key));
            }
        }
    }
    let defaults = defaults();
    for option in options() {
        if option.default.is_some()
            && !is_open(option.key)
            && get_path(&defaults, option.key).is_none()
        {
            return Err(format!("{} default was not built", option.key));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/settings/schema.rs"]
mod tests;

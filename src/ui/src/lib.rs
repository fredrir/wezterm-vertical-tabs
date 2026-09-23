//! Event-driven in-memory Ratatui UI. The host publishes complete frames atomically and
//! schedules only `next_deadline`; this crate performs no terminal, mux, or storage I/O.
mod icons;
mod input;
mod interaction;
mod render;
mod settings_page;
mod shortcuts;
mod sidebar;
mod theme;

pub use input::{EditResult, Key, Modifiers, MouseButton, TextEditor, UiInput};
pub use ratatui::{buffer::Buffer, layout::Rect};
pub use shortcuts::is_shortcut;
pub use theme::Theme;

#[cfg(test)]
#[path = "../tests/input.rs"]
mod input_tests;

#[cfg(test)]
#[path = "../tests/interaction_flow.rs"]
mod interaction_flow_tests;

#[cfg(test)]
#[path = "../tests/settings_page.rs"]
mod settings_page_tests;

#[cfg(test)]
#[path = "../tests/transient_surfaces.rs"]
mod transient_surfaces_tests;

#[cfg(test)]
#[path = "../tests/ui.rs"]
mod ui_tests;

use ratatui::layout::Position;
use std::time::Duration;
use tachyonfx::{Effect, fx};
use vtabs_core::{Intent, Model, PaneId, SpaceId, Tab, TabId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ElementId {
    SpaceTitle,
    Search,
    Refresh,
    CreateFolder,
    Folder(String),
    SettingsCategory(String),
    SettingsSearch,
    CloseSettings,
    ResetSettings,
    CreateSpace,
    NewTab,
    Settings,
    SettingsTab,
    CloseSettingsTab,
    Rail,
    Space(SpaceId),
    Tab(TabId),
    Pane(TabId, PaneId),
    ClosePane(TabId, PaneId),
    CloseTab(TabId),
    Menu(String),
    Setting(String),
    Editor,
    Submit,
    Cancel,
}

impl ElementId {
    /// Controls nested in a row share its hover, press and drag identity.
    pub(crate) fn row(&self) -> ElementId {
        match self {
            Self::CloseTab(id) | Self::Pane(id, _) | Self::ClosePane(id, _) => Self::Tab(*id),
            Self::CloseSettingsTab => Self::SettingsTab,
            Self::CreateFolder => Self::SpaceTitle,
            other => other.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HitRegion {
    pub id: ElementId,
    pub rect: Rect,
    pub tooltip: String,
}

#[derive(Clone, Debug)]
pub enum HostAction {
    /// Custom menu actions remain semantic; the adapter resolves a registered Lua action.
    Custom(String),
    MoveTabToNewWindow(TabId),
    /// The host closes an idle tab directly and asks `confirm_close_tab` for a busy one.
    CloseTab(TabId),
    FocusPane(TabId, PaneId),
    /// The host closes an idle split directly and asks `confirm_close_pane` for a busy one.
    ClosePane(TabId, PaneId),
    KillPane(TabId, PaneId),
    /// A pane dragged out of its tab becomes a tab of its own, optionally at an index.
    DetachPane {
        tab: TabId,
        pane: PaneId,
        index: Option<usize>,
    },
    /// A pane dropped inside another tab becomes one of its splits.
    JoinPane {
        pane: PaneId,
        tab: TabId,
    },
    /// A tab dropped inside another tab hands over every pane it has.
    JoinTab {
        source: TabId,
        tab: TabId,
    },
    /// The host asks the owning window to show a tab, reattaching its domain if needed.
    ShowTab {
        window: u64,
        tab: TabId,
    },
}

/// A tab outside this window's sidebar: another window's, or one a detached domain took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignTab {
    pub window: u64,
    pub id: TabId,
    pub label: String,
    pub title: String,
    pub place: String,
    pub remote: bool,
    pub os: String,
}
impl ForeignTab {
    pub fn new(window: u64, tab: &Tab, home: Option<&str>, place: impl Into<String>) -> Self {
        Self {
            window,
            id: tab.id,
            label: tab_name(tab, home).unwrap_or_else(|| tab.title.clone()),
            title: tab.title.clone(),
            place: place.into(),
            remote: tab.remote,
            os: tab.os.clone(),
        }
    }
}

fn tab_name(tab: &Tab, home: Option<&str>) -> Option<String> {
    tab.custom_title()
        .map(str::to_owned)
        .or_else(|| tab.location(home))
}

#[derive(Clone, Debug)]
pub enum UiIntent {
    Refresh,
    Domain(Intent),
    SetClipboard(String),
    RequestClipboard,
    Host(HostAction),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceTransform {
    /// Fraction of the surface width. Applied by the compositor, never to pane sizes.
    pub translate_x: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct FrameUpdate {
    pub revision: u64,
    pub resized: bool,
    pub changed_cells: Vec<(u16, u16)>,
    pub dirty_rows: Vec<u16>,
    pub cursor: Option<Position>,
    /// Rows the caret moves down to follow text the host centers in a two-row surface.
    pub cursor_shift: f32,
    pub ime_rect: Option<Rect>,
    pub transform: SurfaceTransform,
}

#[derive(Clone, Debug)]
enum Action {
    Domain(Intent),
    NewSpace,
    NewFolder,
    RenameFolder(String),
    MoveToFolder(TabId),
    RenameSpace(SpaceId),
    EditSpaceIcon(SpaceId),
    EditSpaceAccent(SpaceId),
    EditSpaceRules(SpaceId),
    DeleteSpace(SpaceId),
    RenameTab(TabId),
    MoveTab(TabId),
    CloseTab(TabId),
    KillPane(TabId, PaneId),
    Host(HostAction),
    Settings,
    CloseSettings,
    EditSetting(String),
    ResetSetting(String),
    EditorCommand { key: Key, target: ElementId },
    Submenu { title: String, items: Vec<MenuItem> },
    Confirm { label: String, action: Box<Action> },
    Close,
}

#[derive(Clone, Debug)]
struct MenuItem {
    id: String,
    /// Palette rows share the sidebar's icon slot and index badge.
    icon: &'static str,
    icon_color: Option<ratatui::style::Color>,
    index: Option<usize>,
    label: String,
    hint: String,
    /// Matched by search but not shown.
    keywords: String,
    action: Action,
    enabled: bool,
}

impl MenuItem {
    fn new(id: impl Into<String>, label: impl Into<String>, action: Action) -> Self {
        Self {
            id: id.into(),
            icon: "",
            icon_color: None,
            index: None,
            label: label.into(),
            hint: String::new(),
            keywords: String::new(),
            action,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug)]
struct Menu {
    title: String,
    /// A confirmation: the title asks, this explains, and the items read as buttons.
    message: Option<String>,
    items: Vec<MenuItem>,
    selected: usize,
    scroll: usize,
    search: Option<MenuSearch>,
}

#[derive(Clone, Debug)]
struct MenuSearch {
    editor: TextEditor,
    all_items: Vec<MenuItem>,
}

#[derive(Clone, Debug)]
enum FormKind {
    CreateFolder,
    RenameFolder(String),
    CreateSpace,
    RenameSpace(SpaceId),
    SpaceIcon(SpaceId),
    SpaceAccent(SpaceId),
    SpaceRules(SpaceId),
    RenameTab(TabId),
    Setting(String),
}

#[derive(Clone, Debug)]
struct Form {
    title: String,
    kind: FormKind,
    editor: TextEditor,
    error: Option<String>,
}

#[derive(Clone, Debug)]
enum Overlay {
    Menu(Menu),
    Form(Form),
}

/// Stamps resolve at the next render; input events carry no clock of their own.
#[derive(Clone, Debug)]
struct Press {
    id: ElementId,
    down: Option<Duration>,
    up: Option<Option<Duration>>,
    level: f32,
    animating: bool,
}

/// Where a drag would land, resolved on every pointer move so the preview never lies.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DropTarget {
    /// Reorder next to a tab; a dragged pane becomes a tab of its own there.
    Beside {
        tab: TabId,
        after: bool,
    },
    /// Become a split of this tab.
    Into(TabId),
    Folder(String),
    /// Leave folders and pins behind; a dragged pane becomes the last tab.
    NewTab,
    Space(SpaceId),
}

/// The drop preview glides between targets instead of jumping.
#[derive(Clone, Copy, Debug)]
struct DropMotion {
    from: f32,
    to: f32,
    start: Option<Duration>,
    progress: f32,
}

/// The tab Settings follows, and the position that anchor last gave it.
#[derive(Clone, Copy, Debug)]
struct SettingsPlace {
    anchor: Option<TabId>,
    slot: usize,
}

#[derive(Clone, Debug)]
struct InlineRename {
    id: TabId,
    initial: String,
    editor: TextEditor,
}

#[derive(Clone, Copy, Debug)]
struct Motion {
    from: f32,
    to: f32,
    start: Duration,
    duration: Duration,
}

/// The UI retains allocated buffers and composes only after semantic invalidation.
/// `Model::revision` must change with model data. `invalidate` handles external style/focus
/// changes; terminal repaint alone does not invalidate the sidebar.
pub struct SidebarUi {
    buffer: Buffer,
    staging: Buffer,
    revision: Option<u64>,
    frame_revision: u64,
    dirty: bool,
    pub theme: Theme,
    rounded_surfaces: Vec<RoundedSurface>,
    sidebar_columns: Option<u16>,
    header_inset: u16,
    settings_page: bool,
    settings_tab: bool,
    settings_place: Option<SettingsPlace>,
    reveal_settings: bool,
    settings_category: String,
    settings_query: TextEditor,
    settings_selected: usize,
    settings_scroll: usize,
    settings_search_focused: bool,
    page_rect: Rect,
    sidebar_rect: Rect,
    sidebar_rows: Vec<SidebarRow>,
    sidebar_revision: Option<(u64, bool)>,
    /// Sidebar-relative origin for the next context menu; sidebar placement changes
    /// between the sidebar-only grid and the window viewport.
    anchor: Option<Position>,
    pointer_origin: Option<(u16, u16)>,
    dragging: bool,
    hits: Vec<HitRegion>,
    focused: Option<ElementId>,
    hovered: Option<ElementId>,
    drag: Option<ElementId>,
    press: Option<Press>,
    drop: Option<DropTarget>,
    drop_motion: Option<DropMotion>,
    drag_label: String,
    /// Where inside its cell the pointer sits; rows are too short to zone by cells alone.
    pointer_fraction: (f32, f32),
    rename: Option<InlineRename>,
    title_rects: Vec<(TabId, Rect)>,
    last_click: Option<(ElementId, Duration)>,
    overlay: Option<Overlay>,
    overlay_stack: Vec<Overlay>,
    restore_focus: Option<ElementId>,
    tab_scroll: usize,
    space_scroll: usize,
    tabs_rect: Rect,
    spaces_rect: Rect,
    overlay_rect: Rect,
    editor_rect: Rect,
    editor_shift: f32,
    cursor: Option<Position>,
    effect: Option<Effect>,
    effect_area: Option<Rect>,
    motion: Option<Motion>,
    last_frame: Duration,
    now: Duration,
    visible: bool,
    window_focused: bool,
    caret_visible: bool,
    caret_deadline: Option<Duration>,
    tooltip_deadline: Option<Duration>,
    show_tooltip: bool,
    config_owned: std::collections::BTreeSet<String>,
    space_activity: std::collections::BTreeSet<SpaceId>,
    last_selected_tab: Option<TabId>,
    last_selected_space: Option<SpaceId>,
    pending_form: Option<u64>,
    last_rail: Option<vtabs_core::RailMode>,
    reveal_selection: bool,
    foreign_tabs: Vec<ForeignTab>,
}

#[derive(Clone, Debug)]
pub struct RoundedSurface {
    pub rect: Rect,
    pub fill: ratatui::style::Color,
    pub radius: f32,
    pub inset: f32,
    /// Icon buttons center a square within their cells; rows keep their full extent.
    pub square: bool,
    /// Fraction of the rect's height to draw, about its center; thin bars need less than a cell.
    pub scale_y: f32,
    /// Holds one text line per cell row, so the host centers nothing beneath it.
    pub stacked: bool,
    /// Rows to move down; marks inside host-centered text follow it by half a row.
    pub shift_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarRow {
    Tab {
        id: TabId,
        number: usize,
    },
    Folder {
        index: usize,
        count: usize,
    },
    /// Separates the pinned and folder group from the open tabs.
    Gap,
    NewTab,
    Settings {
        number: usize,
    },
}

fn running(process: &str) -> String {
    if process.is_empty() {
        "A process is still running.".into()
    } else {
        format!("{process} is still running.")
    }
}

pub type Ui = SidebarUi;

impl Default for SidebarUi {
    fn default() -> Self {
        Self::new()
    }
}

impl SidebarUi {
    pub fn new() -> Self {
        Self {
            buffer: Buffer::empty(Rect::default()),
            staging: Buffer::empty(Rect::default()),
            revision: None,
            frame_revision: 0,
            dirty: true,
            theme: Theme::default(),
            rounded_surfaces: Vec::new(),
            sidebar_columns: None,
            header_inset: 0,
            settings_page: false,
            settings_tab: false,
            settings_place: None,
            reveal_settings: false,
            settings_category: "all".into(),
            settings_query: TextEditor::default(),
            settings_selected: 0,
            settings_scroll: 0,
            settings_search_focused: false,
            page_rect: Rect::default(),
            sidebar_rect: Rect::default(),
            sidebar_rows: Vec::new(),
            sidebar_revision: None,
            anchor: None,
            pointer_origin: None,
            dragging: false,
            hits: Vec::new(),
            focused: None,
            hovered: None,
            drag: None,
            press: None,
            drop: None,
            drop_motion: None,
            drag_label: String::new(),
            pointer_fraction: (0.5, 0.5),
            rename: None,
            title_rects: Vec::new(),
            last_click: None,
            overlay: None,
            overlay_stack: Vec::new(),
            restore_focus: None,
            tab_scroll: 0,
            space_scroll: 0,
            tabs_rect: Rect::default(),
            spaces_rect: Rect::default(),
            overlay_rect: Rect::default(),
            editor_rect: Rect::default(),
            editor_shift: 0.0,
            cursor: None,
            effect: None,
            effect_area: None,
            motion: None,
            last_frame: Duration::ZERO,
            now: Duration::ZERO,
            visible: true,
            window_focused: true,
            caret_visible: true,
            caret_deadline: None,
            tooltip_deadline: None,
            show_tooltip: false,
            config_owned: Default::default(),
            space_activity: Default::default(),
            last_selected_tab: None,
            last_selected_space: None,
            pending_form: None,
            last_rail: None,
            reveal_selection: true,
            foreign_tabs: Vec::new(),
        }
    }
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }
    pub fn hit_regions(&self) -> &[HitRegion] {
        &self.hits
    }
    pub fn focused(&self) -> Option<&ElementId> {
        self.focused.as_ref()
    }
    pub fn is_modal(&self) -> bool {
        self.overlay.is_some() || self.settings_page
    }
    /// Settings retain a sidebar beside their content page. Transient surfaces use the
    /// window viewport without changing the user's sidebar reservation.
    pub fn needs_expanded_space(&self) -> bool {
        self.settings_page
    }
    pub fn has_focus(&self) -> bool {
        self.focused.is_some() || self.is_modal()
    }
    /// Call when content receives focus; this does not mark the OS window unfocused.
    pub fn release_focus(&mut self) {
        self.hide_settings();
        if self.rename.take().is_some() {
            self.caret_deadline = None;
            self.dirty = true;
        }
        self.focused = None;
        self.drag = None;
    }
    /// The host found a running process; the prompt names it when known.
    pub fn confirm_close_tab(&mut self, id: TabId, process: &str) {
        self.dismiss();
        self.confirm(
            "Close tab?",
            running(process),
            "Close",
            Action::Domain(Intent::CloseTab(id)),
        );
    }
    pub fn confirm_close_pane(&mut self, tab: TabId, pane: PaneId, process: &str) {
        self.dismiss();
        self.confirm(
            "Close split?",
            running(process),
            "Close",
            Action::KillPane(tab, pane),
        );
    }
    /// Every confirmation shares one dialog with the accepting button preselected.
    fn confirm(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        accept: &str,
        action: Action,
    ) {
        if let Some(overlay) = self.overlay.take() {
            self.overlay_stack.push(overlay);
        }
        self.open_overlay(Overlay::Menu(Menu {
            title: title.into(),
            message: Some(message.into()),
            items: vec![
                MenuItem::new("cancel", "Cancel", Action::Close),
                MenuItem::new("confirm", accept, action),
            ],
            selected: 1,
            scroll: 0,
            search: None,
        }));
    }
    /// Rows the tab search lists after this window's own tabs.
    pub fn set_foreign_tabs(&mut self, tabs: Vec<ForeignTab>) {
        self.foreign_tabs = tabs;
    }
    /// The current space leads with its index badges; every other tab this window can reach follows.
    pub fn open_tab_navigator(&mut self, model: &Model) {
        let home = model.home.as_deref();
        let row = |tab: &Tab| {
            let name = tab_name(tab, home);
            let mut item = MenuItem::new(
                format!("tab/{}", tab.id),
                name.as_deref().unwrap_or(&tab.title),
                Action::Domain(Intent::ActivateTab(tab.id)),
            );
            let (remote, os) = tab
                .panes
                .iter()
                .find(|pane| pane.active)
                .map_or((tab.remote, &tab.os), |pane| (pane.remote, &pane.os));
            item.icon = icons::host(remote, os);
            item.icon_color = self.theme.host(remote, os);
            if name.is_some_and(|name| name != tab.title) {
                item.keywords = tab.title.clone();
            }
            item
        };
        let mut items: Vec<_> = model
            .visible_ids()
            .iter()
            .filter_map(|id| model.tabs.get(id))
            .enumerate()
            .map(|(index, tab)| {
                let mut item = row(tab);
                item.index = Some(index + 1);
                item.hint = item.keywords.clone();
                item
            })
            .collect();
        let current = model.selected_space.as_str();
        for space in model.spaces.iter() {
            for id in model.space_tabs(&space.id) {
                let hidden = model.is_hidden(id);
                if space.id == current && !hidden {
                    continue;
                }
                let mut item = row(&model.tabs[&id]);
                item.hint = if hidden {
                    format!("{} · hidden", space.name)
                } else {
                    space.name.clone()
                };
                items.push(item);
            }
        }
        items.extend(self.foreign_tabs.iter().map(|tab| {
            let mut item = MenuItem::new(
                format!("window/{}/tab/{}", tab.window, tab.id),
                &tab.label,
                Action::Host(HostAction::ShowTab {
                    window: tab.window,
                    tab: tab.id,
                }),
            );
            item.icon = icons::host(tab.remote, &tab.os);
            item.icon_color = self.theme.host(tab.remote, &tab.os);
            item.hint = tab.place.clone();
            if tab.label != tab.title {
                item.keywords = tab.title.clone();
            }
            item
        }));
        if model.settings.rail != vtabs_core::RailMode::Expanded {
            let mut item = MenuItem::new(
                "sidebar/expand",
                if model.settings.rail == vtabs_core::RailMode::Hidden {
                    "Show sidebar"
                } else {
                    "Expand sidebar"
                },
                Action::Domain(Intent::SetRail(vtabs_core::RailMode::Expanded)),
            );
            item.icon = icons::SIDEBAR;
            items.push(item);
        }
        let search = Some(MenuSearch {
            editor: TextEditor::default(),
            all_items: items.clone(),
        });
        self.open_overlay(Overlay::Menu(Menu {
            message: None,
            title: "Search tabs".into(),
            items,
            selected: model
                .selected_tab
                .and_then(|id| model.visible_ids().iter().position(|tab| *tab == id))
                .unwrap_or(0),
            scroll: 0,
            search,
        }));
    }
    pub fn show_error(&mut self, message: impl Into<String>) {
        self.open_overlay(Overlay::Menu(Menu {
            message: None,
            title: message.into(),
            items: vec![MenuItem::new("dismiss", "Dismiss", Action::Close)],
            selected: 0,
            scroll: 0,
            search: None,
        }));
    }
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.pending_form = None;
        if let Some(Overlay::Form(form)) = &mut self.overlay {
            form.error = Some(message.into());
            self.dirty = true;
        } else {
            self.show_error(message);
        }
    }
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn set_config_owned(&mut self, keys: impl IntoIterator<Item = String>) {
        let keys = keys.into_iter().collect();
        if self.config_owned != keys {
            self.config_owned = keys;
            self.invalidate();
        }
    }
    pub fn hit_test(&self, x: u16, y: u16) -> Option<&HitRegion> {
        self.hits
            .iter()
            .rev()
            .find(|hit| hit.rect.contains(Position::new(x, y)))
    }
    pub fn next_deadline(&self) -> Option<Duration> {
        if !self.visible || !self.window_focused || self.buffer.area.is_empty() {
            return None;
        }
        [
            self.effect
                .as_ref()
                .map(|_| self.last_frame + Duration::from_millis(8)),
            self.motion
                .map(|_| self.last_frame + Duration::from_millis(8)),
            self.press
                .as_ref()
                .filter(|press| press.animating)
                .map(|_| self.last_frame + Duration::from_millis(8)),
            self.drop_motion
                .filter(|motion| motion.progress < 1.0)
                .map(|_| self.last_frame + Duration::from_millis(8)),
            self.caret_deadline,
            self.tooltip_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }
    pub fn has_animation(&self) -> bool {
        self.effect.is_some()
            || self.motion.is_some()
            || self.press.as_ref().is_some_and(|press| press.animating)
            || self.drop_motion.is_some_and(|motion| motion.progress < 1.0)
    }
    /// Hosts report the pointer's place within its cell so short rows can tell edge from middle.
    pub fn set_pointer_fraction(&mut self, x: f32, y: f32) {
        self.pointer_fraction = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
    }
    /// Hosts stamp pointer input so click timing does not depend on repaint cadence.
    pub fn set_clock(&mut self, now: Duration) {
        self.now = self.now.max(now);
    }
    pub fn cancel_effects(&mut self) {
        let effect = self.effect.take().is_some();
        let motion = self.motion.take().is_some();
        if effect || motion {
            self.dirty = true;
        }
        self.effect_area = None;
    }
    fn start_effect(&mut self, model: &Model) {
        self.effect = None;
        self.effect_area = None;
        if model.settings.animations
            && !model.settings.reduced_motion
            && model.settings.animation_ms > 0
            && self.visible
            && self.window_focused
        {
            let Some(area) = self
                .hovered
                .as_ref()
                .or(self.focused.as_ref())
                .and_then(|id| self.hits.iter().find(|hit| &hit.id == id))
                .map(|hit| hit.rect)
            else {
                return;
            };
            self.effect = Some(fx::fade_from_fg(
                self.theme.muted,
                u32::from(model.settings.animation_ms),
            ));
            self.effect_area = Some(area);
        }
    }
    /// Animate only the surface. The caller has already committed the final pane
    /// reservation and must not derive content geometry from this visual transform.
    pub fn transition_surface(&mut self, from: f32, to: f32, now: Duration, duration: Duration) {
        self.motion = (duration > Duration::ZERO).then_some(Motion {
            from,
            to,
            start: now,
            duration,
        });
        self.now = now;
        self.dirty = true;
    }
    pub fn dismiss(&mut self) {
        self.overlay = None;
        self.pending_form = None;
        self.overlay_stack.clear();
        self.anchor = None;
        self.focused = self.restore_focus.take();
        self.caret_deadline = None;
        self.show_tooltip = false;
        self.tooltip_deadline = None;
        self.cancel_effects();
        self.dirty = true;
    }
    /// Settings behave like a tab: the row stays listed while another tab is active.
    pub fn open_settings(&mut self) {
        self.dismiss();
        self.settings_page = true;
        self.reveal_settings = !self.settings_tab;
        self.settings_tab = true;
        self.settings_search_focused = false;
        self.dirty = true;
    }
    pub fn hide_settings(&mut self) {
        if self.settings_page {
            self.settings_page = false;
            self.settings_search_focused = false;
            self.dirty = true;
        }
    }
    pub fn close_settings(&mut self) {
        self.settings_page = false;
        self.settings_tab = false;
        self.settings_place = None;
        self.settings_search_focused = false;
        self.dismiss();
        self.focused = None;
    }
    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }
    pub fn content_page(&self) -> bool {
        self.settings_page
    }
    /// Transient UI uses the window viewport while the terminal remains visible. Reserve
    /// that viewport while a tooltip is pending so it can extend past the sidebar edge.
    pub fn overlay_surface(&self) -> bool {
        self.overlay.is_some() || self.show_tooltip || self.tooltip_deadline.is_some()
    }
    pub fn rounded_surfaces(&self) -> &[RoundedSurface] {
        &self.rounded_surfaces
    }
    pub fn set_layout(&mut self, sidebar_columns: u16, header_inset: u16) {
        if self.sidebar_columns != Some(sidebar_columns) || self.header_inset != header_inset {
            self.sidebar_columns = Some(sidebar_columns);
            self.header_inset = header_inset;
            self.invalidate();
        }
    }
    pub fn open_create_folder(&mut self) {
        self.open_form("New folder", FormKind::CreateFolder, "");
    }
    pub fn open_create_space(&mut self) {
        self.open_form("Create space", FormKind::CreateSpace, "");
    }
    fn open_overlay(&mut self, overlay: Overlay) {
        self.cancel_effects();
        self.caret_deadline = None;
        self.caret_visible = true;
        self.show_tooltip = false;
        self.tooltip_deadline = None;
        if self.overlay.is_none() && self.overlay_stack.is_empty() {
            self.restore_focus = self.focused.clone();
        }
        self.overlay = Some(overlay);
        self.dirty = true;
    }
    fn open_form(&mut self, title: impl Into<String>, kind: FormKind, value: &str) {
        if let Some(overlay) = self.overlay.take() {
            self.overlay_stack.push(overlay);
        }
        let mut editor = TextEditor::new(value);
        editor.select_all();
        self.open_overlay(Overlay::Form(Form {
            title: title.into(),
            kind,
            editor,
            error: None,
        }));
        self.focused = Some(ElementId::Editor);
        self.reset_caret();
    }
    fn reset_caret(&mut self) {
        self.caret_visible = true;
        self.caret_deadline =
            (self.visible && self.window_focused).then_some(self.now + Duration::from_millis(600));
    }
    fn back(&mut self) {
        if let Some(previous) = self.overlay_stack.pop() {
            self.overlay = Some(previous);
            self.caret_deadline = None;
            self.dirty = true;
        } else {
            self.dismiss();
        }
    }
}

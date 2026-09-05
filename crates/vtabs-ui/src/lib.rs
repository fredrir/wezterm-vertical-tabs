//! Event-driven in-memory Ratatui UI. The host publishes complete frames atomically and
//! schedules only `next_deadline`; this crate performs no terminal, mux, or storage I/O.
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

use ratatui::layout::Position;
use std::time::Duration;
use tachyonfx::{Effect, fx};
use vtabs_core::{Intent, Model, SpaceId, TabId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ElementId {
    PrivateInfo,
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
    Rail,
    Space(SpaceId),
    Tab(TabId),
    CloseTab(TabId),
    Menu(String),
    Setting(String),
    Editor,
    Submit,
    Cancel,
}

#[derive(Clone, Debug)]
pub struct HitRegion {
    pub id: ElementId,
    pub rect: Rect,
    pub tooltip: String,
}

#[derive(Clone, Debug)]
pub enum NativeUiAction {
    /// Custom menu actions remain semantic; the adapter resolves a registered Lua action.
    Custom(String),
    MoveTabToNewWindow(TabId),
}

#[derive(Clone, Debug)]
pub enum UiIntent {
    Refresh,
    Domain(Intent),
    SetClipboard(String),
    RequestClipboard,
    Native(NativeUiAction),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceTransform {
    /// Fraction of the surface width. Applied by the native compositor, never to pane sizes.
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
    Settings,
    EditSetting(String),
    ResetSetting(String),
    Submenu { title: String, items: Vec<MenuItem> },
    Confirm { label: String, action: Box<Action> },
    Close,
}

#[derive(Clone, Debug)]
struct MenuItem {
    id: String,
    label: String,
    hint: String,
    action: Action,
    enabled: bool,
}

impl MenuItem {
    fn new(id: impl Into<String>, label: impl Into<String>, action: Action) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            hint: String::new(),
            action,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug)]
struct Menu {
    title: String,
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

#[derive(Clone, Copy, Debug)]
struct Motion {
    from: f32,
    to: f32,
    start: Duration,
    duration: Duration,
}

/// The UI retains allocated buffers and composes only after semantic invalidation.
/// `Model::revision` must change with model data. `invalidate` handles external style/focus
/// changes; native terminal repaint alone does not invalidate the sidebar.
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
    settings_category: String,
    settings_query: TextEditor,
    settings_selected: usize,
    settings_scroll: usize,
    settings_search_focused: bool,
    page_rect: Rect,
    sidebar_rect: Rect,
    sidebar_rows: Vec<SidebarRow>,
    pointer_origin: Option<(u16, u16)>,
    dragging: bool,
    hits: Vec<HitRegion>,
    focused: Option<ElementId>,
    hovered: Option<ElementId>,
    drag: Option<ElementId>,
    overlay: Option<Overlay>,
    overlay_stack: Vec<Overlay>,
    restore_focus: Option<ElementId>,
    tab_scroll: usize,
    space_scroll: usize,
    tabs_rect: Rect,
    spaces_rect: Rect,
    overlay_rect: Rect,
    editor_rect: Rect,
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
}

#[derive(Clone, Debug)]
pub struct RoundedSurface {
    pub rect: Rect,
    pub fill: ratatui::style::Color,
    pub border: ratatui::style::Color,
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SidebarRow {
    Tab(TabId),
    Folder(String),
    NewTab,
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
            settings_category: "all".into(),
            settings_query: TextEditor::default(),
            settings_selected: 0,
            settings_scroll: 0,
            settings_search_focused: false,
            page_rect: Rect::default(),
            sidebar_rect: Rect::default(),
            sidebar_rows: Vec::new(),
            pointer_origin: None,
            dragging: false,
            hits: Vec::new(),
            focused: None,
            hovered: None,
            drag: None,
            overlay: None,
            overlay_stack: Vec::new(),
            restore_focus: None,
            tab_scroll: 0,
            space_scroll: 0,
            tabs_rect: Rect::default(),
            spaces_rect: Rect::default(),
            overlay_rect: Rect::default(),
            editor_rect: Rect::default(),
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
    /// Forms and menus need usable width even when the user's rail preference is hidden.
    pub fn needs_expanded_space(&self) -> bool {
        self.is_modal()
    }
    pub fn has_focus(&self) -> bool {
        self.focused.is_some() || self.is_modal()
    }
    /// Call when native content receives focus; this does not mark the OS window unfocused.
    pub fn release_focus(&mut self) {
        self.close_settings();
        self.focused = None;
        self.drag = None;
    }
    pub fn open_tab_navigator(&mut self, model: &Model) {
        let mut items: Vec<_> = model
            .visible_ids()
            .iter()
            .filter_map(|id| model.tabs.get(id))
            .enumerate()
            .map(|(index, tab)| {
                MenuItem::new(
                    format!("tab/{}", tab.id),
                    format!("{} {}", index + 1, tab.display_title()),
                    Action::Domain(Intent::ActivateTab(tab.id)),
                )
            })
            .collect();
        if model.settings.rail != vtabs_core::RailMode::Expanded {
            let owned = model.config_owned.contains("rail");
            let mut item = MenuItem::new(
                "sidebar/expand",
                if owned {
                    "Rail controlled by Lua"
                } else if model.settings.rail == vtabs_core::RailMode::Hidden {
                    "Show sidebar"
                } else {
                    "Expand sidebar"
                },
                Action::Domain(Intent::SetRail(vtabs_core::RailMode::Expanded)),
            );
            item.enabled = !owned;
            items.push(item);
        }
        let search = Some(MenuSearch {
            editor: TextEditor::default(),
            all_items: items.clone(),
        });
        self.open_overlay(Overlay::Menu(Menu {
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
            self.caret_deadline,
            self.tooltip_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }
    pub fn has_animation(&self) -> bool {
        self.effect.is_some() || self.motion.is_some()
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
        if model.settings.animations
            && !model.settings.reduced_motion
            && model.settings.animation_ms > 0
            && self.visible
            && self.window_focused
        {
            self.effect = Some(fx::fade_from_fg(
                self.theme.muted,
                u32::from(model.settings.animation_ms),
            ));
            self.effect_area = self
                .focused
                .as_ref()
                .and_then(|id| self.hits.iter().find(|h| &h.id == id))
                .map(|h| h.rect);
        }
    }
    /// Animate only the native surface. The caller has already committed the final pane
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
        self.focused = self.restore_focus.take();
        self.caret_deadline = None;
        self.cancel_effects();
        self.dirty = true;
    }
    pub fn open_settings(&mut self) {
        self.dismiss();
        self.settings_page = true;
        self.settings_search_focused = false;
        self.dirty = true;
    }
    pub fn close_settings(&mut self) {
        self.settings_page = false;
        self.settings_search_focused = false;
        self.dismiss();
        self.focused = None;
    }
    pub fn content_page(&self) -> bool {
        self.settings_page
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

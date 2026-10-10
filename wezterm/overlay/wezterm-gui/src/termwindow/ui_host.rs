//! Window-owned surfaces. Product state lives in the registered provider.
use crate::quad::{HeapQuadAllocator, QuadTrait, TripleLayerQuadAllocator};
use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{
    BorderColor, ComputedElement, DisplayType, Element, ElementColors, ElementContent, Float,
    LayoutContext,
};
use crate::termwindow::render::corners::*;
use crate::termwindow::render::RenderScreenLineParams;
use crate::termwindow::{TabInformation, TermWindow, TermWindowNotif, UIItem, UIItemType};
use config::keyassignment::SpawnCommand;
use mux::renderable::RenderableDimensions;
use mux::{tab::TabId, Mux, MuxNotification};
use std::{
    collections::HashSet,
    rc::Rc,
    sync::Arc,
    time::Instant,
};
use termwiz::surface::Line;
use wezterm_term::color::ColorAttribute;
use window::color::LinearRgba;
use window::{DeadKeyStatus, KeyEvent, MouseEvent, Window, WindowOps};

use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
const FRAME_TOP_LEFT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
        PolyCommand::QuadTo {
            control: (BlockCoord::Zero, BlockCoord::Zero),
            to: (BlockCoord::Zero, BlockCoord::One),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];
const FRAME_TOP_RIGHT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
        PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
        PolyCommand::QuadTo {
            control: (BlockCoord::One, BlockCoord::Zero),
            to: (BlockCoord::Zero, BlockCoord::Zero),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];
const FRAME_BOTTOM_LEFT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Zero),
        PolyCommand::QuadTo {
            control: (BlockCoord::Zero, BlockCoord::One),
            to: (BlockCoord::One, BlockCoord::One),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];
const FRAME_BOTTOM_RIGHT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::One),
        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
        PolyCommand::QuadTo {
            control: (BlockCoord::One, BlockCoord::One),
            to: (BlockCoord::One, BlockCoord::Zero),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Bounds {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reservation {
    pub width: f32,
    pub right: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geometry {
    pub sidebar: Bounds,
    pub content: Bounds,
    pub cell_width: f32,
    pub cell_height: f32,
    pub dpi: f32,
    /// Width and height reserved for AppKit's integrated title controls, in pixels.
    pub header_inset: f32,
    pub header_height: f32,
}
impl Geometry {
    pub fn ui_bounds(self, whole_window: bool) -> Bounds {
        if !whole_window {
            return self.sidebar;
        }
        let x = self.sidebar.x.min(self.content.x);
        let y = self.sidebar.y.min(self.content.y);
        Bounds {
            x,
            y,
            width: (self.sidebar.x + self.sidebar.width).max(self.content.x + self.content.width)
                - x,
            height: (self.sidebar.y + self.sidebar.height)
                .max(self.content.y + self.content.height)
                - y,
        }
    }
    /// The header strip spans only the sidebar when content reserves nothing above it.
    pub fn header_width(self) -> f32 {
        if self.header_height > 0. && self.content.y < self.sidebar.y + self.header_height {
            self.sidebar.width
        } else {
            self.ui_bounds(true).width
        }
    }
}
/// A rounded shape in cell coordinates relative to `Geometry::ui_bounds`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedSurface {
    pub bounds: Bounds,
    /// Corner radius is logical pixels at 96 dpi.
    pub radius: f32,
    /// Visual inset leaves the raw cell bounds available for hit targets and text alignment.
    pub inset: f32,
    /// Icon buttons center a square; rows keep their full extent.
    pub square: bool,
    pub fill: LinearRgba,
}
pub struct Snapshot {
    pub revision: u64,
    pub window_id: usize,
    pub tabs: Vec<Arc<mux::tab::Tab>>,
    /// Tabs whose bell rang since they were last active.
    pub bells: HashSet<TabId>,
    pub active: Option<TabId>,
    pub focused: bool,
    /// The window's effective configuration, including its overrides.
    pub config: config::ConfigHandle,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection {
    pub tabs: Vec<TabId>,
    pub active: Option<TabId>,
}
pub enum Navigation {
    Index(isize),
    Relative(isize, bool),
    Last,
    Move(usize),
    MoveRelative(isize),
    Navigator,
    /// A page shown in place of content closes before any tab beneath it.
    ClosePage,
}
pub enum Input<'a> {
    RawKey(&'a window::RawKeyEvent),
    Key(&'a KeyEvent),
    Mouse(&'a MouseEvent, Geometry),
    Composition(&'a DeadKeyStatus),
    Focus(bool),
}
pub enum Command {
    ToggleTerminalOverlay,
    OpenCommandPalette,
    RunPaletteCommand(crate::commands::ExpandedCommand),
    RunLauncherEntry(crate::overlay::launcher::Entry),
    Activate(TabId),
    Close(TabId, bool),
    Spawn(SpawnCommand, bool),
    /// The provider changed the mux; the next sync reads it again.
    Resync,
    Clipboard(String),
    Paste(u64),
    Semantic(String),
}
pub struct Surface {
    pub rows: Vec<Rc<Line>>,
    pub columns: usize,
    pub revision: u64,
    pub offset: (f32, f32),
    pub opacity: f32,
    /// Per row, pixel spans from the grid's left whose text sits half a cell lower.
    pub centered: Vec<Vec<(f32, f32)>>,
}
pub trait Provider {
    fn initialize(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>;
    fn reservation(&self) -> Reservation;
    /// Whether the sidebar's own header controls fit after `inset` pixels of title buttons.
    fn header_fits(&self, sidebar_width: f32, cell_width: f32, inset: f32) -> bool;
    fn text_input_active(&self) -> bool;
    fn bind(&mut self, window: Window);
    /// The window shows another mux window, or closes; background refreshes stop.
    fn suspend(&mut self);
    fn snapshot(&mut self, snapshot: Snapshot);
    fn navigation(&mut self, navigation: Navigation);
    fn open_command_palette(&mut self, commands: Vec<crate::commands::ExpandedCommand>);
    fn open_launcher(
        &mut self,
        args: config::keyassignment::LauncherActionArgs,
        entries: crate::overlay::launcher::LauncherEntries,
    );
    fn input(&mut self, input: Input<'_>) -> bool;
    fn terminal_input(&mut self, input: Input<'_>) -> bool;
    fn message(&mut self, message: serde_json::Value);
    fn projection(&self) -> Projection;
    fn commands(&mut self) -> Vec<Command>;
    fn render(&mut self, geometry: Geometry, now: Instant);
    fn surface(&self) -> &Surface;
    fn background(&self) -> Option<LinearRgba>;
    fn content_page(&self) -> bool;
    fn overlay_surface(&self) -> bool;
    fn primitives(&self) -> &[RoundedSurface];
    fn deadline(&self) -> Option<Instant>;
    fn caret(&self) -> Option<(usize, usize)>;
    fn keyboard_focus(&self) -> bool;
    fn inspect(&self) -> serde_json::Value;
    fn shutdown(self: Box<Self>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>;
    fn prepare_command(&self, new_window: bool, command: &mut SpawnCommand);
    fn reserve_spawn(&mut self, new_window: bool, command: &SpawnCommand) -> serde_json::Value;
}

thread_local! {
    static ARRIVALS: std::cell::RefCell<std::collections::HashMap<usize, Vec<serde_json::Value>>> = Default::default();
}
pub fn spawn_completed(window: usize, tab: usize, context: serde_json::Value) {
    ARRIVALS.with(|arrivals| {
        arrivals
            .borrow_mut()
            .entry(window)
            .or_default()
            .push(serde_json::json!({"spawned":{"tab_id":tab,"context":context}}))
    });
}
pub fn spawn_failed(window: usize, context: serde_json::Value) {
    if let Some(gui) = crate::frontend::front_end().gui_window_for_mux_window(window) {
        gui.window
            .notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.vtabs_message_for(window, serde_json::json!({"spawn_failed":context}))
            })));
    }
}

const VTABS_UI_ZINDEX: i8 = 5;
fn vtabs_inset_bounds(mut bounds: Bounds, inset: f32, square: bool) -> Bounds {
    let inset = if inset.is_finite() { inset.max(0.) } else { 0. };
    if square {
        let side = bounds.width.min(bounds.height).max(0.);
        bounds.x += (bounds.width - side) / 2.;
        bounds.y += (bounds.height - side) / 2.;
        bounds.width = side;
        bounds.height = side;
    }
    let x = inset.min(bounds.width.max(0.) / 2.);
    let y = inset.min(bounds.height.max(0.) / 2.);
    bounds.x += x;
    bounds.y += y;
    bounds.width = (bounds.width - x * 2.).max(0.);
    bounds.height = (bounds.height - y * 2.).max(0.);
    bounds
}
struct PrimitiveCache {
    surface: RoundedSurface,
    quads: HeapQuadAllocator,
}
struct RowCache {
    line: Rc<Line>,
    centered_ranges: Vec<(f32, f32)>,
    quads: HeapQuadAllocator,
    origin: (f32, f32),
}
impl RowCache {
    fn matches(&self, line: &Rc<Line>, centered_ranges: &[(f32, f32)]) -> bool {
        Rc::ptr_eq(&self.line, line) && self.centered_ranges == centered_ranges
    }
}
fn vtabs_caret_rect(
    geometry: Geometry,
    offset: (f32, f32),
    caret: (usize, usize),
    whole_window: bool,
) -> Option<window::Rect> {
    let bounds = geometry.ui_bounds(whole_window);
    let x = bounds.x + offset.0 + caret.0 as f32 * geometry.cell_width;
    let y = bounds.y + offset.1 + caret.1 as f32 * geometry.cell_height;
    let bounds: window::RectF = euclid::rect(bounds.x, bounds.y, bounds.width, bounds.height);
    // Bounds lead so a NaN caret coordinate yields an empty intersection.
    let caret = euclid::rect(x, y, geometry.cell_width, geometry.cell_height);
    bounds.intersection(&caret)?.round_out().try_cast()
}
pub struct UiHost {
    pub(super) terminal: super::terminal_overlay::TerminalOverlay,
    pub provider: Box<dyn Provider>,
    pub projection: Projection,
    pub dirty: bool,
    needs_commit: bool,
    revision: u64,
    actual_active: Option<TabId>,
    known: HashSet<TabId>,
    bells: HashSet<TabId>,
    bound: bool,
    initialized: bool,
    cache: Vec<RowCache>,
    primitive_cache: Vec<PrimitiveCache>,
    primitive_cache_key: (usize, usize, usize, u32, u32, u32, u32, u32),
    cache_key: (usize, usize, usize, usize, usize, bool),
    chrome: Option<ComputedElement>,
    chrome_items: Vec<UIItem>,
    chrome_key: (usize, usize, usize, usize, usize, bool),
    chrome_dragging: bool,
    geometry: Geometry,
    deadline_task: Option<promise::spawn::Task<()>>,
    deadline_schedule: Option<Instant>,
    deadline_token: u64,
}
impl UiHost {
    pub fn new(window_id: usize) -> Self {
        let mut host = Self {
            terminal: Default::default(),
            provider: crate::vtabs::create(window_id),
            projection: Projection::default(),
            dirty: true,
            needs_commit: true,
            revision: 0,
            actual_active: None,
            known: HashSet::new(),
            bells: HashSet::new(),
            bound: false,
            initialized: false,
            cache: Vec::new(),
            primitive_cache: Vec::new(),
            primitive_cache_key: Default::default(),
            cache_key: (0, 0, 0, 0, 0, false),
            chrome: None,
            chrome_items: Vec::new(),
            chrome_key: (0, 0, 0, 0, 0, false),
            chrome_dragging: false,
            geometry: Geometry::default(),
            deadline_task: None,
            deadline_schedule: None,
            deadline_token: 0,
        };
        ARRIVALS.with(|arrivals| {
            if let Some(arrivals) = arrivals.borrow().get(&window_id) {
                host.initialize(arrivals);
            }
        });
        host
    }
    fn initialize(&mut self, arrivals: &[serde_json::Value]) {
        for arrival in arrivals {
            if arrival["spawned"]["context"]["new_window"] == true {
                self.provider.message(serde_json::json!({"initialize":arrival["spawned"]["context"],"tab_id":arrival["spawned"]["tab_id"]}));
                self.initialized = true;
            }
        }
    }
    pub fn shutdown(mut self) {
        drop(self.deadline_task.take());
        self.provider.suspend();
        let activity = mux::activity::Activity::new();
        promise::spawn::spawn(async move {
            self.provider.shutdown().await;
            drop(activity);
        })
        .detach();
    }
    fn commit(&mut self) {
        let next = self.provider.projection();
        let mut seen = HashSet::new();
        if next
            .tabs
            .iter()
            .all(|id| self.known.contains(id) && seen.insert(*id))
            && next.active.map_or(true, |id| seen.contains(&id))
        {
            self.projection = next;
        } else {
            log::error!("UI rejected invalid tab projection");
        }
    }
}

impl TermWindow {
    fn vtabs_host_mut(&mut self, window_id: usize) -> Option<&mut UiHost> {
        if window_id == self.mux_window_id {
            self.ui_host.as_mut()
        } else {
            self.vtabs_suspended.get_mut(&window_id)
        }
    }
    /// Provider deadlines include durable writes, which must run when unfocused or minimized.
    fn vtabs_schedule_provider(&mut self, window_id: usize) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(ui) = self.vtabs_host_mut(window_id) else {
            return;
        };
        let deadline = ui.provider.deadline();
        if ui.deadline_schedule == deadline {
            return;
        }
        drop(ui.deadline_task.take());
        ui.deadline_schedule = deadline;
        ui.deadline_token = ui.deadline_token.wrapping_add(1);
        let Some(deadline) = deadline else {
            return;
        };
        let token = ui.deadline_token;
        let geometry = ui.geometry;
        ui.deadline_task = Some(promise::spawn::spawn(async move {
            smol::Timer::at(deadline).await;
            let repaint = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                let active = tw.mux_window_id == window_id;
                let geometry = if active {
                    tw.vtabs_geometry()
                } else {
                    geometry
                };
                let Some(ui) = tw.vtabs_host_mut(window_id) else {
                    return;
                };
                if ui.deadline_token != token {
                    return;
                }
                ui.deadline_schedule = None;
                ui.deadline_task.take();
                ui.geometry = geometry;
                // Rendering also advances nonvisual provider work, such as a debounced write.
                ui.provider.render(geometry, Instant::now());
                if active {
                    tw.vtabs_sync();
                    repaint.invalidate();
                }
                tw.vtabs_schedule_provider(window_id);
            })));
        }));
    }
    pub fn vtabs_reserved_width(&self) -> f32 {
        let request = self
            .ui_host
            .as_ref()
            .map(|ui| ui.provider.reservation())
            .unwrap_or(Reservation {
                width: 0.,
                right: false,
            });
        let border = self.get_os_border();
        let context = config::DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.terminal_size.pixel_width as f32,
            pixel_cell: self.render_metrics.cell_size.width as f32,
        };
        let padding = self.config.window_padding.left.evaluate_as_pixels(context)
            + super::resize::effective_right_padding(&self.config, context) as f32;
        let available = (self
            .dimensions
            .pixel_width
            .saturating_sub((border.left + border.right).get()) as f32
            - padding
            - 2. * self.vtabs_frame_gutter())
        .max(0.);
        (request.width * self.dimensions.dpi as f32 / 96.)
            .round()
            .clamp(
                0.,
                (available - self.render_metrics.cell_size.width as f32 * 2.).max(0.),
            )
    }
    pub fn vtabs_left_width(&self) -> f32 {
        if self
            .ui_host
            .as_ref()
            .map_or(false, |ui| ui.provider.reservation().right)
        {
            0.
        } else {
            self.vtabs_reserved_width()
        }
    }
    pub fn vtabs_right_width(&self) -> f32 {
        self.vtabs_reserved_width() - self.vtabs_left_width()
    }
    /// The rounded frame occupies padding, never terminal cells.
    pub fn vtabs_frame_gutter(&self) -> f32 {
        Self::vtabs_frame_gutter_impl(self.dimensions.dpi as f32)
    }
    pub fn vtabs_frame_gutter_impl(dpi: f32) -> f32 {
        (20. * dpi / 96.).round().max(0.)
    }
    fn vtabs_integrated_macos(&self) -> bool {
        cfg!(target_os = "macos")
            && self.config.integrated_title_button_style
                == window::IntegratedTitleButtonStyle::MacOsNative
            && self.vtabs_titlebar_height() > 0.
    }
    /// A left sidebar wide enough for the native buttons carries the header itself.
    fn vtabs_native_header_inset(&self) -> Option<f32> {
        if !self.vtabs_integrated_macos() {
            return None;
        }
        let width = self.vtabs_reserved_width();
        let cell_width = self.render_metrics.cell_size.width as f32;
        let inset = if self.vtabs_right_width() > 0. {
            0.
        } else {
            80. * self.dimensions.dpi as f32 / 96.
        };
        // Narrow left rails put their controls below AppKit's traffic buttons.
        self.ui_host
            .as_ref()
            .is_some_and(|ui| ui.provider.header_fits(width, cell_width, inset))
            .then_some(inset)
    }
    /// Without a sidebar, or beside a left one too narrow for them, the native buttons hide.
    fn vtabs_title_buttons_hidden(&self) -> bool {
        let narrow_left =
            self.vtabs_left_width() > 0. && self.vtabs_native_header_inset().is_none();
        self.ui_host.is_some()
            && self.vtabs_integrated_macos()
            && (self.vtabs_reserved_width() == 0. || narrow_left)
    }
    fn vtabs_inline_header(&self) -> Option<f32> {
        self.vtabs_native_header_inset()
            .or_else(|| self.vtabs_title_buttons_hidden().then_some(0.))
    }
    fn update_vtabs_title_buttons(&self) {
        if let Some(window) = self.window.as_ref() {
            window.set_title_buttons_hidden(self.vtabs_title_buttons_hidden());
        }
    }
    /// Mirrors how mouse_event_impl maps window coordinates onto terminal cells.
    fn update_vtabs_drag_exclusion(&self) {
        if let Some(window) = self.window.as_ref() {
            if self.terminal_overlay_visible() {
                let bounds = self.terminal_overlay_bounds();
                window.set_window_drag_exclusion(window::Rect::new(
                    window::Point::new(bounds.x as isize, bounds.y as isize),
                    window::Size::new(bounds.width as isize, bounds.height as isize),
                ));
                return;
            }
            let border = self.get_os_border();
            let (padding_left, padding_top) = self.padding_left_top();
            window.set_window_drag_exclusion(window::Rect::new(
                window::Point::new(
                    (padding_left + border.left.get() as f32) as isize,
                    (padding_top + border.top.get() as f32) as isize,
                ),
                window::Size::new(
                    self.terminal_size.pixel_width as isize,
                    self.terminal_size.pixel_height as isize,
                ),
            ));
        }
    }
    pub fn vtabs_begin_paint(&mut self) {
        self.vtabs_sync();
        self.resize_terminal_overlay();
        self.update_vtabs_drag_exclusion();
        self.update_vtabs_title_buttons();
    }
    /// Content then starts at the frame gutter, matching its other three sides.
    pub fn vtabs_content_top(&self) -> f32 {
        if self.vtabs_title_buttons_hidden()
            || (self.vtabs_inline_header().is_some() && self.vtabs_left_width() > 0.)
        {
            0.
        } else {
            self.vtabs_titlebar_height()
        }
    }
    pub fn vtabs_titlebar_height(&self) -> f32 {
        Self::vtabs_titlebar_height_impl(
            &self.config,
            &self.fonts,
            self.dimensions.dpi as f32,
            self.window_state.contains(window::WindowState::FULL_SCREEN),
        )
    }
    pub fn vtabs_titlebar_height_impl(
        config: &config::ConfigHandle,
        fonts: &wezterm_font::FontConfiguration,
        dpi: f32,
        fullscreen: bool,
    ) -> f32 {
        use window::{IntegratedTitleButtonStyle as Style, WindowDecorations as Decorations};
        if !config
            .window_decorations
            .contains(Decorations::INTEGRATED_BUTTONS)
            || config.window_decorations.contains(Decorations::TITLE)
            || fullscreen
        {
            return 0.;
        }
        let scale = dpi / 96.;
        let controls = match config.integrated_title_button_style {
            Style::MacOsNative => 28.,
            Style::Windows => 30.,
            Style::Gnome => 38.,
        } * scale;
        let font_height = fonts
            .title_font()
            .map(|font| (font.metrics().cell_height.get() as f32 * 1.75).ceil())
            .unwrap_or(controls);
        controls.max(font_height)
    }
    fn ensure_vtabs_titlebar(&mut self, ui: &mut UiHost, width: f32) -> anyhow::Result<()> {
        let height = self.vtabs_titlebar_height();
        if height == 0. || width <= 0. {
            ui.chrome = None;
            ui.chrome_items.clear();
            return Ok(());
        }
        let key = (
            width as usize,
            self.dimensions.dpi,
            self.config.generation(),
            self.shape_generation,
            self.window_state.bits() as usize,
            self.focused.is_some(),
        );
        if ui.chrome.is_some() && ui.chrome_key == key {
            return Ok(());
        }
        let font = self.fonts.title_font()?;
        let metrics = crate::utilsprites::RenderMetrics::with_font_metrics(&font.metrics());
        let colors = ElementColors {
            border: BorderColor::default(),
            bg: if self.vtabs_integrated_macos() {
                LinearRgba::TRANSPARENT
            } else {
                if self.focused.is_some() {
                    self.config.window_frame.active_titlebar_bg
                } else {
                    self.config.window_frame.inactive_titlebar_bg
                }
                .to_linear()
            }
            .into(),
            text: if self.focused.is_some() {
                self.config.window_frame.active_titlebar_fg
            } else {
                self.config.window_frame.inactive_titlebar_fg
            }
            .to_linear()
            .into(),
        };
        let maximized = self
            .window_state
            .intersects(window::WindowState::MAXIMIZED | window::WindowState::FULL_SCREEN);
        // AppKit owns MacOsNative controls and their hit testing. Other styles reuse
        // the existing window button elements and actions without tab UI.
        let buttons = if self.config.integrated_title_button_style
            == window::IntegratedTitleButtonStyle::MacOsNative
        {
            Vec::new()
        } else {
            self.config
                .integrated_title_buttons
                .iter()
                .map(|button| {
                    crate::termwindow::render::window_buttons::window_button_element(
                        *button,
                        maximized,
                        &font,
                        &metrics,
                        &self.config,
                    )
                })
                .collect()
        };
        let group = Element::new(&font, ElementContent::Children(buttons))
            .colors(colors.clone())
            .float(
                if self.config.integrated_title_button_alignment
                    == window::IntegratedTitleButtonAlignment::Right
                {
                    Float::Right
                } else {
                    Float::None
                },
            );
        let border = self.get_os_border();
        let element = Element::new(&font, ElementContent::Children(vec![group]))
            .display(DisplayType::Block)
            .item_type(UIItemType::TabBar(TabBarItem::None))
            .min_width(Some(config::Dimension::Pixels(width)))
            .min_height(Some(config::Dimension::Pixels(height)))
            .colors(colors);
        let chrome = self.compute_element(
            &LayoutContext {
                height: config::DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: config::DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(
                    border.left.get() as f32,
                    border.top.get() as f32,
                    width,
                    height,
                ),
                metrics: &metrics,
                gl_state: self.render_state.as_ref().unwrap(),
                zindex: 10,
            },
            &element,
        )?;
        ui.chrome_items = chrome.ui_items();
        ui.chrome = Some(chrome);
        ui.chrome_key = key;
        Ok(())
    }
    fn vtabs_titlebar_mouse(&mut self, event: &MouseEvent) -> bool {
        use window::{MouseEventKind as Kind, MousePress};
        let geometry = self.vtabs_geometry();
        let Some(window) = self.window.clone() else {
            return false;
        };
        let Some(mut ui) = self.ui_host.take() else {
            return false;
        };
        if let Err(error) = self.ensure_vtabs_titlebar(&mut ui, geometry.header_width()) {
            log::error!("window controls: {error}");
            self.ui_host = Some(ui);
            return false;
        }
        let item = ui
            .chrome_items
            .iter()
            .rev()
            .find(|item| item.hit_test(event.coords.x, event.coords.y))
            .cloned();
        if item.is_none() && !ui.chrome_dragging {
            let was_hovered = self.current_mouse_event.as_ref().map_or(false, |old| {
                ui.chrome_items
                    .iter()
                    .any(|item| item.hit_test(old.coords.x, old.coords.y))
            });
            if was_hovered {
                self.current_mouse_event = Some(event.clone());
                window.invalidate();
            }
            self.ui_host = Some(ui);
            return false;
        }
        self.current_mouse_event = Some(event.clone());
        // Header clicks use current hit regions, including resized AppKit toolbars.
        ui.geometry = geometry;
        ui.provider.render(geometry, Instant::now());
        if ui.provider.input(Input::Mouse(event, geometry)) {
            ui.needs_commit = true;
            ui.chrome_dragging = false;
            self.window_drag_position = None;
            self.ui_host = Some(ui);
            window.invalidate();
            return true;
        }
        let action = item
            .as_ref()
            .and_then(|item| match &item.item_type {
                UIItemType::TabBar(action) => Some(action.clone()),
                _ => None,
            })
            .unwrap_or(TabBarItem::None);
        if matches!(event.kind, Kind::Press(MousePress::Left)) {
            let cell = self.render_metrics.cell_size;
            let position = wezterm_term::input::ClickPosition {
                column: (event.coords.x / cell.width.max(1)).max(0) as usize,
                row: (event.coords.y / cell.height.max(1)) as i64,
                x_pixel_offset: 0,
                y_pixel_offset: 0,
            };
            let button = wezterm_term::input::MouseButton::Left;
            self.last_mouse_click = Some(match self.last_mouse_click.take() {
                Some(click) => click.add(button, position),
                None => wezterm_term::input::LastMouseClick::new(button, position),
            });
            ui.chrome_dragging = matches!(action, TabBarItem::None);
        }
        let moving = ui.chrome_dragging && matches!(event.kind, Kind::Move);
        if matches!(event.kind, Kind::Release(MousePress::Left)) {
            ui.chrome_dragging = false;
            self.window_drag_position = None;
        }
        self.last_ui_item = item;
        self.ui_host = Some(ui);
        if moving {
            if let Some(start) = &self.window_drag_position {
                window.set_window_position(window::ScreenPoint::new(
                    event.screen_coords.x - start.coords.x,
                    event.screen_coords.y - start.coords.y,
                ));
            }
        } else if !matches!(event.kind, Kind::VertWheel(_) | Kind::HorzWheel(_)) {
            self.mouse_event_tab_bar(action, event.clone(), &window);
        }
        window.set_cursor(Some(window::CursorIcon::Default));
        window.invalidate();
        true
    }
    pub fn vtabs_geometry(&self) -> Geometry {
        let border = self.get_os_border();
        let width = self.vtabs_reserved_width();
        let top = self.vtabs_titlebar_height();
        let content_top = self.vtabs_content_top();
        let content = Bounds {
            x: border.left.get() as f32 + self.vtabs_left_width(),
            y: border.top.get() as f32 + content_top,
            width: (self.dimensions.pixel_width as f32
                - (border.left + border.right).get() as f32
                - width)
                .max(0.),
            height: (self.dimensions.pixel_height as f32
                - (border.top + border.bottom).get() as f32
                - content_top)
                .max(0.),
        };
        let header_inset = self.vtabs_inline_header();
        let inline_header = header_inset.is_some();
        Geometry {
            sidebar: Bounds {
                x: if self.vtabs_right_width() > 0. {
                    content.x + content.width
                } else {
                    border.left.get() as f32
                },
                y: if inline_header {
                    border.top.get() as f32
                } else {
                    content.y
                },
                width,
                height: content.height + if inline_header { content_top } else { 0. },
            },
            content,
            cell_width: self.render_metrics.cell_size.width as f32,
            cell_height: self.render_metrics.cell_size.height as f32,
            dpi: self.dimensions.dpi as f32,
            header_inset: header_inset.unwrap_or(0.),
            header_height: if inline_header { top } else { 0. },
        }
    }
    pub fn vtabs_empty(&self) -> bool {
        self.ui_host
            .as_ref()
            .map_or(false, |ui| !ui.dirty && ui.projection.active.is_none())
    }
    pub fn vtabs_content_hidden(&self) -> bool {
        self.ui_host
            .as_ref()
            .is_some_and(|ui| ui.provider.content_page())
            || (self.vtabs_empty()
                && !Mux::get()
                    .get_active_tab_for_window(self.mux_window_id)
                    .map_or(false, |tab| self.tab_state(tab.tab_id()).overlay.is_some()))
    }
    pub fn vtabs_bell(&mut self, pane_id: usize) {
        if let Some((_, window, tab)) = Mux::get().resolve_pane_id(pane_id) {
            if window == self.mux_window_id {
                if let Some(ui) = self.ui_host.as_mut() {
                    ui.bells.insert(tab);
                    ui.dirty = true;
                }
            }
        }
    }
    pub fn vtabs_mark_dirty(&mut self) {
        if let Some(ui) = self.ui_host.as_mut() {
            ui.dirty = true;
        }
    }
    fn vtabs_reservation(&self) -> (f32, f32) {
        (self.vtabs_reserved_width(), self.vtabs_left_width())
    }
    fn vtabs_apply_reservation(&mut self, before: (f32, f32)) {
        if before != self.vtabs_reservation() {
            if let Some(window) = self.window.clone() {
                self.apply_dimensions(&self.dimensions.clone(), None, &window);
            }
        }
    }
    /// Syncs, re-lays out a changed reservation and repaints.
    fn vtabs_relayout(&mut self, before: (f32, f32)) {
        self.vtabs_sync();
        self.vtabs_apply_reservation(before);
        if let Some(window) = &self.window {
            window.invalidate();
        }
    }
    pub fn vtabs_sync(&mut self) {
        let before = self.vtabs_reservation();
        let Some(mut ui) = self.ui_host.take() else {
            return;
        };
        let arrivals = ARRIVALS.with(|arrivals| {
            arrivals
                .borrow_mut()
                .remove(&self.mux_window_id)
                .unwrap_or_default()
        });
        ui.dirty |= !arrivals.is_empty()
            || ui.actual_active
                != Mux::get()
                    .get_active_tab_for_window(self.mux_window_id)
                    .map(|tab| tab.tab_id());
        if !ui.dirty && !ui.needs_commit && ui.bound && arrivals.is_empty() {
            self.ui_host = Some(ui);
            return;
        }
        if !ui.initialized {
            ui.initialize(&arrivals);
        }
        if !ui.bound {
            if let Some(window) = self.window.clone() {
                ui.provider.bind(window);
                ui.bound = true;
            }
        }
        if ui.dirty {
            let (tabs, active) = match Mux::get().get_window(self.mux_window_id) {
                Some(window) => (
                    window.iter_tabs().cloned().collect::<Vec<_>>(),
                    window.get_active_tab().map(|tab| tab.tab_id()),
                ),
                None => (Vec::new(), None),
            };
            if let Some(id) = active {
                ui.bells.remove(&id);
            }
            ui.actual_active = active;
            ui.revision += 1;
            ui.known = tabs.iter().map(|tab| tab.tab_id()).collect();
            ui.provider.snapshot(Snapshot {
                revision: ui.revision,
                window_id: self.mux_window_id,
                tabs,
                bells: ui.bells.clone(),
                active,
                focused: self.focused.is_some(),
                config: self.config.clone(),
            });
            ui.dirty = false;
        }
        for arrival in arrivals {
            ui.provider.message(arrival);
        }
        ui.commit();
        ui.needs_commit = false;
        let mut commands = ui.provider.commands();
        if let Some(id) = ui.projection.active {
            if Mux::get()
                .get_active_tab_for_window(self.mux_window_id)
                .map(|tab| tab.tab_id())
                != Some(id)
                && !commands
                    .iter()
                    .any(|command| matches!(command, Command::Activate(target) if *target == id))
            {
                commands.push(Command::Activate(id));
            }
        }
        self.ui_host = Some(ui);
        self.vtabs_commands(commands);
        self.vtabs_schedule_provider(self.mux_window_id);
        self.vtabs_apply_reservation(before);
    }
    /// Tabs in the sidebar's visible order; None only without a mux window.
    pub fn vtabs_tab_information(&self) -> Option<Vec<TabInformation>> {
        let mux = Mux::get();
        let window = mux.get_window(self.mux_window_id)?;
        let Some(ui) = self.ui_host.as_ref() else {
            return Some(vec![]);
        };
        let last = window
            .get_last_active_tab_idx()
            .and_then(|idx| window.iter_tabs().nth(idx))
            .map(|tab| tab.tab_id());
        let tabs = ui.projection.tabs.iter().enumerate().filter_map(|(index, id)| {
            let tab = mux.get_tab(*id)?;
            let panes = self.get_pos_panes_for_tab(&tab);
            Some(TabInformation {
                tab_index: index,
                tab_id: *id,
                is_active: ui.projection.active == Some(*id),
                is_last_active: last == Some(*id),
                window_id: self.mux_window_id,
                tab_title: tab.get_title(),
                active_pane: panes
                    .iter()
                    .find(|p| p.is_active)
                    .map(Self::pos_pane_to_pane_info),
            })
        });
        Some(tabs.collect())
    }
    pub fn vtabs_activate_visible(&mut self, id: TabId) {
        self.vtabs_sync();
        if let Some(index) = self
            .ui_host
            .as_ref()
            .and_then(|ui| ui.projection.tabs.iter().position(|tab| *tab == id))
        {
            self.vtabs_navigation(Navigation::Index(index as isize));
        }
    }
    /// Tabs that arrived before subscribing missed TabAddedToWindow; they adopt this size.
    pub fn vtabs_adopt_tab_sizes(&self) {
        if let Some(window) = Mux::get().get_window(self.mux_window_id) {
            for tab in window.iter_tabs() {
                if tab.get_size() != self.terminal_size {
                    tab.resize(self.terminal_size);
                }
            }
        }
    }
    pub fn vtabs_shutdown(&mut self) {
        if let Some(ui) = self.ui_host.take() {
            ui.shutdown();
        }
        for (_, ui) in self.vtabs_suspended.drain() {
            ui.shutdown();
        }
    }
    /// Returns true when upstream must not handle the notification.
    pub fn vtabs_notif(&mut self, notif: &TermWindowNotif) -> bool {
        match notif {
            TermWindowNotif::MuxNotification(notification) => {
                if !matches!(
                    notification,
                    MuxNotification::PaneOutput(_) | MuxNotification::TabResized(_)
                ) {
                    self.vtabs_mark_dirty();
                }
                match notification {
                    MuxNotification::Alert {
                        alert: wezterm_term::Alert::Bell,
                        pane_id,
                    } => self.vtabs_bell(*pane_id),
                    // The window owns content dimensions; new tabs adopt them without resizing it.
                    MuxNotification::TabAddedToWindow { tab_id, .. } => {
                        if let Some(tab) = Mux::get().get_tab(*tab_id) {
                            tab.resize(self.terminal_size);
                        }
                        return true;
                    }
                    MuxNotification::TabResized(_) => return true,
                    MuxNotification::WindowRemoved(window_id) => {
                        if let Some(ui) = self.vtabs_suspended.remove(window_id) {
                            ui.shutdown();
                        }
                    }
                    _ => {}
                }
            }
            TermWindowNotif::SwitchToMuxWindow(window_id) => {
                self.hide_terminal_overlay();
                if let Some(mut ui) = self.ui_host.take() {
                    ui.provider.suspend();
                    ui.provider.input(Input::Focus(false));
                    ui.dirty = true;
                    self.vtabs_suspended.insert(self.mux_window_id, ui);
                }
                self.ui_host = Some(
                    self.vtabs_suspended
                        .remove(window_id)
                        .unwrap_or_else(|| UiHost::new(*window_id)),
                );
            }
            _ => {}
        }
        false
    }
    /// Runs before modals: sidebar clipboard edits, and only window actions without a tab.
    pub fn vtabs_preempt_assignment(
        &mut self,
        assignment: &config::keyassignment::KeyAssignment,
    ) -> bool {
        use config::keyassignment::KeyAssignment::*;
        self.vtabs_sync();
        if self.terminal_overlay_visible() {
            if matches!(assignment, CloseCurrentTab { .. } | CloseCurrentPane { .. }) {
                self.hide_terminal_overlay();
                return true;
            }
            // Structural actions refer to mux tabs, which don't own this pane.
            if matches!(assignment,
                SplitHorizontal(_) | SplitVertical(_) | SplitPane(_) |
                ActivatePaneDirection(_) | ActivatePaneByIndex(_) | AdjustPaneSize(_, _) |
                TogglePaneZoomState | RotatePanes(_) | PaneSelect(_) |
                ActivateTab(_) | ActivateTabRelative(_) | ActivateTabRelativeNoWrap(_) |
                ActivateLastTab | MoveTab(_) | MoveTabRelative(_) |
                SpawnTab(_) | SpawnCommandInNewTab(_)
            ) { return true; }
            return false;
        }
        self.vtabs_clipboard_assignment(assignment)
            || (self.vtabs_empty()
                && !matches!(
                    assignment,
                    ActivateTab(_)
                        | ActivateTabRelative(_)
                        | ActivateTabRelativeNoWrap(_)
                        | ActivateLastTab
                        | SpawnTab(_)
                        | SpawnWindow
                        | SpawnCommandInNewTab(_)
                        | SpawnCommandInNewWindow(_)
                        | ShowTabNavigator
                        | ActivateCommandPalette
                        | ShowLauncher
                        | ShowLauncherArgs(_)
                        | SwitchToWorkspace { .. }
                        | SwitchWorkspaceRelative(_)
                        | EmitEvent(_)
                        | Multiple(_)
                        | ActivateKeyTable { .. }
                        | PopKeyTable
                        | ClearKeyTableStack
                        | Nop
                        | DisableDefaultAssignment
                        | QuitApplication
                        | Hide
                        | HideApplication
                        | Show
                        | ToggleFullScreen
                        | IncreaseFontSize
                        | DecreaseFontSize
                        | ResetFontSize
                        | ReloadConfiguration
                        | ActivateWindow(_)
                        | ActivateWindowRelative(_)
                        | ActivateWindowRelativeNoWrap(_)
                ))
    }
    /// Tab actions follow the sidebar's visible order, not the window's tab indices.
    pub fn vtabs_assignment(&mut self, assignment: &config::keyassignment::KeyAssignment) -> bool {
        use config::keyassignment::KeyAssignment::*;
        let navigation = match assignment {
            ActivateTab(index) => Navigation::Index(*index),
            ActivateTabRelative(delta) => Navigation::Relative(*delta, true),
            ActivateTabRelativeNoWrap(delta) => Navigation::Relative(*delta, false),
            ActivateLastTab => Navigation::Last,
            MoveTab(index) => Navigation::Move(*index),
            MoveTabRelative(delta) => Navigation::MoveRelative(*delta),
            ShowTabNavigator => Navigation::Navigator,
            CloseCurrentTab { .. }
                if self
                    .ui_host
                    .as_ref()
                    .is_some_and(|ui| ui.provider.content_page()) =>
            {
                Navigation::ClosePage
            }
            ActivateCommandPalette => {
                self.vtabs_open_command_palette();
                return true;
            }
            _ => return false,
        };
        self.vtabs_navigation(navigation);
        true
    }
    pub fn vtabs_navigation(&mut self, navigation: Navigation) {
        self.hide_terminal_overlay();
        self.vtabs_sync();
        let before = self.vtabs_reservation();
        if let Some(ui) = self.ui_host.as_mut() {
            ui.provider.navigation(navigation);
            ui.needs_commit = true;
        }
        self.vtabs_relayout(before);
    }
    pub fn vtabs_open_command_palette(&mut self) {
        self.hide_terminal_overlay();
        self.vtabs_sync();
        let before = self.vtabs_reservation();
        let commands = super::palette::CommandPalette::new(self).into_commands();
        self.cancel_modal();
        if let Some(ui) = self.ui_host.as_mut() {
            ui.provider.open_command_palette(commands);
            ui.needs_commit = true;
        }
        self.vtabs_relayout(before);
    }
    pub fn vtabs_open_launcher(
        &mut self,
        mut args: config::keyassignment::LauncherActionArgs,
        initial_choice_idx: usize,
    ) {
        self.hide_terminal_overlay();
        self.vtabs_sync();
        let Some(window) = self.window.clone() else { return };
        let Some(ui) = self.ui_host.as_ref() else { return };
        let visible_tabs = ui.projection.tabs.clone();
        let mux_window_id = self.mux_window_id;
        let domain = self.get_active_pane_or_overlay()
            .map(|pane| pane.domain_id())
            .unwrap_or_else(|| Mux::get().default_domain().domain_id());
        let config = self.config.clone();
        args.alphabet.get_or_insert_with(|| config.launcher_alphabet.clone());
        promise::spawn::spawn(async move {
            let entries = crate::overlay::launcher::LauncherArgs::new(args.flags, visible_tabs, domain)
                .await
                .build_entries(&config, initial_choice_idx);
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                // Domain labels may run Lua; the window can switch workspaces while awaiting them.
                if tw.mux_window_id != mux_window_id { return; }
                let before = tw.vtabs_reservation();
                tw.cancel_modal();
                if let Some(ui) = tw.ui_host.as_mut() {
                    ui.provider.open_launcher(args, entries);
                    ui.needs_commit = true;
                }
                tw.vtabs_relayout(before);
            })));
        }).detach();
    }
    pub fn vtabs_message_for(&mut self, window_id: usize, message: serde_json::Value) {
        if window_id == self.mux_window_id {
            self.vtabs_message(message);
        } else if let Some(ui) = self.vtabs_suspended.get_mut(&window_id) {
            ui.provider.message(message);
            ui.needs_commit = true;
            self.vtabs_schedule_provider(window_id);
        }
    }
    pub fn vtabs_message(&mut self, message: serde_json::Value) {
        if message.get("action").is_some_and(|action| action != "quick_terminal") {
            self.hide_terminal_overlay();
        }
        let before = self.vtabs_reservation();
        if let Some(ui) = self.ui_host.as_mut() {
            ui.provider.message(message);
            ui.needs_commit = true;
        }
        self.vtabs_relayout(before);
    }
    pub fn vtabs_input(&mut self, input: Input<'_>) -> bool {
        self.vtabs_sync();
        if self.terminal_overlay_visible() {
            match &input {
                Input::Focus(false) => self.hide_terminal_overlay(),
                Input::Mouse(event, _) => {
                    if !self.terminal_overlay_bounds().contains(event.coords.x as f32, event.coords.y as f32) {
                        if matches!(self.current_mouse_capture, Some(super::MouseCapture::TerminalPane(_)))
                            && matches!(event.kind, window::MouseEventKind::Move | window::MouseEventKind::Release(_))
                        {
                            return false;
                        }
                        if matches!(event.kind, window::MouseEventKind::Press(_)) {
                            self.hide_terminal_overlay();
                        }
                        return true;
                    }
                    return false;
                }
                Input::Key(_) | Input::RawKey(_) => {
                    let consumed = self.ui_host.as_mut().is_some_and(|ui| ui.provider.terminal_input(input));
                    if consumed {
                        let commands = self.ui_host.as_mut().unwrap().provider.commands();
                        self.vtabs_commands(commands);
                    }
                    return consumed || self.terminal_overlay_pane().is_none();
                }
                Input::Composition(_) => return false,
                _ => {}
            }
        }
        let before = self.vtabs_reservation();
        if let Input::Mouse(event, _) = &input {
            if self.vtabs_titlebar_mouse(event) {
                self.vtabs_relayout(before);
                return true;
            }
        }
        let geometry = self.vtabs_geometry();
        let consumed = self.ui_host.as_mut().map_or(false, |ui| {
            // Input uses the same complete surface and hit regions as the next paint.
            ui.geometry = geometry;
            ui.provider.render(geometry, Instant::now());
            let consumed = ui.provider.input(input);
            ui.needs_commit |= consumed;
            consumed
        });
        if consumed {
            self.vtabs_relayout(before);
        } else {
            self.vtabs_sync();
        }
        consumed
    }
    /// macOS menu key equivalents run clipboard assignments without any key event.
    fn vtabs_clipboard_assignment(&mut self, assignment: &config::keyassignment::KeyAssignment) -> bool {
        use config::keyassignment::KeyAssignment::{CopyTo, PasteFrom};
        let key = match assignment {
            CopyTo(_) => 'c',
            PasteFrom(_) => 'v',
            _ => return false,
        };
        if !self.ui_host.as_ref().map_or(false, |ui| ui.provider.text_input_active()) {
            return false;
        }
        let modifiers = if cfg!(target_os = "macos") {
            window::Modifiers::SUPER
        } else {
            window::Modifiers::CTRL
        };
        let event = KeyEvent {
            key: window::KeyCode::Char(key),
            modifiers,
            leds: Default::default(),
            repeat_count: 1,
            key_is_down: true,
            raw: None,
            #[cfg(windows)]
            win32_uni_char: None,
        };
        self.vtabs_input(Input::Key(&event))
    }
    /// The provider adjusts the command and reserves a sidebar place for its tab.
    pub fn vtabs_prepare_spawn(
        &mut self,
        spawn: &SpawnCommand,
        spawn_where: crate::spawn::SpawnWhere,
    ) -> (SpawnCommand, Option<serde_json::Value>) {
        use crate::spawn::SpawnWhere;
        let new_window = spawn_where == SpawnWhere::NewWindow;
        let mut spawn = spawn.clone();
        if let Some(ui) = self.ui_host.as_ref() {
            ui.provider.prepare_command(new_window, &mut spawn);
        }
        let context = if matches!(spawn_where, SpawnWhere::NewTab | SpawnWhere::NewWindow) {
            self.ui_host
                .as_mut()
                .map(|ui| ui.provider.reserve_spawn(new_window, &spawn))
        } else {
            None
        };
        (spawn, context)
    }
    fn vtabs_commands(&mut self, commands: Vec<Command>) {
        for command in commands {
            let mux = Mux::get();
            match command {
                Command::ToggleTerminalOverlay => self.toggle_terminal_overlay(),
                Command::Activate(id) => {
                    if let Some(mut window) = mux.get_window_mut(self.mux_window_id) {
                        let idx = window.iter_tabs().position(|tab| tab.tab_id() == id);
                        if let Some(idx) = idx {
                            window.remember_and_set_active_tab_idx(idx);
                        }
                    }
                    if let Some(pane) = self.get_active_pane_or_overlay() {
                        pane.focus_changed(true);
                    }
                    if let Some(ui) = self.ui_host.as_mut() {
                        ui.actual_active = Some(id);
                    }
                    self.update_scrollbar();
                }
                Command::Close(id, confirm) => {
                    let idx = mux
                        .get_window(self.mux_window_id)
                        .and_then(|w| w.iter_tabs().position(|t| t.tab_id() == id));
                    if let Some(idx) = idx {
                        self.close_specific_tab(idx, confirm);
                    }
                    self.vtabs_mark_dirty();
                }
                Command::Spawn(command, new_window) => self.spawn_command(
                    &command,
                    if new_window {
                        crate::spawn::SpawnWhere::NewWindow
                    } else {
                        crate::spawn::SpawnWhere::NewTab
                    },
                ),
                Command::Resync => self.vtabs_mark_dirty(),
                Command::Clipboard(text) => {
                    if let Some(window) = &self.window {
                        window.set_clipboard(window::Clipboard::Clipboard, text);
                    }
                }
                Command::Paste(token) => {
                    let window_id = self.mux_window_id;
                    if let Some(window) = self.window.clone() {
                        promise::spawn::spawn(async move {
                            let message = match window
                                .get_clipboard(window::Clipboard::Clipboard)
                                .await
                            {
                                Ok(text) => serde_json::json!({"paste":text,"token":token}),
                                Err(error) => serde_json::json!({"paste_error":error.to_string(),"token":token}),
                            };
                            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                                tw.vtabs_message_for(window_id, message)
                            })));
                        })
                        .detach();
                    }
                }
                Command::OpenCommandPalette => self.vtabs_open_command_palette(),
                Command::RunLauncherEntry(entry) => {
                    if let Some(id) = entry.tab_id {
                        self.vtabs_activate_visible(id);
                    } else if let Some(pane) = self.get_active_pane_or_overlay() {
                        if let Err(err) = self.perform_key_assignment(&pane, &entry.action) {
                            log::error!("Error while performing launcher action: {err:#}");
                        }
                    }
                }
                Command::RunPaletteCommand(command) => {
                    if let Err(err) = super::palette::save_recent(&command) {
                        log::error!("Error while saving recents: {err:#}");
                    }
                    if let Some(pane) = self.get_active_pane_or_overlay() {
                        if let Err(err) = self.perform_key_assignment(&pane, &command.action) {
                            log::error!("Error while performing {command:?}: {err:#}");
                        }
                    }
                }
                Command::Semantic(name) => self.emit_window_event(&name, None),
            }
        }
    }
    pub(super) fn paint_vtabs_rounded_fill(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        rect: Bounds,
        radius: f32,
        color: LinearRgba,
    ) -> anyhow::Result<()> {
        if ![rect.x, rect.y, rect.width, rect.height, radius]
            .iter()
            .all(|value| value.is_finite())
            || rect.width <= 0.
            || rect.height <= 0.
            || color.tuple().3 <= 0.
        {
            return Ok(());
        }
        let radius = radius.max(0.).min(rect.width / 2.).min(rect.height / 2.);
        if radius < 1. {
            self.filled_rectangle(
                layers,
                0,
                euclid::rect(rect.x, rect.y, rect.width, rect.height),
                color,
            )?;
            return Ok(());
        }
        // Disjoint center strips avoid doubling alpha on translucent surfaces.
        for (x, y, width, height) in [
            (
                rect.x + radius,
                rect.y,
                rect.width - radius * 2.,
                rect.height,
            ),
            (rect.x, rect.y + radius, radius, rect.height - radius * 2.),
            (
                rect.x + rect.width - radius,
                rect.y + radius,
                radius,
                rect.height - radius * 2.,
            ),
        ] {
            if width > 0. && height > 0. {
                self.filled_rectangle(layers, 0, euclid::rect(x, y, width, height), color)?;
            }
        }
        for (x, y, corner) in [
            (rect.x, rect.y, TOP_LEFT_ROUNDED_CORNER),
            (
                rect.x + rect.width - radius,
                rect.y,
                TOP_RIGHT_ROUNDED_CORNER,
            ),
            (
                rect.x,
                rect.y + rect.height - radius,
                BOTTOM_LEFT_ROUNDED_CORNER,
            ),
            (
                rect.x + rect.width - radius,
                rect.y + rect.height - radius,
                BOTTOM_RIGHT_ROUNDED_CORNER,
            ),
        ] {
            self.poly_quad(
                layers,
                0,
                euclid::point2(x, y),
                corner,
                0,
                euclid::size2(radius, radius),
                color,
            )?
            .set_grayscale();
        }
        Ok(())
    }
    fn paint_vtabs_frame(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        content: Bounds,
        color: LinearRgba,
    ) -> anyhow::Result<()> {
        let dpi = self.dimensions.dpi as f32;
        let Bounds {
            x,
            y,
            width,
            height,
        } = content;
        if ![x, y, width, height, dpi]
            .iter()
            .all(|value| value.is_finite())
            || width <= 0.
            || height <= 0.
            || dpi <= 0.
        {
            return Ok(());
        }
        let edge = (8. * dpi / 96.)
            .round()
            .max(0.)
            .min(width / 2.)
            .min(height / 2.);
        let radius = (Self::vtabs_frame_gutter_impl(dpi) - edge)
            .max(0.)
            .min((width / 2. - edge).max(0.))
            .min((height / 2. - edge).max(0.));
        for (x, y, width, height) in [
            (x, y, width, edge),
            (x, y + height - edge, width, edge),
            (x, y + edge, edge, height - edge * 2.),
            (x + width - edge, y + edge, edge, height - edge * 2.),
        ] {
            if width > 0. && height > 0. {
                self.filled_rectangle(layers, 0, euclid::rect(x, y, width, height), color)?;
            }
        }
        if radius >= 1. {
            for (x, y, corner) in [
                (x + edge, y + edge, FRAME_TOP_LEFT),
                (x + width - edge - radius, y + edge, FRAME_TOP_RIGHT),
                (x + edge, y + height - edge - radius, FRAME_BOTTOM_LEFT),
                (
                    x + width - edge - radius,
                    y + height - edge - radius,
                    FRAME_BOTTOM_RIGHT,
                ),
            ] {
                self.poly_quad(
                    layers,
                    0,
                    euclid::point2(x, y),
                    corner,
                    0,
                    euclid::size2(radius, radius),
                    color,
                )?
                .set_grayscale();
            }
        }
        Ok(())
    }
    pub fn paint_vtabs_ui(&mut self) -> anyhow::Result<()> {
        let geometry = self.vtabs_geometry();
        // All terminal image, text and background layers remain below the frame and popovers.
        let layer = self
            .render_state
            .as_ref()
            .unwrap()
            .layer_for_zindex(VTABS_UI_ZINDEX)?;
        let mut allocation = layer.quad_allocator();
        let layers = &mut allocation;
        let Some(mut ui) = self.ui_host.take() else {
            return Ok(());
        };
        let result = (|| {
            let start = Instant::now();
            self.ensure_vtabs_titlebar(&mut ui, geometry.header_width())?;
            if let Some(chrome) = &ui.chrome {
                self.render_element(chrome, self.render_state.as_ref().unwrap(), None)?;
            }
            ui.geometry = geometry;
            ui.provider.render(geometry, start);
            let surface = ui.provider.surface();
            let bounds =
                geometry.ui_bounds(ui.provider.content_page() || ui.provider.overlay_surface());
            // Transient surfaces leave the terminal visible beneath their own rounded shapes.
            let background_bounds = geometry.ui_bounds(ui.provider.content_page());
            let rounded = !ui.provider.primitives().is_empty();
            let clip = (
                bounds.x - self.dimensions.pixel_width as f32 / 2.,
                bounds.y - self.dimensions.pixel_height as f32 / 2.,
                bounds.x + bounds.width - self.dimensions.pixel_width as f32 / 2.,
                bounds.y + bounds.height - self.dimensions.pixel_height as f32 / 2.,
            );
            let origin = (clip.0, clip.1);
            let grid_width = surface.columns as f32 * geometry.cell_width;
            let key = (
                self.shape_generation,
                usize::from(self.focused.is_some()),
                self.config.generation(),
                geometry.cell_width as usize,
                geometry.cell_height as usize,
                rounded,
            );
            if ui.cache_key != key {
                ui.cache.clear();
                ui.cache_key = key;
            }
            ui.cache.truncate(surface.rows.len());
            let palette = self.palette().clone();
            let gl_state = self.render_state.as_ref().unwrap();
            let white_space = gl_state.util_sprites.white_space.texture_coords();
            let filled_box = gl_state.util_sprites.filled_box.texture_coords();
            let background = ui
                .provider
                .background()
                .or_else(|| {
                    surface.rows.first().and_then(|line| {
                        line.visible_cells()
                            .next()
                            .map(|cell| palette.resolve_bg(cell.attrs().background()).to_linear())
                    })
                })
                .unwrap_or(palette.background.to_linear());
            if self.vtabs_integrated_macos() {
                let border = self.get_os_border();
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(
                        border.left.get() as f32,
                        border.top.get() as f32,
                        geometry.header_width(),
                        self.vtabs_titlebar_height(),
                    ),
                    background,
                )?;
            }
            self.paint_vtabs_frame(layers, geometry.content, background)?;
            self.filled_rectangle(
                layers,
                0,
                euclid::rect(
                    background_bounds.x,
                    background_bounds.y,
                    background_bounds.width,
                    background_bounds.height,
                ),
                background,
            )?;
            let primitive_key = (
                self.shape_generation,
                self.dimensions.pixel_width,
                self.dimensions.pixel_height,
                geometry.cell_width.to_bits(),
                geometry.cell_height.to_bits(),
                geometry.dpi.to_bits(),
                bounds.x.to_bits(),
                bounds.y.to_bits(),
            );
            if ui.primitive_cache_key != primitive_key {
                // Corner glyph UVs belong to one atlas, even when surface geometry is unchanged.
                ui.primitive_cache.clear();
                ui.primitive_cache_key = primitive_key;
            }
            ui.primitive_cache.truncate(ui.provider.primitives().len());
            for (index, primitive) in ui.provider.primitives().iter().enumerate() {
                if ui
                    .primitive_cache
                    .get(index)
                    .is_none_or(|cached| cached.surface != *primitive)
                {
                    let rect = Bounds {
                        x: bounds.x + primitive.bounds.x * geometry.cell_width,
                        y: bounds.y + primitive.bounds.y * geometry.cell_height,
                        width: primitive.bounds.width * geometry.cell_width,
                        height: primitive.bounds.height * geometry.cell_height,
                    };
                    let scale = geometry.dpi / 96.;
                    let rect =
                        vtabs_inset_bounds(rect, primitive.inset * scale, primitive.square);
                    let mut quads = HeapQuadAllocator::default();
                    self.paint_vtabs_rounded_fill(
                        &mut TripleLayerQuadAllocator::Heap(&mut quads),
                        rect,
                        primitive.radius * scale,
                        primitive.fill,
                    )?;
                    let cached = PrimitiveCache {
                        surface: *primitive,
                        quads,
                    };
                    if index < ui.primitive_cache.len() {
                        ui.primitive_cache[index] = cached;
                    } else {
                        ui.primitive_cache.push(cached);
                    }
                }
                ui.primitive_cache[index].quads.apply_transformed(
                    layers,
                    surface.offset,
                    surface.opacity,
                    clip,
                );
            }
            // Patched "Mono" fonts shrink icons into one cell; the bundled symbols font
            // leads this chain so an icon can fill the blank cell beside it.
            let ui_font = {
                let mut style = self.config.font.clone();
                style
                    .font
                    .insert(0, config::FontAttributes::new("Symbols Nerd Font Mono"));
                self.fonts.resolve_font(&style).ok()
            };
            for (y, line) in surface.rows.iter().enumerate() {
                let centered_ranges = surface.centered.get(y).map_or(&[][..], Vec::as_slice);
                if ui
                    .cache
                    .get(y)
                    .is_none_or(|cached| !cached.matches(line, centered_ranges))
                {
                    let mut quads = HeapQuadAllocator::default();
                    self.render_screen_line(
                        RenderScreenLineParams {
                            top_pixel_y: bounds.y + y as f32 * geometry.cell_height,
                            left_pixel_x: bounds.x,
                            pixel_width: grid_width,
                            stable_line_idx: None,
                            line,
                            selection: 0..0,
                            cursor: &Default::default(),
                            palette: &palette,
                            dims: &RenderableDimensions {
                                cols: surface.columns,
                                physical_top: 0,
                                scrollback_rows: 0,
                                scrollback_top: 0,
                                viewport_rows: surface.rows.len(),
                                dpi: self.terminal_size.dpi,
                                pixel_height: (surface.rows.len() as f32 * geometry.cell_height)
                                    as usize,
                                pixel_width: grid_width as usize,
                                reverse_video: false,
                            },
                            config: &self.config,
                            cursor_border_color: LinearRgba::default(),
                            foreground: palette.foreground.to_linear(),
                            pane: None,
                            is_active: true,
                            selection_fg: LinearRgba::default(),
                            selection_bg: LinearRgba::default(),
                            cursor_fg: LinearRgba::default(),
                            cursor_bg: LinearRgba::default(),
                            cursor_is_default_color: true,
                            white_space,
                            filled_box,
                            window_is_transparent: false,
                            default_bg: palette.resolve_bg(ColorAttribute::Default).to_linear(),
                            style: None,
                            font: ui_font.clone(),
                            use_pixel_positioning: false,
                            render_metrics: self.render_metrics,
                            shape_key: None,
                            password_input: false,
                        },
                        &mut TripleLayerQuadAllocator::Heap(&mut quads),
                    )?;
                    if rounded {
                        quads.remove_solid_backgrounds();
                        quads.offset_glyphs_in_ranges(
                            centered_ranges,
                            origin.0,
                            geometry.cell_height / 2.,
                        );
                    }
                    let cached = RowCache {
                        line: Rc::clone(line),
                        centered_ranges: centered_ranges.to_vec(),
                        quads,
                        origin,
                    };
                    if y < ui.cache.len() {
                        ui.cache[y] = cached;
                    } else {
                        ui.cache.push(cached);
                    }
                }
                ui.cache[y].quads.apply_transformed(
                    layers,
                    (
                        surface.offset.0 + origin.0 - ui.cache[y].origin.0,
                        surface.offset.1 + origin.1 - ui.cache[y].origin.1,
                    ),
                    surface.opacity,
                    clip,
                );
            }
            if ui.provider.keyboard_focus() {
                if let (Some(window), Some(caret)) = (&self.window, ui.provider.caret()) {
                    let mut offset = surface.offset;
                    let x = (caret.0 as f32 + 0.5) * geometry.cell_width;
                    if surface.centered.get(caret.1).is_some_and(|ranges| {
                        ranges.iter().any(|(left, right)| x >= *left && x < *right)
                    }) {
                        offset.1 += geometry.cell_height / 2.;
                    }
                    if let Some(rect) = vtabs_caret_rect(
                        geometry,
                        offset,
                        caret,
                        ui.provider.content_page() || ui.provider.overlay_surface(),
                    ) {
                        window.set_text_cursor_position(rect);
                    }
                }
            }
            metrics::histogram!("ui_host.paint").record(start.elapsed());
            Ok(())
        })();
        self.ui_host = Some(ui);
        self.vtabs_schedule_provider(self.mux_window_id);
        result
    }
}

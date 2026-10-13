//! The provider boundary: snapshots, commands and rendered surfaces.
use super::{Bounds, Geometry, Reservation};
pub use crate::vtabs::{Message, SpawnContext};
use config::keyassignment::SpawnCommand;
use mux::tab::TabId;
use std::{
    collections::HashSet,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use termwiz::surface::Line;
use window::color::LinearRgba;
use window::{DeadKeyStatus, KeyEvent, MouseEvent, Window};

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
/// Runs in place of the overlay's shell; omitted values keep the quick terminal's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalProgram {
    pub spawn: SpawnCommand,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub padding: TerminalPadding,
    pub opacity: Option<f32>,
    pub blur: Option<f32>,
    pub border: Option<TerminalBorder>,
    /// Over the window's palette; its background also fills the frame.
    pub colors: Option<config::Palette>,
    pub auto_hide: Option<bool>,
    pub hide_on_escape: Option<HideOnEscape>,
    pub preserve: bool,
}
/// Sides left `None` follow the next level, and finally the window's `window_padding`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TerminalPadding {
    pub left: Option<config::Dimension>,
    pub top: Option<config::Dimension>,
    pub right: Option<config::Dimension>,
    pub bottom: Option<config::Dimension>,
}
impl TerminalPadding {
    pub fn or(self, other: Self) -> Self {
        Self {
            left: self.left.or(other.left),
            top: self.top.or(other.top),
            right: self.right.or(other.right),
            bottom: self.bottom.or(other.bottom),
        }
    }
    pub fn resolve(self, window: &config::WindowPadding) -> config::WindowPadding {
        config::WindowPadding {
            left: self.left.unwrap_or(window.left),
            top: self.top.unwrap_or(window.top),
            right: self.right.unwrap_or(window.right),
            bottom: self.bottom.unwrap_or(window.bottom),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TerminalBorder {
    /// The terminal foreground at 22% opacity.
    #[default]
    Palette,
    None,
    Color(LinearRgba),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HideOnEscape {
    Never,
    Always,
    /// Only while nothing runs in the pane.
    #[default]
    Auto,
}
/// What the quick terminal makes of a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalKey {
    /// Reaches the pane.
    Pass,
    /// Any effect is queued in `commands`.
    Handled,
    /// An unmodified Escape that hides only while nothing runs in the pane.
    Escape,
}
pub enum Command {
    ToggleTerminalOverlay,
    HideTerminalOverlay,
    /// The same program hides; anything else visible is replaced.
    ToggleTerminalProgram(TerminalProgram),
    OpenCommandPalette,
    RunPaletteCommand(crate::commands::ExpandedCommand),
    RunLauncherEntry(super::launcher::Entry),
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickTerminalConfig {
    pub size: (f32, f32),
    pub padding: TerminalPadding,
    pub opacity: f32,
    /// Applies only below full opacity.
    pub blur: f32,
    pub border: TerminalBorder,
    pub background_color: Option<LinearRgba>,
    pub auto_hide: bool,
    pub hide_on_escape: HideOnEscape,
    pub preserve_shell: bool,
    /// Size steps ease over this; zero while motion is off.
    pub transition: Duration,
}
impl QuickTerminalConfig {
    /// The program's size stays with the overlay's steps.
    pub fn with(self, program: &TerminalProgram) -> Self {
        let background = program
            .colors
            .as_ref()
            .and_then(|colors| colors.background)
            .map(|color| color.to_linear());
        Self {
            padding: program.padding.or(self.padding),
            opacity: program.opacity.unwrap_or(self.opacity),
            blur: program.blur.unwrap_or(self.blur),
            border: program.border.unwrap_or(self.border),
            background_color: background.or(self.background_color),
            auto_hide: program.auto_hide.unwrap_or(self.auto_hide),
            hide_on_escape: program.hide_on_escape.unwrap_or(self.hide_on_escape),
            ..self
        }
    }
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
        entries: super::launcher::LauncherEntries,
    );
    fn input(&mut self, input: Input<'_>) -> bool;
    /// `escape` is what the shown pane's settings make of an unmodified Escape.
    fn terminal_input(&mut self, input: Input<'_>, escape: HideOnEscape) -> TerminalKey;
    fn glyph_scale(&self) -> f64;
    fn quick_terminal_config(&self) -> QuickTerminalConfig;
    fn message(&mut self, message: Message);
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
    fn prepare_command(&self, command: &mut SpawnCommand);
    fn reserve_spawn(&mut self, new_window: bool, command: &SpawnCommand) -> SpawnContext;
}

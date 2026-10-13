//! Window layout, sidebar reservations and native title-control geometry.
use super::TermWindow;
use window::WindowOps;

/// AppKit's traffic buttons in logical pixels at 96 dpi.
const NATIVE_BUTTONS_WIDTH: f32 = 80.;

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
    pub fn intersects(&self, other: &Bounds) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reservation {
    pub width: f32,
    pub right: bool,
}
impl Reservation {
    fn scaled(self, dpi: f32) -> f32 {
        (self.width * dpi / 96.).round()
    }
}
/// Leaves the terminal at least two cells of `available`.
fn clamp_reserved(width: f32, available: f32, cell_width: f32) -> f32 {
    width.clamp(0., (available - cell_width * 2.).max(0.))
}

/// Sidebar, header and frame gutter around the window padding, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}
impl Insets {
    fn new(right_side: bool, width: f32, top: f32, gutter: f32) -> Self {
        let (left, right) = if right_side { (0., width) } else { (width, 0.) };
        Self {
            left: left + gutter,
            top: top + gutter,
            right: right + gutter,
            bottom: gutter,
        }
    }
    pub fn pad(
        self,
        left: usize,
        top: usize,
        right: usize,
        bottom: usize,
    ) -> (usize, usize, usize, usize) {
        (
            left + self.left as usize,
            top + self.top as usize,
            right + self.right as usize,
            bottom + self.bottom as usize,
        )
    }
}
impl super::UiHost {
    /// The first window is sized around its terminal, so only its width bounds the sidebar.
    pub fn initial_insets(
        &self,
        config: &config::ConfigHandle,
        fonts: &wezterm_font::FontConfiguration,
        render_metrics: &crate::utilsprites::RenderMetrics,
        terminal: &wezterm_term::TerminalSize,
    ) -> Insets {
        let dpi = terminal.dpi as f32;
        let reservation = self.provider.reservation();
        let requested = reservation.scaled(dpi);
        let width = clamp_reserved(
            requested,
            terminal.pixel_width as f32 + requested,
            render_metrics.cell_size.width as f32,
        );
        Insets::new(
            reservation.right,
            width,
            TermWindow::vtabs_titlebar_height_impl(config, fonts, dpi, false),
            TermWindow::vtabs_frame_gutter_impl(dpi),
        )
    }
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
    /// The terminal palette background, which content pages share with shells.
    pub page_background: Option<(u8, u8, u8)>,
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
impl TermWindow {
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
            + crate::termwindow::resize::effective_right_padding(&self.config, context) as f32;
        let available = (self
            .dimensions
            .pixel_width
            .saturating_sub((border.left + border.right).get()) as f32
            - padding
            - 2. * self.vtabs_frame_gutter())
        .max(0.);
        clamp_reserved(
            request.scaled(self.dimensions.dpi as f32),
            available,
            self.render_metrics.cell_size.width as f32,
        )
    }
    pub fn vtabs_insets(&self) -> Insets {
        let gutter = self.vtabs_frame_gutter();
        Insets {
            left: self.vtabs_left_width() + gutter,
            top: self.vtabs_content_top() + gutter,
            right: self.vtabs_right_width() + gutter,
            bottom: gutter,
        }
    }
    /// Pane backgrounds fill to the window edges, minus the sidebar, header or quick terminal.
    pub fn vtabs_pane_background(
        &self,
        pos: &mux::tab::PositionedPane,
        rect: window::RectF,
    ) -> window::RectF {
        if let Some(bounds) = self.terminal_overlay_content(pos.pane.pane_id()) {
            return euclid::rect(bounds.x, bounds.y, bounds.width, bounds.height);
        }
        let rightmost = pos.left + pos.width >= self.terminal_size.cols as usize;
        let left = if pos.left == 0 {
            self.vtabs_left_width()
        } else {
            0.
        };
        let top = if pos.top == 0 {
            self.vtabs_content_top()
        } else {
            0.
        };
        let right = if rightmost {
            self.vtabs_right_width()
        } else {
            0.
        };
        euclid::rect(
            rect.origin.x + left,
            rect.origin.y + top,
            rect.size.width - left - right,
            rect.size.height - top,
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
    pub(super) fn vtabs_integrated_macos(&self) -> bool {
        cfg!(target_os = "macos")
            && self.config.integrated_title_button_style
                == window::IntegratedTitleButtonStyle::MacOsNative
            && self.vtabs_titlebar_height() > 0.
    }
    /// A left sidebar wide enough for the native buttons carries the header itself.
    pub(super) fn vtabs_native_header_inset(&self) -> Option<f32> {
        if !self.vtabs_integrated_macos() {
            return None;
        }
        let width = self.vtabs_reserved_width();
        let cell_width = self.render_metrics.cell_size.width as f32;
        let inset = if self.vtabs_right_width() > 0. {
            0.
        } else {
            NATIVE_BUTTONS_WIDTH * self.dimensions.dpi as f32 / 96.
        };
        // Narrow left rails put their controls below AppKit's traffic buttons.
        self.ui_host
            .as_ref()
            .is_some_and(|ui| ui.provider.header_fits(width, cell_width, inset))
            .then_some(inset)
    }
    /// Without a sidebar, or beside a left one too narrow for them, the native buttons hide.
    pub(super) fn vtabs_title_buttons_hidden(&self) -> bool {
        let narrow_left =
            self.vtabs_left_width() > 0. && self.vtabs_native_header_inset().is_none();
        self.ui_host.is_some()
            && self.vtabs_integrated_macos()
            && (self.vtabs_reserved_width() == 0. || narrow_left)
    }
    pub(super) fn vtabs_inline_header(&self) -> Option<f32> {
        self.vtabs_native_header_inset()
            .or_else(|| self.vtabs_title_buttons_hidden().then_some(0.))
    }
    /// AppKit draws the native buttons above a quick terminal reaching into their corner.
    pub(super) fn vtabs_title_buttons_covered(&self) -> bool {
        if !self.vtabs_integrated_macos() || !self.terminal_overlay_visible() {
            return false;
        }
        let border = self.get_os_border();
        let buttons = Bounds {
            x: border.left.get() as f32,
            y: border.top.get() as f32,
            width: NATIVE_BUTTONS_WIDTH * self.dimensions.dpi as f32 / 96.,
            height: self.vtabs_titlebar_height(),
        };
        self.terminal_overlay_bounds().intersects(&buttons)
    }
    pub(super) fn update_vtabs_title_buttons(&self) {
        if let Some(window) = self.window.as_ref() {
            window.set_title_buttons_hidden(
                self.vtabs_title_buttons_hidden() || self.vtabs_title_buttons_covered(),
            );
        }
    }
    /// Mirrors how mouse_event_impl maps window coordinates onto terminal cells.
    pub(super) fn update_vtabs_drag_exclusion(&self) {
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
            page_background: self.palette.as_ref().map(|palette| {
                let (r, g, b, _) = palette.background.to_srgb_u8();
                (r, g, b)
            }),
        }
    }
    pub(super) fn vtabs_reservation(&self) -> (f32, f32) {
        (self.vtabs_reserved_width(), self.vtabs_left_width())
    }
    pub(super) fn vtabs_apply_reservation(&mut self, before: (f32, f32)) {
        if before != self.vtabs_reservation() {
            if let Some(window) = self.window.clone() {
                self.apply_dimensions(&self.dimensions.clone(), None, &window);
            }
        }
    }
    /// Syncs, re-lays out a changed reservation and repaints.
    pub(super) fn vtabs_relayout(&mut self, before: (f32, f32)) {
        self.vtabs_sync();
        self.vtabs_apply_reservation(before);
        if let Some(window) = &self.window {
            window.invalidate();
        }
    }
}

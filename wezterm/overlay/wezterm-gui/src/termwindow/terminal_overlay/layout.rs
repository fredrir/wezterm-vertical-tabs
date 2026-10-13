//! Where the quick terminal sits: frame, padding, grid and size steps.
use super::*;

pub(super) const SHELL_SIZE: (f32, f32) = (0.75, 0.75);

/// Fewest columns and rows a size step leaves.
pub(super) const MIN_CELLS: (f32, f32) = (20., 5.);

/// The frame and content as painted this frame, against the layout they ease toward.
#[derive(Clone, Copy, Debug)]
pub(super) struct Drawn {
    pub(super) layout: Layout,
    pub(super) size: (f32, f32),
    pub(super) painted: Layout,
    pub(super) origin: (f32, f32),
    pub(super) moving: bool,
}

/// `step` by `cells`, then by pixels until `grid` follows, as percent padding grows with the frame.
pub(super) fn step_cells(
    current: f32,
    cells: f32,
    cell: f32,
    (minimum, maximum): (f32, f32),
    grid: impl Fn(f32) -> f32,
) -> f32 {
    let mut target = step(current, cells * cell, minimum, maximum);
    let want = grid(current) + cells;
    while cells > 0. && grid(target) < want && target < maximum {
        target = (target + 1.).min(maximum);
    }
    while cells < 0. && grid(target) > want && target > minimum {
        target = (target - 1.).max(minimum);
    }
    target
}

/// `current` moved by `delta` within `minimum..=maximum`, never back past where it started.
pub(super) fn step(current: f32, delta: f32, minimum: f32, maximum: f32) -> f32 {
    if delta > 0. {
        (current + delta).min(maximum).max(current)
    } else {
        (current + delta).max(minimum).min(current)
    }
}

/// Between the frame and the grid, read as the main window reads its own settings.
#[derive(Clone, Copy, Debug)]
pub(super) struct Edges {
    pub(super) border: f32,
    pub(super) padding: WindowPadding,
    pub(super) alignment: WindowContentAlignment,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
    pub(super) frame: Bounds,
    /// Inside the border and padding, which always hold the content.
    pub(super) inner: Bounds,
    pub(super) content: Bounds,
    pub(super) size: TerminalSize,
}

impl Layout {
    pub(super) fn new(
        bounds: Bounds,
        (width, height): (f32, f32),
        edges: &Edges,
        (cell_width, cell_height): (f32, f32),
        dpi: u32,
    ) -> Self {
        // Whole device pixels keep glyphs and frame edges crisp.
        let frame = Bounds {
            x: (bounds.x + bounds.width * (1. - width) / 2.).round(),
            y: (bounds.y + bounds.height * (1. - height) / 2.).round(),
            width: (bounds.width * width).round(),
            height: (bounds.height * height).round(),
        };
        let border = edges.border.min(frame.width / 4.).min(frame.height / 4.);
        let (edge_width, edge_height) = (frame.width - 2. * border, frame.height - 2. * border);
        let side = |dimension: config::Dimension, pixel_max: f32, pixel_cell: f32| {
            let context = DimensionContext {
                dpi: dpi as f32,
                pixel_max,
                pixel_cell,
            };
            dimension
                .evaluate_as_pixels(context)
                .min(pixel_max / 4.)
                .max(0.)
                .floor()
        };
        let padding = &edges.padding;
        let left = border + side(padding.left, edge_width, cell_width);
        let right = border + side(padding.right, edge_width, cell_width);
        let top = border + side(padding.top, edge_height, cell_height);
        let bottom = border + side(padding.bottom, edge_height, cell_height);
        let inner = Bounds {
            x: frame.x + left,
            y: frame.y + top,
            width: (frame.width - left - right).max(0.),
            height: (frame.height - top - bottom).max(0.),
        };
        let cols = (inner.width / cell_width).floor().max(1.) as usize;
        let rows = (inner.height / cell_height).floor().max(1.) as usize;
        let width = cols as f32 * cell_width;
        let height = rows as f32 * cell_height;
        let x = match edges.alignment.horizontal {
            HorizontalWindowContentAlignment::Left => inner.x,
            HorizontalWindowContentAlignment::Center => inner.x + (inner.width - width) / 2.,
            HorizontalWindowContentAlignment::Right => inner.x + inner.width - width,
        };
        let y = match edges.alignment.vertical {
            VerticalWindowContentAlignment::Top => inner.y,
            VerticalWindowContentAlignment::Center => inner.y + (inner.height - height) / 2.,
            VerticalWindowContentAlignment::Bottom => inner.y + inner.height - height,
        };
        Self {
            frame,
            inner,
            content: Bounds {
                x: x.floor(),
                y: y.floor(),
                width,
                height,
            },
            size: TerminalSize {
                cols,
                rows,
                pixel_width: width as usize,
                pixel_height: height as usize,
                dpi,
            },
        }
    }
}

impl TermWindow {
    fn terminal_overlay_edges(&self) -> Edges {
        let config = self.terminal_overlay_config();
        let bordered = config.map_or(true, |config| config.border != TerminalBorder::None);
        Edges {
            border: if bordered {
                border_width(self.dimensions.dpi as u32)
            } else {
                0.
            },
            padding: config
                .map(|config| config.padding)
                .unwrap_or_default()
                .resolve(&self.config.window_padding),
            alignment: self.config.window_content_alignment,
        }
    }

    fn terminal_overlay_layout_of(&self, size: (f32, f32)) -> Layout {
        Layout::new(
            self.vtabs_geometry().ui_bounds(true),
            size,
            &self.terminal_overlay_edges(),
            (
                self.render_metrics.cell_size.width as f32,
                self.render_metrics.cell_size.height as f32,
            ),
            self.dimensions.dpi as u32,
        )
    }

    fn terminal_overlay_target(&self) -> (f32, f32) {
        self.ui_host.as_ref().map_or(SHELL_SIZE, |ui| {
            ui.terminal
                .session
                .borrow()
                .size(ui.provider.quick_terminal_config().size)
        })
    }

    pub(super) fn terminal_overlay_layout(&self) -> Layout {
        self.terminal_overlay_layout_of(self.terminal_overlay_target())
    }

    pub(super) fn terminal_overlay_drawn(&self) -> Drawn {
        let size = self.terminal_overlay_target();
        let layout = self.terminal_overlay_layout_of(size);
        let origin = (layout.content.x, layout.content.y);
        let animated = self
            .ui_host
            .as_ref()
            .and_then(|ui| ui.terminal.session.borrow().animate_to(size, origin));
        let Some((drawn, (x, y))) = animated else {
            return Drawn {
                layout,
                size,
                painted: layout,
                origin,
                moving: false,
            };
        };
        Drawn {
            layout,
            size: drawn,
            painted: self.terminal_overlay_layout_of(drawn),
            origin: (x.round(), y.round()),
            moving: true,
        }
    }

    /// The frame as painted, mid step included.
    pub(in crate::termwindow) fn terminal_overlay_bounds(&self) -> Bounds {
        self.terminal_overlay_drawn().painted.frame
    }

    pub(in crate::termwindow) fn terminal_overlay_content(
        &self,
        pane_id: PaneId,
    ) -> Option<Bounds> {
        let pane = self.terminal_overlay_pane()?;
        let overlay = self
            .pane_state(pane.pane_id())
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.pane_id());
        (pane.pane_id() == pane_id || overlay == Some(pane_id))
            .then(|| self.terminal_overlay_layout().content)
    }

    pub(in crate::termwindow) fn pane_padding_left_top(&self, pane_id: PaneId) -> (f32, f32) {
        if let Some(bounds) = self.terminal_overlay_content(pane_id) {
            let border = self.get_os_border();
            (
                bounds.x - border.left.get() as f32,
                bounds.y - border.top.get() as f32,
            )
        } else {
            self.padding_left_top()
        }
    }

    pub(in crate::termwindow) fn positioned_terminal_overlay(&self) -> Option<PositionedPane> {
        self.terminal_overlay_pane()?;
        let pane = self.get_active_pane_or_overlay()?;
        let size = self.terminal_overlay_layout().size;
        Some(PositionedPane {
            pane,
            index: 0,
            is_active: true,
            is_zoomed: false,
            left: 0,
            top: 0,
            width: size.cols,
            height: size.rows,
            pixel_width: size.pixel_width,
            pixel_height: size.pixel_height,
        })
    }

    /// `AdjustPaneSize` in cells; Right and Down grow, and the frame stays centered.
    pub(in crate::termwindow) fn adjust_terminal_overlay(
        &mut self,
        direction: PaneDirection,
        amount: usize,
    ) {
        let (horizontal, sign) = match direction {
            PaneDirection::Right => (true, 1.),
            PaneDirection::Left => (true, -1.),
            PaneDirection::Down => (false, 1.),
            PaneDirection::Up => (false, -1.),
            PaneDirection::Next | PaneDirection::Prev => return,
        };
        let Some(ui) = &self.ui_host else {
            return;
        };
        let bounds = self.vtabs_geometry().ui_bounds(true);
        let drawn = self.terminal_overlay_drawn();
        let (frame, inner) = (drawn.layout.frame, drawn.layout.inner);
        let metrics = &self.render_metrics.cell_size;
        let (extent, current, padding, cell, minimum_cells) = if horizontal {
            let padding = frame.width - inner.width;
            (
                bounds.width,
                frame.width,
                padding,
                metrics.width as f32,
                MIN_CELLS.0,
            )
        } else {
            let padding = frame.height - inner.height;
            (
                bounds.height,
                frame.height,
                padding,
                metrics.height as f32,
                MIN_CELLS.1,
            )
        };
        if extent <= 0. {
            return;
        }
        let size = self.terminal_overlay_target();
        let grid = |frame: f32| {
            let size = if horizontal {
                (frame / extent, size.1)
            } else {
                (size.0, frame / extent)
            };
            let layout = self.terminal_overlay_layout_of(size);
            let cells = if horizontal {
                layout.size.cols
            } else {
                layout.size.rows
            };
            cells as f32
        };
        let cells = sign * amount as f32;
        let target = step_cells(
            current,
            cells,
            cell,
            (minimum_cells * cell + padding, extent),
            grid,
        );
        if target == current {
            return;
        }
        let duration = ui.provider.quick_terminal_config().transition;
        ui.terminal.session.borrow_mut().resize(
            horizontal,
            target / extent,
            (drawn.size, drawn.origin),
            duration,
            Instant::now(),
        );
        if let Some(window) = &self.window {
            window.invalidate();
        }
    }
}

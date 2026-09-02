// AppKit sizes the window buttons in points; macOS counts 72 dpi as 1x, everything else 96.
pub const POINT_DPI: f64 = 72.0;
pub const LOGICAL_DPI: f64 = 96.0;
// wezterm-gui reserves this much for the traffic lights.
pub const BUTTON_PT: f64 = 70.0;
/// Centre-to-centre spacing of the three lights; the strip's own glyphs sit on the same pitch.
pub const BUTTON_PITCH_PT: f64 = 20.0;
pub const TITLEBAR_PT: f64 = 28.0;
pub const BUTTON_CENTER_PT: f64 = TITLEBAR_PT / 2.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct Dims {
    pub cols: u32,
    pub viewport_rows: u32,
    pub pixel_width: f64,
    pub pixel_height: f64,
    pub dpi: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StripOpts {
    pub is_mac: bool,
    pub integrated_buttons: bool,
    pub native_button_style: bool,
    pub position_left: bool,
    pub is_full_screen: bool,
    pub preview: bool,
    pub rail: bool,
    pub rail_width: u32,
    pub padding_top: i64,
    pub toggle_button: bool,
    pub card_x1: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripGeometry {
    pub rows: u32,
    pub rows_reserved: u32,
    pub cols: u32,
    /// In points, so the action cluster can match the lights' pitch the same way the reserve does.
    pub cell_w: Option<f64>,
    pub width: Option<u32>,
    pub toggle_row: u32,
    pub toggle_x: u32,
}

/// Cells the top strip keeps clear and where the toggle goes; pure, so it needs no macOS window.
pub fn strip_geometry(dims: Dims, opts: StripOpts) -> StripGeometry {
    let mut rows: u32 = 0;
    let mut cols: u32 = 0;
    let reserve = opts.is_mac
        && opts.integrated_buttons
        && opts.native_button_style
        && opts.position_left
        && !opts.is_full_screen;
    let (mut cell_h, mut cell_w) = (0.0f64, 0.0f64);
    if dims.cols > 0 && dims.viewport_rows > 0 {
        let base = if opts.preview { LOGICAL_DPI } else { POINT_DPI };
        let scale = dims.dpi.unwrap_or(base) / base;
        cell_w = dims.pixel_width / f64::from(dims.cols) / scale;
        cell_h = dims.pixel_height / f64::from(dims.viewport_rows) / scale;
        if reserve && cell_w > 0.0 && cell_h > 0.0 {
            cols = (BUTTON_PT / cell_w).ceil() as u32;
            rows = ((TITLEBAR_PT / cell_h).ceil() as u32).max(1);
        }
    }
    // Rail mode centres the toggle *below* the lights: at 9 columns there is no column 11.
    let pad_top = opts.padding_top.max(0) as u32;
    let toggle_row = if opts.rail {
        if cols > 0 { rows + 1 } else { 1 }
    } else if rows > 0 {
        // With a half-cell of window padding, pane row n is centred at n * cell_h, so the row
        // nearest the lights' centre is round(), not ceil().
        ((BUTTON_CENTER_PT / cell_h + 0.5).floor() as u32)
            .min(rows)
            .max(1)
    } else {
        // no lights to line up with, so padding.top is air above the strip, not below it
        pad_top + 1
    };
    let width = opts.rail_width.max(cols);
    let toggle_x = if opts.rail {
        width.div_ceil(2)
    } else if cols > 0 {
        cols + 2
    } else {
        opts.card_x1.unwrap_or(2)
    };
    let strip_rows = if rows > 0 {
        // the lights own the top of the pane, so the padding lands under the reserve, not over it
        rows.max(if opts.toggle_button { toggle_row } else { 0 }) + pad_top
    } else {
        pad_top + u32::from(opts.toggle_button)
    };
    StripGeometry {
        rows: strip_rows,
        rows_reserved: rows,
        cols,
        cell_w: (cell_w > 0.0).then_some(cell_w),
        width: opts.rail.then_some(width),
        toggle_row,
        toggle_x,
    }
}

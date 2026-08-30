local wezterm = require "wezterm" ---@type Wezterm

local triple = wezterm.target_triple or ""

local M = {
  triple = triple,
  is_mac = triple:find "apple%-darwin" ~= nil,
  is_windows = triple:find "windows" ~= nil,
}

M.SUPER = M.is_mac and "CMD" or "CTRL|SHIFT"
M.SUPER2 = M.is_mac and "CMD|SHIFT" or "CTRL|SHIFT|ALT"

-- AppKit sizes the window buttons in points, so the reserve is measured in points too. macOS
-- counts 72 dpi as 1x (wezterm window/src/lib.rs:23), and a pane reports device pixels, so a 2x
-- display would otherwise halve every count below.
M.POINT_DPI = 72
-- Every non-Apple platform calls 96 dpi 1x and scales from there (Windows reports 120 at 125 %), so
-- previewing the reserve on one of them has to divide by *its* 1x or a plain 96-dpi host reads as
-- a 1.33x Mac and reserves a third too many columns.
M.LOGICAL_DPI = 96
-- wezterm-gui/src/termwindow/render/fancy_tab_bar.rs:382 reserves this much for the traffic lights.
M.BUTTON_PT = 70
-- macOS titlebar height; NEEDS-ENG-CONFIRM #1, over-reserving is the safe direction.
M.TITLEBAR_PT = 28
-- The lights are centred in the title bar, so this is where the toggle must line up.
M.BUTTON_CENTER_PT = M.TITLEBAR_PT / 2
-- `window_padding` takes a bare number as device pixels; the `pt` unit is what scales with the dpi
-- (wezterm config/src/units.rs:81, 166-173).
M.TITLEBAR_PAD = string.format("%dpt", M.TITLEBAR_PT)

---Cells the top strip keeps clear and where the toggle goes; pure, so it needs no macOS window.
function M.strip_geometry(dims, opts)
  dims, opts = dims or {}, opts or {}
  local rows, cols = 0, 0
  local reserve = opts.is_mac
    and opts.integrated_buttons
    and opts.native_button_style
    and opts.position == "left"
    and not opts.is_full_screen
  local cell_h = 0
  if reserve and dims.cols and dims.cols > 0 and dims.viewport_rows and dims.viewport_rows > 0 then
    local base = opts.preview and M.LOGICAL_DPI or M.POINT_DPI
    local scale = (dims.dpi or base) / base
    local cell_w = (dims.pixel_width or 0) / dims.cols / scale
    cell_h = (dims.pixel_height or 0) / dims.viewport_rows / scale
    if cell_w > 0 and cell_h > 0 then
      cols = math.ceil(M.BUTTON_PT / cell_w)
      rows = math.max(1, math.ceil(M.TITLEBAR_PT / cell_h))
    end
  end
  -- Rail mode centres the toggle *below* the lights: at 9 columns there is no column 11.
  local rail = opts.rail == true
  local toggle_row
  if rail then
    toggle_row = cols > 0 and rows + 1 or 1
  elseif rows > 0 then
    -- `rows` is a count, not an index: derive the row from the lights' centre and clamp into it.
    toggle_row = math.max(1, math.min(math.ceil(M.BUTTON_CENTER_PT / cell_h), rows))
  else
    toggle_row = 1
  end
  local width = math.max(opts.rail_width or 0, cols)
  local toggle_x
  if rail then
    toggle_x = math.ceil(width / 2)
  else
    toggle_x = cols > 0 and cols + 2 or (opts.card_x1 or 2)
  end
  local strip_rows = math.max(rows, opts.toggle_button and toggle_row or 0) + (opts.padding_top or 0)
  return {
    rows = strip_rows,
    rows_reserved = rows,
    cols = cols,
    width = rail and width or nil,
    toggle_row = toggle_row,
    toggle_x = toggle_x,
  }
end

return M

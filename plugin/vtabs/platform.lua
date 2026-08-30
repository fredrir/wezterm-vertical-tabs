local wezterm = require "wezterm" ---@type Wezterm

local triple = wezterm.target_triple or ""

local M = {
  triple = triple,
  is_mac = triple:find "apple%-darwin" ~= nil,
  is_windows = triple:find "windows" ~= nil,
}

M.SUPER = M.is_mac and "CMD" or "CTRL|SHIFT"
M.SUPER2 = M.is_mac and "CMD|SHIFT" or "CTRL|SHIFT|ALT"

-- wezterm-gui/src/termwindow/render/fancy_tab_bar.rs:378 reserves this much for the traffic lights.
M.BUTTON_PX = 70
-- macOS titlebar height; NEEDS-ENG-CONFIRM #1, over-reserving is the safe direction.
M.TITLEBAR_PX = 28

---Cells the top strip must keep clear, and where the toggle glyph goes. Pure, so it is testable
---without a macOS window: `dims` is a pane's `get_dimensions()`, `opts` the resolved config.
function M.strip_geometry(dims, opts)
  dims, opts = dims or {}, opts or {}
  local rows, cols = 0, 0
  local reserve = opts.is_mac
    and opts.integrated_buttons
    and opts.native_button_style
    and opts.position == "left"
    and not opts.is_full_screen
  if reserve and dims.cols and dims.cols > 0 and dims.viewport_rows and dims.viewport_rows > 0 then
    local cell_w = (dims.pixel_width or 0) / dims.cols
    local cell_h = (dims.pixel_height or 0) / dims.viewport_rows
    if cell_w > 0 and cell_h > 0 then
      cols = math.ceil(M.BUTTON_PX / cell_w)
      rows = math.max(1, math.ceil(M.TITLEBAR_PX / cell_h))
    end
  end
  local toggle_row = rows > 0 and rows or 1
  local toggle_x = cols > 0 and cols + 2 or (opts.card_x1 or 2)
  local strip_rows = math.max(rows, opts.toggle_button and toggle_row or 0) + (opts.padding_top or 0)
  return {
    rows = strip_rows,
    cols = cols,
    toggle_row = toggle_row,
    toggle_x = toggle_x,
  }
end

return M

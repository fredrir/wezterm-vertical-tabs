local H = require "support.helpers"
local wezterm = require "wezterm"
local config = require "vtabs.config"
local platform = require "vtabs.platform"

local test, eq, legacy = H.test, H.eq, H.legacy
local strip_geom, RETINA, here = H.strip_geom, H.RETINA, H.here

test("the reserve is ceil(70pt / cell width): 9 cols at 8pt, 8 at 9pt, 7 at 10-11pt, 6 at 12pt", function()
  local want = { [8] = 9, [9] = 8, [10] = 7, [11] = 7, [12] = 6 }
  for cell, cols in pairs(want) do
    local g = strip_geom { cols = 28, viewport_rows = 30, pixel_width = 28 * cell, pixel_height = 570 }
    eq(g.cols, cols, cell .. " pt cells")
    eq(g.cols, math.ceil(70 / cell), "and it is the formula, not a table")
    assert(g.cols <= 9, "9 is the widest reserve a readable cell can produce, never 11")
    eq(g.toggle_x, g.cols + 2, "11 is the toggle column at a 9-column reserve, not the reserve")
  end
end)

test("previewing the reserve off macOS divides by that host's own 1x, not by 72", function()
  -- the same 8.4 pt cell on every one of them: a preview that reads wider than a Mac is a wrong preview
  local cases = {
    { name = "mac 1x", dpi = platform.POINT_DPI, scale = 1 },
    { name = "mac 2x", dpi = platform.POINT_DPI * 2, scale = 2 },
    { name = "linux 1x", dpi = platform.LOGICAL_DPI, scale = 1, preview = true },
    { name = "linux 2x", dpi = platform.LOGICAL_DPI * 2, scale = 2, preview = true },
    { name = "windows 125%", dpi = 120, scale = 1.25, preview = true },
  }
  for _, case in ipairs(cases) do
    local g = strip_geom({
      cols = RETINA.cols,
      viewport_rows = RETINA.viewport_rows,
      pixel_width = RETINA.pixel_width * case.scale,
      pixel_height = RETINA.pixel_height * case.scale,
      dpi = case.dpi,
    }, { preview = case.preview })
    eq(g.cols, 9, case.name .. ": same logical cell, same reserve")
    assert(g.cols <= 9, case.name .. ": 9 is the widest a readable cell produces, never 11 or 12")
    eq(g.toggle_x, 11, case.name .. ": and the toggle still clears it")
  end
  -- and the flag is what does it: 96 dpi read as a 1.33x Mac is the bug
  local unscaled = strip_geom {
    cols = RETINA.cols,
    viewport_rows = RETINA.viewport_rows,
    pixel_width = RETINA.pixel_width,
    pixel_height = RETINA.pixel_height,
    dpi = platform.LOGICAL_DPI,
  }
  eq(unscaled.cols, 12, "without the preview flag a 96 dpi host over-reserves by three columns")
end)

test("a 2x display doubles the pixels and keeps the points, so the reserve does not move", function()
  local one_x = strip_geom(RETINA)
  local two_x = strip_geom {
    cols = RETINA.cols,
    viewport_rows = RETINA.viewport_rows,
    pixel_width = RETINA.pixel_width * 2,
    pixel_height = RETINA.pixel_height * 2,
    dpi = platform.POINT_DPI * 2,
  }
  eq(two_x.cols, one_x.cols, "the lights are 70 points wide on both")
  eq(two_x.rows_reserved, one_x.rows_reserved)
  eq(two_x.toggle_row, one_x.toggle_row)
  eq(two_x.toggle_x, one_x.toggle_x)
  eq(two_x.cols, 9)
  -- Device pixels alone would halve it, which is the bug this pins.
  eq(math.ceil(70 / (RETINA.pixel_width * 2 / RETINA.cols)), 5)
end)

test("the macOS strip reserves the traffic lights from the pane's own cell size", function()
  local g = strip_geom(RETINA)
  eq(g.cols, math.ceil(70 / (235 / 28)), "70 px of buttons, never a hardcoded column count")
  eq(g.cols, 9)
  eq(g.rows, 3, "max(reserve 2, toggle 1) + padding_top 1")
  -- The reserve is a row COUNT; the toggle must line up with the lights' centre, not sit below it.
  eq(g.toggle_row, 1, "beside the lights at a retina cell height")
  eq(g.toggle_x, 11, "clear of the reserve")
  local small = strip_geom { cols = 28, viewport_rows = 30, pixel_width = 235, pixel_height = 270 }
  eq(small.rows_reserved, 4, "a small font reserves more rows")
  eq(small.toggle_row, 2, "and the centre moves down with them, never past the reserve")
  assert(small.toggle_row <= small.rows_reserved, "always inside the reserve")
  local wide = strip_geom { cols = 28, viewport_rows = 30, pixel_width = 560, pixel_height = 570 }
  eq(wide.cols, 4, "bigger cells need fewer of them")
end)

test("every branch that is not a windowed left-hand macOS sidebar reserves no columns", function()
  eq(strip_geom(RETINA, { is_full_screen = true }).cols, 0)
  eq(strip_geom(RETINA, { position = "right" }).cols, 0)
  eq(strip_geom(RETINA, { native_button_style = false }).cols, 0)
  eq(strip_geom(RETINA, { integrated_buttons = false }).cols, 0)
  eq(strip_geom(RETINA, { is_mac = false }).cols, 0, "linux and windows")
  eq(strip_geom({}, {}).cols, 0, "no dimensions, no guess")
  eq(strip_geom({ cols = 0, viewport_rows = 0, pixel_width = 0, pixel_height = 0 }).cols, 0)
end)

test("the strip is toggle plus padding when nothing is reserved", function()
  local linux = { is_mac = false }
  eq(strip_geom(RETINA, linux).rows, 2, "padding_top sits above the toggle row, not below it")
  eq(strip_geom(RETINA, linux).toggle_row, 2, "so the glyphs never touch the window edge")
  eq(strip_geom(RETINA, { is_mac = false, toggle_button = false }).rows, 1, "padding without a toggle is still padding")
  eq(strip_geom(RETINA, { is_mac = false, padding_top = 0 }).rows, 1, "no padding, just the toggle row")
  eq(strip_geom(RETINA, { is_mac = false, padding_top = 0 }).toggle_row, 1)
  eq(strip_geom(RETINA, { is_mac = false, toggle_button = false, padding_top = 0 }).rows, 0)
  eq(strip_geom(RETINA, linux).toggle_x, 2, "card_x1")
  eq(strip_geom(RETINA, { is_mac = false, card_x1 = 4 }).toggle_x, 4)
end)

test("the shipped padding gives the toggle a one-row strip outside macOS", function()
  local g = strip_geom(RETINA, { is_mac = false, padding_top = config.defaults.padding.top })
  eq(g.rows, 1, "the shipped padding.top is 0: the window's half-cell is the air above")
  eq(g.toggle_row, 1)
  eq(math.min(g.toggle_row + 1, g.rows), 1, "so the hit band is the one row there is")
  assert(g.toggle_row <= g.rows, "P1-spec §9: the toggle row is inside the strip")
end)

test("the toggle span never reaches past the strip into the first card row", function()
  for _, over in ipairs {
    {},
    { is_mac = false },
    { is_full_screen = true },
    { position = "right" },
    { padding_top = 0 },
    { is_mac = false, padding_top = 0 },
    { padding_top = 3 },
  } do
    local g = strip_geom(RETINA, over)
    local span_last = math.min(g.toggle_row + 1, g.rows)
    assert(span_last <= g.rows, "toggle span inside the strip")
    assert(g.toggle_row <= g.rows, "toggle row inside the strip")
  end
end)

test("macOS window decorations are set only for a left sidebar the user has not configured", function()
  local vtabs = dofile(here .. "/../init.lua")
  assert(platform.is_mac, "the stub target triple is darwin")
  local function decorations(opts, preset)
    local cfg = { keys = {} }
    cfg.window_decorations = preset
    vtabs.apply_to_config(cfg, opts)
    return cfg.window_decorations
  end
  eq(decorations {}, "INTEGRATED_BUTTONS|RESIZE")
  eq(decorations { position = "right" }, nil, "a right sidebar reserves nothing, so it opts out")
  eq(decorations { titlebar = "plain" }, nil)
  eq(decorations({}, "TITLE|RESIZE"), "TITLE|RESIZE", "a user value is never overwritten")
  local before = #wezterm.log
  decorations({}, "RESIZE")
  assert(#wezterm.log > before, "RESIZE alone hides the buttons, so it warns")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("the window padding is zeroed on the sides the sidebar touches, and never when you set it", function()
  local vtabs = dofile(here .. "/../init.lua")
  local function padding(opts, preset)
    local cfg = { keys = {}, window_padding = preset }
    vtabs.apply_to_config(cfg, opts)
    return cfg.window_padding
  end
  local left = padding {}
  eq(left.left, 0, "the sidebar's own edge, so its background reaches it")
  eq(left.right, "1cell", "the far side keeps wezterm's own default")
  eq(left.top, "0.5cell", '"sides" is the default: the terminal keeps its own top and bottom half-cell')
  eq(left.bottom, "0.5cell")
  local right = padding { position = "right" }
  eq(right.right, 0, "mirrored for a right sidebar")
  eq(right.left, "1cell")
  local flush = padding { edge_to_edge = true }
  eq(flush.left, 0, "true reaches every edge the sidebar touches")
  eq(flush.top, 0, "at the cost of the content pane's own top and bottom")
  eq(flush.bottom, 0)
  local sides = padding { edge_to_edge = "sides" }
  eq(sides.left, 0, '"sides" still gives the sidebar its own edge')
  eq(sides.right, "1cell")
  eq(sides.top, "0.5cell", "but hands the content pane back wezterm's top and bottom half-cell")
  eq(sides.bottom, "0.5cell")
  -- Zen supersedes the asymmetric padding: the card grew by the inset, so the padding absorbs it.
  local zen = padding { frame = "zen", backend = { path = "/bin/wez-vtabs" } }
  eq(zen.left, 14, "margin + inset on every side")
  eq(zen.top, 14)
  eq(zen.right, 14)
  eq(zen.bottom, 14)
  eq(padding({ frame = { zen = true, margin = 10, inset = 0 }, backend = { path = "/bin/wez-vtabs" } }).top, 10)
  local mine = { left = 8, right = 8, top = 8, bottom = 8 }
  eq(padding({}, mine), mine, "a user value is never overwritten")
  eq(padding { edge_to_edge = false }, nil, "and the opt-out never touches it")
  eq(config.defaults.padding.left, 1, "one column, so window 0 + sidebar 1 is the 10 px the top gets in 11")
  eq(config.setup({ position = "right" }).padding.right, 1, "symmetric, so the mirror is a no-op")
  eq(config.setup({ position = "right" }).padding.left, 1)
  eq(config.defaults.padding.bottom, 0, "the window's half-cell is the air below, not a sidebar row")
  eq(config.setup({ position = "right", padding = { right = 4 } }).padding.right, 4, "yours wins")
  eq(
    config.setup({ position = "right", padding = { right = 4 } }).padding.left,
    config.defaults.padding.left,
    "untouched, unmirrored"
  )
  eq(
    config.setup({ position = "right", padding = 3 }).padding.right,
    config.defaults.padding.right,
    "a padding that is not a table"
  )
  eq(
    config.setup({ position = "right", padding = { left = -5 } }).padding.left,
    config.defaults.padding.left,
    "and one out of range"
  )
  eq(
    config.setup({ position = "right", padding = { left = -5 } }).padding.right,
    config.defaults.padding.right,
    "both come back mirrored"
  )
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local state = require "vtabs.state"
local hit = require "vtabs.hit"
local glyphs = require "vtabs.glyphs"
local model = require "vtabs.model"
local sidebar = require "vtabs.sidebar"
local fake = require "fake_mux"
local view_mod = require "vtabs.view"

local test, eq, frame_rows, title_row = H.test, H.eq, H.frame_rows, H.title_row
local legacy, p1_view, mark_ready = H.legacy, H.p1_view, H.mark_ready
local mouse, window, ready_window, strip_geom = H.mouse, H.window, H.ready_window, H.strip_geom

-- P1-spec §7, verbatim. Injected values in other tests cannot keep a wrong default green.
local P1_DEFAULTS = {
  width = 28,
  padding = { top = 0, left = 1, right = 1, bottom = 0 },
  edge_to_edge = "sides",
  row_gap = 0,
  tab_height = "card",
  meta = false,
  separator = "gap",
  pinned_style = "dense",
  new_tab_button = "ghost",
  new_tab_label = "New tab",
  corners = "chamfer",
  scroll_indicator = "auto",
  titlebar = "auto",
  toggle_button = true,
  close_button = "hover",
  show_index = false,
}

test("addendum 2 §1.5: padding.bottom gives the ghost the air the cards get, and edge_to_edge picks a band", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  eq(cfg.padding.bottom, 0, "the window's half-cell supplies the air below, not a sidebar row")
  eq(cfg.padding.left, cfg.padding.right, "one column either side, so left and right air match")
  local v = p1_view { rows = 20, opts = { separator = "gap" } }
  local rows, r = frame_rows(v)
  local last_ghost
  for row = 1, v.rows do
    last_ghost = r.hits[row].kind == "new_tab" and row or last_ghost
  end
  assert(last_ghost, "the ghost is drawn")
  for row = last_ghost + 1, v.rows do
    eq(r.hits[row].kind, "space", "row " .. row .. " below the ghost is reserved padding")
    eq(util.width(rows[row]), 28, "and painted, not left to whatever was there")
  end
  eq(v.rows - last_ghost, cfg.padding.bottom, "exactly padding.bottom rows of it")

  local deep =
    p1_view { rows = 20, opts = { separator = "gap", padding = { top = 1, left = 2, right = 2, bottom = 3 } } }
  local _, dr = frame_rows(deep)
  local deep_last
  for row = 1, deep.rows do
    deep_last = dr.hits[row].kind == "new_tab" and row or deep_last
  end
  eq(deep.rows - deep_last, 3, "the ghost never claims a row padding.bottom reserved")

  eq(config.setup({ edge_to_edge = true, backend = { path = "/bin/wez-vtabs" } }).edge_to_edge, true)
  eq(
    config.setup({ edge_to_edge = "top", backend = { path = "/bin/wez-vtabs" } }).edge_to_edge,
    "sides",
    "and nothing else"
  )
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("addendum 2 §1.6: the toggle row rounds to the lights' centre instead of ceiling past it", function()
  -- 14 pt below the top edge, with a half-cell of window padding above: pane row n is centred at
  -- n * cell_h, so round() is the nearest row and ceil() is a whole row low from 10 pt cells up
  for _, case in ipairs { { 9, 2 }, { 10, 1 }, { 11, 1 }, { 12, 1 }, { 13, 1 } } do
    local cell_h, want = case[1], case[2]
    local g = strip_geom {
      cols = 28,
      viewport_rows = 30,
      pixel_width = 28 * 8.4,
      pixel_height = 30 * cell_h,
    }
    eq(g.toggle_row, want, cell_h .. " pt cells")
    eq(g.toggle_row, math.max(1, math.min(math.floor(14 / cell_h + 0.5), g.rows_reserved)), "and it is the rule")
    assert(g.toggle_row <= g.rows_reserved, cell_h .. ": never past the reserve")
  end
end)

test("addendum 2 §1.6c: every chrome glyph is in the primary font's cmap, never decorative Unicode", function()
  local icons_mod = require "vtabs.icons"
  -- the stub has no nerdfonts table, so these resolve to ASCII: that is the point of the rule, a
  -- glyph the primary font does not carry is a fallback font's problem and looks like one
  eq(icons_mod.defaults.strip_new_tab, "+", "the strip trio is uniform and light")
  eq(icons_mod.defaults.settings, "⚙", "the gear is the recorded exception to the in-font rule")
  eq(icons_mod.defaults.go, wezterm.nerdfonts.seti_go or "G", "dev_go draws nothing in either Nerd Font")
  local resolved = glyphs.resolve(config.setup({ backend = { path = "/bin/wez-vtabs" } }).glyphs, {})
  for _, key in ipairs { "toggle_left", "toggle_right", "strip_new_tab", "settings", "close" } do
    eq(util.width(resolved[key]), 1, key .. " is one column, so the ASCII guard never fires")
  end
  eq(resolved.toggle_left, "❮")
  eq(resolved.toggle_right, "❯")
  local swapped = glyphs.resolve(
    config.setup({ icon_map = { toggle_left = "Z", settings = "S" }, backend = { path = "/bin/wez-vtabs" } }).glyphs,
    {}
  )
  eq(swapped.toggle_left, "Z", "and icon_map reaches every one of them")
  eq(swapped.settings, "S")
  eq(resolved.new_tab, "+", "the ghost card keeps its own plus, separate from the strip's")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("addendum 2 §1.6c: the action stride follows the lights' 20 pt pitch, not a cell count", function()
  local layout_mod = require "vtabs.layout"
  local platform_mod = require "vtabs.platform"
  local g = layout_mod.grid(config.setup { backend = { path = "/bin/wez-vtabs" } }, 28)
  for _, case in ipairs { { 6, 3 }, { 7, 3 }, { 8, 3 }, { 10, 2 }, { 12, 2 } } do
    local cell_w, want = case[1], case[2]
    local cluster = layout_mod.strip_actions(
      config.setup { backend = { path = "/bin/wez-vtabs" } },
      { cols = 0, toggle_row = 1, cell_w = cell_w },
      g,
      false,
      28
    )
    assert(#cluster >= 2, cell_w .. " pt: the cluster fits")
    eq(cluster[2].x - cluster[1].x, want, cell_w .. " pt cells: " .. want .. " columns per 20 pt")
    eq(cluster[1].x2 - cluster[1].x1 + 1, want, "and the span is exactly one stride wide")
    eq(cluster[2].x1, cluster[1].x2 + 1, "so neighbours stay contiguous with no dead cell between")
    eq(want, math.max(2, math.floor(platform_mod.BUTTON_PITCH_PT / cell_w + 0.5)), "and it is the rule")
  end
  local no_cells = layout_mod.strip_actions(
    config.setup { backend = { path = "/bin/wez-vtabs" } },
    { cols = 0, toggle_row = 1 },
    g,
    false,
    28
  )
  eq(no_cells[2].x - no_cells[1].x, 3, "with no cell size to go on, three columns")
end)

test("the shipped defaults are the §7 table", function()
  local shipped = config.defaults
  for key, want in pairs(P1_DEFAULTS) do
    if type(want) == "table" then
      for field, value in pairs(want) do
        eq(shipped[key][field], value, key .. "." .. field)
      end
    else
      eq(shipped[key], want, key)
    end
  end
  eq(shipped.theme.use_scheme_tab_bar, nil, "the deprecated key is gone from the defaults")
  local resolved = config.setup {}
  for key, want in pairs(P1_DEFAULTS) do
    if type(want) ~= "table" then
      eq(resolved[key], want, "setup keeps " .. key)
    end
  end
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("tab_height accepts the row counts as well as the names", function()
  eq(config.setup({ tab_height = 2 }).tab_height, "card")
  eq(config.setup({ tab_height = 1 }).tab_height, "row")
  eq(config.setup({ tab_height = 3 }).tab_height, "tall")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("a dragged card paints its meta row in drag_fg, the loudest colour on that surface", function()
  local win = ready_window()
  local wid = win.gui:window_id()
  local cards = model.ordered(model.build(win.gui))
  for _, item in ipairs(cards) do
    item.meta = "~/projects/api"
  end
  -- An unmistakable drag_fg, so "which colour painted this row" needs no inference.
  local resolved = theme.resolve({ drag_fg = "#ff00ff" }, fake.palette)
  local cfg = config.get()
  local function frame(drag)
    return render.render {
      cols = 28,
      rows = 14,
      items = cards,
      theme = resolved,
      cfg = cfg,
      glyphs = cfg.glyphs,
      scroll = 0,
      strip = { rows = 1 },
      drag = drag,
    }
  end
  local idle = frame(nil)
  local dragged = frame { tab_id = cards[1].tab_id, over_index = 1, active = true }
  local function meta_row(r)
    for row = 1, 14 do
      if r.hits[row] and r.hits[row].part == "meta" and r.hits[row].id == cards[1].tab_id then
        return row
      end
    end
  end
  local row = meta_row(dragged)
  assert(row, "the drag chip still has a meta row")
  local function row_has(data, y, colour)
    local seg = data:match("\27%[" .. y .. ";1H(.-)\27%[" .. (y + 1) .. ";1H") or ""
    return seg:find("38;2;" .. table.concat(colour, ";"), 1, true) ~= nil
  end
  assert(row_has(dragged.data, row, resolved.drag_fg), "drag colours, not meta_fg")
  assert(not row_has(idle.data, meta_row(idle) or 1, resolved.drag_fg), "an idle card is unaffected")
  local shipped = theme.resolve({}, fake.palette)
  assert(
    theme.contrast(shipped.drag_fg, shipped.drag_bg) >= math.min(3.5, theme.contrast(shipped.fg, shipped.drag_bg)),
    "and by default it is the best the palette can do on drag_bg"
  )
  state.session.drag[wid] = nil
end)

test("a footer row is a target in its own right, never empty space", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local clicked = 0
  local cfg = config.get()
  cfg.hooks.footer = function()
    return {
      { text = "inert" },
      {
        text = "live",
        id = "x",
        on_click = function()
          clicked = clicked + 1
        end,
      },
    }
  end
  view_mod.sync(gui, { force = true })
  local hits = state.session.hits[sb:pane_id()]
  local inert, live
  for row = 1, 24 do
    if hits[row] and hits[row].kind == "footer" then
      if hits[row].entry.on_click then
        live = row
      else
        inert = inert or row
      end
    end
  end
  assert(inert and live, "both footer rows are hit records")
  local before = #win.tab_list
  for _ = 1, 2 do
    mouse(gui, sb, "down", "left", 5, inert)
  end
  eq(#win.tab_list, before, "double-clicking a footer row without on_click opens nothing")
  mouse(gui, sb, "down", "left", 5, live)
  eq(clicked, 1, "and a row with on_click still fires it")
  cfg.hooks.footer = nil
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a hover repaint tracks any span, not just the close button", function()
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  eq(hit.in_close, nil, "the shim is gone")
  local sent = #sb.sent
  local row = title_row(sb, win.tab_list[2].id)
  mouse(gui, sb, "move", "none", 5, row)
  local painted = #sb.sent
  assert(painted > sent, "entering a card repaints")
  mouse(gui, sb, "move", "none", 26, row)
  assert(#sb.sent > painted, "crossing into the close span repaints again")
end)

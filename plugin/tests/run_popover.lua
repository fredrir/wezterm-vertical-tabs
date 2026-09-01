local H = require "support.helpers"
local util = require "vtabs.util"
local config = require "vtabs.config"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local input = require "vtabs.input"
local popover = require "vtabs.popover"
local view_mod = require "vtabs.view"
local palettes = require "palettes"

local test, eq, rgb, hex = H.test, H.eq, H.rgb, H.hex
local window = H.window
local ready_window, open_popover, here = H.ready_window, H.open_popover, H.here

test("the popover surface is never harder to read than the sidebar body", function()
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local ceiling = math.min(4.5, 0.95 * theme.contrast(t.fg, t.bg))
    local where = " on " .. p.name
    assert(theme.contrast(t.fg, t.surface_raised) >= ceiling - 0.001, "fg vs surface_raised" .. where)
    assert(theme.contrast(t.meta_fg, t.surface_raised) >= 3.5 - 0.001, "meta_fg vs surface_raised" .. where)
    assert(theme.contrast(t.fg, t.surface_raised) < theme.contrast(t.fg, t.bg), "raised is a surface" .. where)
    -- disabled is quiet by design; it only has to stay a colour, not a gate
    assert(theme.contrast(t.disabled_fg, t.surface_raised) > 1.5, "disabled_fg still visible" .. where)
  end
  -- Only the two low-contrast schemes need the lift lowered below 0.09.
  local lowered = {}
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local dark = theme.luminance(t.bg) < 0.5
    local full = theme.mix(t.bg, t.fg, 0.09 * (dark and 1.0 or 0.6))
    if rgb(t.surface_raised) ~= rgb(full) then
      lowered[p.name] = true
    end
  end
  eq(rgb(util.sorted_keys(lowered)), rgb { "Solarized Dark", "Solarized Light" })
end)

test("the scrim is a contrast target: every palette lands in the same narrow band", function()
  local lo, hi = 99, 0
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local where = " on " .. p.name
    assert(t.scrim >= 0.30 and t.scrim <= 0.70, "scrim inside its range" .. where)
    local fg = theme.contrast(theme.mix(t.fg, t.bg, t.scrim), t.bg)
    local card = theme.contrast(theme.mix(t.active_bg, t.bg, t.scrim), t.bg)
    assert(fg >= 2.0 and fg <= 3.0, "scrimmed text stays legible but recedes" .. where .. ": " .. fg)
    assert(card < 1.3, "the scrimmed active card stops reading as a block" .. where .. ": " .. card)
    lo, hi = math.min(lo, fg), math.max(hi, fg)
  end
  assert(hi / lo < 1.2, "a fixed fade would spread 2.5x; the target keeps it under 1.2x")
end)

test("the new surfaces are overridable like every other theme key", function()
  local over = theme.resolve({ surface_raised = "#010203", disabled_fg = "#040506", scrim = 0.5 }, palettes[1])
  eq(rgb(over.surface_raised), "1,2,3")
  eq(rgb(over.disabled_fg), "4,5,6")
  eq(over.scrim, 0.5)
end)

test("the sidebar page is tinted by default; the frame gutter keeps the terminal background", function()
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local where = " on " .. p.name
    assert(rgb(t.bg) ~= hex(p.background), "the default tint is visible" .. where)
    eq(rgb(t.content_bg), hex(p.background), "content_bg is never tinted" .. where)
    eq(rgb(t.bg), rgb(theme.mix(t.content_bg, t.fg, 0.06 * (theme.luminance(t.bg) < 0.5 and 1 or 1))), where)
  end
  eq(config.setup({}).theme.elevation, 0.06, "the shipped default")
  local seamless = theme.resolve({ elevation = 0 }, palettes[1])
  eq(rgb(seamless.bg), hex(palettes[1].background), "0 is still the seamless option")
  -- Out of range resets to the default rather than painting the sidebar in the foreground.
  eq(config.setup({ theme = { elevation = 1 } }).theme.elevation, 0.06)
  eq(config.setup({ theme = { elevation = 0 } }).theme.elevation, 0)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("wezterm is told not to dim the idle pane, unless the user asked for dimming", function()
  local vtabs = dofile(here .. "/../init.lua")
  local function hsb(opts, preset)
    local cfg = { keys = {} }
    cfg.inactive_pane_hsb = preset
    vtabs.apply_to_config(cfg, opts)
    return cfg.inactive_pane_hsb
  end
  local off = hsb {}
  eq(off.brightness, 1.0, "the sidebar is chrome; wezterm would dim it whenever the shell has focus")
  eq(off.saturation, 1.0)
  eq(hsb { dim_inactive_panes = true }, nil, "opting in leaves wezterm's default alone")
  -- The frame is no excuse: wezterm skips only the pane's default fill under a background layer and
  -- goes on dimming explicit-bg cells, which is every cell the sidebar paints. Without this the
  -- sidebar rendered (38,38,52) against a frame tint of (41,41,58).
  local zen = hsb { frame = "zen", backend = { path = "/bin/wez-vtabs" } }
  eq(zen.brightness, 1.0, "a background layer does not spare the sidebar's own cells")
  eq(zen.saturation, 1.0)
  local mine = { brightness = 0.5, saturation = 0.5 }
  eq(hsb({}, mine), mine, "a user value is never overwritten")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("cell counts and durations must be whole numbers", function()
  eq(config.setup({ width = 28.7 }).width, 28, "a fractional width would reach AdjustPaneSize")
  eq(config.setup({ row_gap = 1.5 }).row_gap, 0)
  eq(config.setup({ rail_width = 5.5 }).rail_width, 5)
  eq(config.setup({ poll_ms = 500.5 }).poll_ms, 500)
  eq(config.setup({ padding = { top = 1.2 } }).padding.top, 0)
  eq(config.setup({ tooltip_delay_ms = 600.5 }).tooltip_delay_ms, 600)
  eq(config.setup({ animation = { fps = 30.5 } }).animation.fps, 30)
  eq(config.setup({ width = 32 }).width, 32, "a whole number survives")
  eq(config.setup({ theme = { elevation = 0.06 } }).theme.elevation, 0.06, "ratios still take fractions")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

---A foreground process the skip list does not name is what makes a close want confirming.
local function make_busy(tab)
  for _, p in ipairs(tab.pane_list) do
    if not sidebar.is_backend(p) then
      p.process = "/usr/bin/sleep"
    end
  end
end

test("the menu's close items raise the same confirm level, and Cancel leaves the tabs alone", function()
  local win, gui = open_popover(1)
  make_busy(win.tab_list[1])
  popover.run(gui, "close")
  eq(popover.get(gui:window_id()).level, "confirm", "the menu asks instead of closing")
  eq(#win.tab_list, 3)
  popover.run(gui, "confirm_cancel")
  eq(popover.get(gui:window_id()), nil)
  eq(#win.tab_list, 3, "Cancel closed nothing")

  local others, others_gui = open_popover(3)
  for _, tab in ipairs(others.tab_list) do
    make_busy(tab)
  end
  popover.run(others_gui, "close_others")
  local pop = popover.get(others_gui:window_id())
  eq(pop.confirm, "close_others")
  eq(pop.count, 2, "the tabs that are not the anchor")
  local head = popover.wire_body(others_gui).header
  assert(head.title:find "^Close ", "the question names the first victim")
  eq(head.meta, "and 1 other", "then how many more follow it")
  popover.run(others_gui, "confirm_close")
  eq(#others.tab_list, 1, "and Close takes them all")
end)

test("a tab the skip list names closes without a question, and so does confirm_close = false", function()
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  local function ask_close(g, pane, tab)
    input.handle(g, pane, "vtabs", '{"t":"do","a":"request_close","id":' .. tab.id .. ',"args":{"row":3,"col":26}}')
  end
  ask_close(gui, sb, win.tab_list[1])
  eq(popover.get(gui:window_id()), nil, "zsh is on the skip list")
  eq(#win.tab_list, 2)

  local opted, opted_gui = ready_window()
  local sb2 = sidebar.find(opted.tab_list[1])
  make_busy(opted.tab_list[1])
  config.setup { meta = "auto", confirm_close = false, backend = { path = "/bin/wez-vtabs" } }
  view_mod.sync(opted_gui, { force = true })
  ask_close(opted_gui, sb2, opted.tab_list[1])
  eq(popover.get(opted_gui:window_id()), nil, "confirm_close = false never asks")
  eq(#opted.tab_list, 2)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)
test("a busy tab in a sidebar too narrow to ask falls back to wezterm's own confirmation", function()
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  make_busy(win.tab_list[1])
  local narrow = sb.cols
  sb.cols = 10
  local acted = #win.actions
  actions.request_close(gui, win.tab_list[1].id, 3, 2)
  eq(popover.get(gui:window_id()), nil, "no unreadable question is opened")
  assert(#win.actions > acted, "wezterm is asked instead")
  eq(win.actions[#win.actions].action.arg.confirm, true, "with its own overlay, which a key can use")
  sb.cols = narrow
end)

test("the selected menu row is an accent fill that clears 4.5 on all ten palettes", function()
  local schemes = require "palettes"
  for _, p in ipairs(schemes) do
    local t = theme.resolve({}, p)
    local where = " on " .. p.name
    assert(theme.contrast(t.popover_sel_fg, t.popover_sel_bg) >= 4.5, "selected text" .. where)
    assert(theme.contrast(t.popover_sel_bg, t.surface_raised) >= 2.5, "fill against the panel" .. where)
    assert(theme.contrast(t.popover_sel_hint, t.popover_sel_bg) >= 3.0, "hint" .. where)
    local ink = rgb(t.popover_sel_fg)
    assert(ink == "0,0,0" or ink == "255,255,255", "the ink is one of the two absolutes" .. where)
    -- The construction this replaced: hover_bg on surface_raised was the same colour in practice.
    assert(theme.contrast(t.hover_bg, t.surface_raised) < 1.2, "which the old one never was" .. where)
  end
end)

test("the menu shows the key bound to an item, which no binding ever carried before", function()
  local bindings = keys.build {}
  local named = 0
  for _, b in ipairs(bindings) do
    if b.vtabs then
      named = named + 1
    end
  end
  eq(named, #bindings, "every binding names itself, or the hints have nothing to match on")

  local win, gui = window(1)
  local menu = popover.items(gui, win.tab_list[1].id)
  local close = nil
  for _, item in ipairs(menu) do
    if item.id == "close" then
      close = item
    end
  end
  assert(close, "the menu offers Close tab")
  assert(close.hint and close.hint ~= "", "and shows the key bound to it, got " .. tostring(close.hint))
  assert(close.hint:find "W", "which is the close_tab binding: " .. close.hint)

  -- wezterm rejects a key entry carrying a field it does not know, so the name never reaches it.
  local cfg = { keys = {} }
  keys.apply(cfg, config.setup {})
  assert(#cfg.keys > 0)
  for _, b in ipairs(cfg.keys) do
    eq(b.vtabs, nil, "no binding handed to wezterm carries the name")
  end
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

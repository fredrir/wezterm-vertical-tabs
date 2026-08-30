local H = require "support.helpers"
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local view_mod = require "vtabs.view"
local platform = require "vtabs.platform"
local frame_mod = require "vtabs.frame"

local test, eq, mark_ready, window, here = H.test, H.eq, H.mark_ready, H.window, H.here

test('frame = "zen" is opt-in, and its renderer is only ever a local one', function()
  eq(frame_mod.enabled(config.setup { backend = { path = "/bin/wez-vtabs" } }), false, "default off")
  assert(frame_mod.enabled(config.setup { frame = "zen", backend = { path = "/bin/wez-vtabs" } }))
  assert(frame_mod.enabled(config.setup { frame = { zen = true }, backend = { path = "/bin/wez-vtabs" } }))
  eq(frame_mod.enabled(config.setup { frame = { margin = 4 }, backend = { path = "/bin/wez-vtabs" } }), false)

  -- Z1: the table and function forms are keyed by remote host or domain, so a path written for an
  -- ssh box would become a local execve target.
  eq(frame_mod.renderer(config.setup { frame = "zen", backend = { path = "/bin/wez-vtabs" } }), "/bin/wez-vtabs")
  eq(frame_mod.renderer(config.setup { frame = "zen", backend = { path = { archie = "/evil" } } }), nil)
  eq(
    frame_mod.renderer(config.setup {
      frame = "zen",
      backend = {
        path = function()
          return "/evil"
        end,
      },
    }),
    nil
  )
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the frame card is sized from the window, refuses nonsense, and stays inside it", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  local cfg = config.setup { frame = "zen", meta = "auto", backend = { path = "/bin/wez-vtabs" } }
  local rect = frame_mod.rect(gui, cfg)
  assert(rect, "a rect for a normal window")
  eq(rect.y, 8, "the margin is the frame's own")
  assert(rect.x >= 8, "the card starts past the sidebar, at " .. rect.x)
  assert(rect.x + rect.cw <= rect.w, "and ends inside the window")
  assert(rect.ch + 16 <= rect.h)

  -- Z3: refused, not clamped. A 60000x1 window clears any per-side cap and still asks for 240 MB.
  local real = getmetatable(gui).get_dimensions
  for _, bad in ipairs {
    { pixel_width = 0, pixel_height = 100 },
    { pixel_width = 60000, pixel_height = 1 },
    { pixel_width = 12000, pixel_height = 12000 },
    { pixel_width = 100 },
    { pixel_width = 1 / 0, pixel_height = 100 },
  } do
    gui.get_dimensions = function()
      return bad
    end
    eq(frame_mod.rect(gui, cfg), nil, "refused " .. tostring(bad.pixel_width))
  end
  gui.get_dimensions = real
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the frame declines to a background of the user's own, and to transparency", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  config.setup { frame = "zen", meta = "auto", backend = { path = "/bin/wez-vtabs" } }
  eq(frame_mod.refuses(gui), nil, "an untouched window is fair game")
  -- wezterm hands back an empty list for an unset `background`, which is not a user's choice.
  gui.overrides = { background = {} }
  eq(frame_mod.refuses(gui), nil, "an empty list is not a background")
  gui.overrides = { window_background_opacity = 0.9 }
  eq(frame_mod.refuses(gui), "window_background_opacity", "transparency composites through the frame")
  gui.overrides = { text_background_opacity = 0.5 }
  eq(frame_mod.refuses(gui), "text_background_opacity")
  gui.overrides = { window_background_image = "/tmp/x.png" }
  eq(frame_mod.refuses(gui), "window_background_image")
  gui.overrides = { background = { { source = { File = "/tmp/mine.png" } } } }
  eq(frame_mod.refuses(gui), "background", "a real one is theirs, not ours to replace")
  -- Z6: `install` writes exactly one layer, so a second is theirs however ours got to the front.
  gui.overrides = { background = { { source = { File = "/tmp/mine.png" } }, { source = { File = "/tmp/b.png" } } } }
  eq(frame_mod.refuses(gui), "background", "a layer appended after ours is still a background")
  gui.overrides = nil
  eq(frame_mod.sync(gui), false, "and sync does nothing at all while it declines")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("installing the frame keeps the padding the titlebar band owns in the same table", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  local cfg = config.setup { frame = "zen", meta = "auto", backend = { path = "/bin/wez-vtabs" } }
  gui.overrides = { colors = { cursor_bg = "#ff0000" }, front_end = "WebGpu" }
  frame_mod.install(gui, "/tmp/frame_mod.png", cfg)
  local out = gui:get_config_overrides()
  eq(out.front_end, "WebGpu", "a key the frame does not own survives")
  eq(out.colors.cursor_bg, "#ff0000", "and so does one inside a table it does touch")
  eq(#out.background, 1, "one layer")
  eq(out.background[1].source.File, "/tmp/frame_mod.png")
  eq(out.background[1].repeat_x, "NoRepeat")
  eq(out.background[1].width, "100%")
  -- Z5: `apply_padding` composes margin + inset and `apply_titlebar_band` holds the override, so a
  -- write from the frame would reset the band's top on the next resize.
  eq(out.window_padding, nil, "the frame never writes the key the band owns")
  eq(out.colors.split, frame_mod.colours(gui, cfg).card, "splits vanish into the card, not the tint")
  gui.overrides = nil
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("zen + collapsed = hidden keeps the traffic-light band across a resize", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  gui.decorations = "INTEGRATED_BUTTONS|RESIZE"
  gui.window_padding = { left = 14, right = 14, top = 14, bottom = 14 }
  local was_mac = platform.is_mac
  platform.is_mac = true
  view_mod.invalidate_theme()
  local cfg = config.setup {
    backend = { path = "/bin/wez-vtabs" },
    frame = "zen",
    collapsed = "hidden",
    meta = "auto",
  }

  state.set_collapsed(gui:window_id(), true)
  assert(view_mod.apply_titlebar_band(gui), "collapsing bands the window")
  eq(gui:get_config_overrides().window_padding.top, platform.TITLEBAR_PAD)

  -- The resize the band exists to survive: a new rect reinstalls the frame.
  frame_mod.install(gui, "/tmp/frame_resize.png", cfg)
  eq(
    gui:get_config_overrides().window_padding.top,
    platform.TITLEBAR_PAD,
    "the lights still clear the shell after the frame reinstalls"
  )
  eq(gui:get_config_overrides().window_padding.left, 14, "and the frame's own sides are untouched")

  state.set_collapsed(gui:window_id(), false)
  assert(view_mod.apply_titlebar_band(gui), "expanding clears the band")
  eq(gui:get_config_overrides().window_padding, nil, "back to the base config's margin + inset")

  platform.is_mac = was_mac
  gui.decorations, gui.window_padding, gui.overrides = nil, nil, nil
  state.set_collapsed(gui:window_id(), false)
  view_mod.invalidate_theme()
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the frame inset lifts the text off the stroke without moving the card's outer edge", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  local base = config.setup {
    frame = { zen = true, inset = 0 },
    meta = "auto",
    backend = { path = "/bin/wez-vtabs" },
  }
  local flat = frame_mod.rect(gui, base)
  local cfg = config.setup { frame = "zen", meta = "auto", backend = { path = "/bin/wez-vtabs" } }
  eq(frame_mod.inset(cfg), 6, "six device pixels by default")
  local inset = frame_mod.rect(gui, cfg)

  -- The grid shrank by 2*inset and the card grew by it, so the two cancel at the outer edge: the
  -- gutter is `margin` on all four sides whatever the inset is.
  local m = frame_mod.margin(cfg)
  eq(inset.y, m, "the card's top gutter is the margin")
  eq(inset.y + inset.ch, inset.h - m, "and so is its bottom")
  eq(inset.x + inset.cw, inset.w - m, "the right edge stays a margin from the window")
  eq(flat.x + flat.cw, flat.w - m, "which is where a flat card ended too")
  eq(inset.y, flat.y, "the top does not move")
  eq(inset.ch, flat.ch)
  -- The text rect it is drawn around is 2*inset narrower, so the card itself is wider.
  assert(inset.cw > flat.cw, "the card covers the air the padding opened, at " .. inset.cw)

  -- An inner gap larger than the outer frame reads inside-out.
  eq(frame_mod.inset(config.setup { frame = { zen = true, inset = 99 }, backend = { path = "/x" } }), 8)
  eq(frame_mod.inset(config.setup { frame = { zen = true, inset = -4 }, backend = { path = "/x" } }), 0)
  eq(
    frame_mod.inset(config.setup { frame = { zen = true, margin = 2, inset = 6 }, backend = { path = "/x" } }),
    2,
    "the margin is the ceiling"
  )
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the frame's file name carries the colours and the process, not just the geometry", function()
  local rect = { w = 100, h = 80, x = 8, y = 8, cw = 60, ch = 64, radius = 8 }
  local dark = { fill = "#29293a", card = "#1e1e2e", border = "#45475a" }
  local light = { fill = "#e6e9ef", card = "#eff1f5", border = "#9ca0b0" }
  -- Z7: a theme change must not match a file written for the old colours.
  assert(frame_mod.path_for(1, rect, dark) ~= frame_mod.path_for(1, rect, light), "colour is in the key")
  eq(frame_mod.path_for(1, rect, dark), frame_mod.path_for(1, rect, dark), "and it is stable")
  assert(frame_mod.path_for(1, rect, dark) ~= frame_mod.path_for(2, rect, dark), "window is still in it")
  -- Window ids restart per GUI process, so the process has to be in the name too.
  assert(frame_mod.path_for(1, rect, dark):match "/frame%-[^-]+%-1%-", "the process key precedes the window")
end)

test("every vtabs module is on the config-reload watch list", function()
  local vtabs = dofile(here .. "/../init.lua")
  local watched = {}
  for _, name in ipairs(vtabs.module_names()) do
    watched[name] = true
  end
  local listing = io.popen('ls "' .. here .. '/../vtabs"')
  local missing = {}
  local seen = 0
  for line in listing:lines() do
    local name = line:match "^(.+)%.lua$"
    if name then
      seen = seen + 1
      if not watched[name] then
        missing[#missing + 1] = name
      end
    end
  end
  listing:close()
  assert(seen > 10, "the listing found the modules")
  eq(table.concat(missing, ","), "", "modules not watched, so edits to them would not reload")
end)

test("a toggle sends the fade around its single resize, and only on a local domain", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  view_mod.sync(gui, { force = true })
  local function anims_since(n)
    local out = {}
    for i = n + 1, #sb.sent do
      local phase = sb.sent[i]:match '"t":"anim".-"phase":"([%a_]+)"' or sb.sent[i]:match '"phase":"([%a_]+)"'
      if sb.sent[i]:find('"anim"', 1, true) then
        out[#out + 1] = phase or "?"
      end
    end
    return out
  end
  local before = #sb.sent
  local actions_before = #win.actions
  actions.toggle_sidebar(gui)
  local sent = anims_since(before)
  assert(#sent >= 1, "the collapse is animated")
  local resizes = 0
  for i = actions_before + 1, #win.actions do
    if win.actions[i].action.action == "AdjustPaneSize" then
      resizes = resizes + 1
    end
  end
  eq(resizes, 1, "one pane resize per toggle, whatever the fade does")

  before = #sb.sent
  sb.domain = "SSH:archie"
  actions.toggle_sidebar(gui)
  eq(#anims_since(before), 0, "animations = auto is off for a remote domain")
  sb.domain = "local"

  config.setup { backend = { path = "/bin/wez-vtabs" }, animations = false }
  before = #sb.sent
  actions.toggle_sidebar(gui)
  eq(#anims_since(before), 0, "and off entirely when asked")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
  sidebar.set_collapsed(gui, false)
end)

test("collapsed = hidden bands the window so the macOS lights clear the shell", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  gui.decorations = "INTEGRATED_BUTTONS|RESIZE"
  gui.window_padding = { left = 4, right = 4, top = 0, bottom = 2 }
  local was_mac = platform.is_mac
  platform.is_mac = true
  view_mod.invalidate_theme()
  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden" }

  eq(gui:get_config_overrides().window_padding, nil, "nothing is overridden while expanded")
  state.set_collapsed(gui:window_id(), true)
  assert(view_mod.apply_titlebar_band(gui), "collapsing applies the band")
  local padded = gui:get_config_overrides().window_padding
  eq(padded.top, platform.TITLEBAR_PAD, "the band is the light reserve, in dpi-scaled points")
  eq(padded.left, 4, "the user's other sides are kept")
  eq(padded.bottom, 2)
  eq(view_mod.apply_titlebar_band(gui), false, "and it is idempotent")
  assert(state.applying_recently(gui:window_id()), "the reload it triggers is marked as ours")

  state.set_collapsed(gui:window_id(), false)
  assert(view_mod.apply_titlebar_band(gui), "expanding clears it")
  eq(gui:get_config_overrides().window_padding, nil, "back to the user's own padding")

  -- The rail never needs the band: its own pane still owns the window's top-left.
  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "rail" }
  state.set_collapsed(gui:window_id(), true)
  eq(view_mod.apply_titlebar_band(gui), false, "a rail is not banded")
  eq(gui:get_config_overrides().window_padding, nil)

  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden", rail_titlebar = "none" }
  eq(view_mod.apply_titlebar_band(gui), false, "rail_titlebar = none declines")
  eq(gui:get_config_overrides().window_padding, nil, "and leaves window_padding alone")

  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden" }
  gui.full_screen = true
  eq(view_mod.apply_titlebar_band(gui), false, "fullscreen has no titlebar to clear")
  gui.full_screen = false
  gui.decorations = "RESIZE"
  view_mod.invalidate_theme()
  eq(view_mod.apply_titlebar_band(gui), false, "and neither does a window without the buttons")

  platform.is_mac = was_mac
  gui.decorations, gui.window_padding = nil, nil
  state.set_collapsed(gui:window_id(), false)
  view_mod.invalidate_theme()
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

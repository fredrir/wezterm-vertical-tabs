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
  eq(out.window_padding.left, 8, "the margin is the padding on every side")
  eq(out.window_padding.bottom, 8)
  assert(out.colors.split, "the divider is hidden in the frame tint")
  gui.overrides = nil
  config.setup { backend = { path = "/bin/wez-vtabs" } }
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

local H = require "support.helpers"
local wezterm = require "wezterm"
local sidebar = require "vtabs.sidebar"
local mux = require "vtabs.mux"

local test, eq = H.test, H.eq

test("the mux facade answers a dead handle with nil, silently, so no caller guards", function()
  local dead = setmetatable({}, {
    __index = function()
      return function()
        error "pane is not valid"
      end
    end,
  })
  local before = #wezterm.log
  for _, name in ipairs {
    "tab_id",
    "panes",
    "panes_with_info",
    "active_pane",
    "tab_of",
    "title",
    "dims",
    "domain",
    "foreground",
    "cwd",
    "user_vars",
    "unseen",
    "tabs_with_info",
    "active_tab",
    "window_id",
    "effective_config",
    "overrides",
  } do
    eq(mux[name](dead), nil, name .. " answers nil")
    -- nil is a dead handle too, so a caller that lost its receiver needs no branch either.
    eq(mux[name](nil), nil, name .. " answers nil for a lost receiver")
  end
  eq(select("#", mux.call(dead, "activate")), 0, "call answers nothing at all")
  eq(select("#", mux.call(nil, "activate")), 0)
  -- A window closing mid-poll would otherwise spray one warning per pane.
  eq(#wezterm.log, before, "the facade is silent")
end)

test("the mux facade reads a live handle through, keeping wezterm's own shape", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  eq(mux.tab_id(tab), tab:tab_id())
  eq(mux.window_id(gui), gui:window_id())
  eq(#mux.panes(tab), #tab:panes())
  eq(#mux.tabs_with_info(gui:mux_window()), 2)
  eq(mux.domain(sb), "local")
  eq(mux.title(sb), sb:get_title())
  eq(mux.tab_of(sb), tab)
  eq(mux.active_tab(gui:mux_window()), win.active_tab_ref)
  -- `dims` keeps the raw shape: strip_geometry and the frame read the pixel pair and the dpi.
  local d = mux.dims(sb)
  eq(type(d), "table")
  eq(d.cols, sb.cols)
  assert(d.pixel_width ~= nil and d.dpi ~= nil, "pixel_width and dpi survive verbatim")
  eq(mux.dims(gui).is_full_screen, false)
end)

-- Standalone config for driving the plugin in a throwaway WezTerm instance.
local wezterm = require "wezterm"
local root = os.getenv "VTABS_ROOT"
package.path = root .. "/plugin/?.lua;" .. root .. "/plugin/?/init.lua;" .. package.path

local vtabs = dofile(root .. "/plugin/init.lua")
local config = wezterm.config_builder()

config.default_prog = { "/bin/sh" }
config.initial_cols = 100
config.initial_rows = 30
config.window_close_confirmation = "NeverPrompt"
config.exit_behavior = "Close"

local mux_socket = os.getenv "VTABS_E2E_MUX"
if mux_socket then
  config.unix_domains = { { name = "e2emux", socket_path = mux_socket } }
  config.default_domain = "e2emux"
end

config.unix_domains = config.unix_domains or {}

local function sidebar_cols(window)
  local out = {}
  for _, info in ipairs(window:mux_window():tabs_with_info()) do
    for _, pane in ipairs(info.tab:panes()) do
      local title = pcall(pane.get_title, pane) and pane:get_title() or ""
      if title:match "^wez%-vtabs:" then
        out[#out + 1] = tostring(pane:get_dimensions().cols)
      end
    end
  end
  return table.concat(out, ",")
end

-- Registered before the plugin, so it reports the width WezTerm dealt out before geometry corrects it.
wezterm.on("window-resized", function(window)
  wezterm.log_info("e2e: sidebar cols on resize " .. sidebar_cols(window))
end)

vtabs.apply_to_config(config, {
  poll_ms = 200,
  debug = true,
  confirm_close = false,
  domain = os.getenv "VTABS_E2E_DOMAIN" or "CurrentPaneDomain",
  backend = { path = os.getenv "VTABS_BIN" },
  icons = false,
  collapsed = os.getenv "VTABS_E2E_COLLAPSED" or nil,
})

local probes = {
  toggle = function(window)
    vtabs.toggle_sidebar(window)
  end,
  toggle_ms = function(window)
    local util = require "vtabs.util"
    local before = util.now_ms()
    vtabs.toggle_sidebar(window)
    wezterm.log_info("e2e: toggle ms " .. tostring(util.now_ms() - before))
  end,
  attach_ms = function(window)
    local util = require "vtabs.util"
    local sidebar = require "vtabs.sidebar"
    local n, worst = 0, 0
    for _, info in ipairs(window:mux_window():tabs_with_info()) do
      if not sidebar.find(info.tab) then
        local before = util.now_ms()
        sidebar.attach(info.tab)
        local took = util.now_ms() - before
        n, worst = n + 1, math.max(worst, took)
      end
    end
    wezterm.log_info(string.format("e2e: attach n %d worst %d", n, worst))
  end,
  grow = function(window)
    local dims = window:get_dimensions()
    window:set_inner_size(dims.pixel_width + 300, dims.pixel_height)
  end,
  rail_mode = function()
    require("vtabs.config").get().collapsed = "rail"
  end,
  hidden_mode = function()
    require("vtabs.config").get().collapsed = "hidden"
  end,
  probe_desired = function(window)
    local geometry = require "vtabs.geometry"
    local state = require "vtabs.state"
    wezterm.log_info(
      "e2e: desired width "
        .. tostring(geometry.desired(window:window_id()))
        .. " collapsed "
        .. tostring(state.is_collapsed(window:window_id()))
    )
  end,
  -- A tab overlay (the tab menu) replaces the tab's panes, so this reports the overlay's pane id.
  probe_active = function(window)
    local pane = window:active_pane()
    wezterm.log_info("e2e: active pane " .. tostring(pane and pane:pane_id()))
  end,
}

-- Test-only hook: a pane printing SetUserVar=vtabs_test=<base64 name> triggers a probe.
wezterm.on("user-var-changed", function(window, _, name, value)
  local probe = name == "vtabs_test" and probes[value]
  if not probe then
    return
  end
  local ok, err = pcall(probe, window)
  if not ok then
    wezterm.log_error("e2e: probe " .. value .. " failed: " .. tostring(err))
  end
end)

return config

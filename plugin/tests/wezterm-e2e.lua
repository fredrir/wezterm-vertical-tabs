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
})

local probes = {
  toggle = function(window)
    vtabs.toggle_sidebar(window)
  end,
  grow = function(window)
    local dims = window:get_dimensions()
    window:set_inner_size(dims.pixel_width + 300, dims.pixel_height)
  end,
  -- An InputSelector replaces the tab's panes, so the active pane becomes a TermWiz overlay pane.
  probe_overlay = function(window)
    local pane = window:active_pane()
    local domain = pane and pane:get_domain_name() or "none"
    wezterm.log_info("e2e: active pane domain " .. tostring(domain))
  end,
  probe_cols = function(window)
    wezterm.log_info("e2e: sidebar cols now " .. sidebar_cols(window))
  end,
}

-- Test-only hook: a pane printing SetUserVar=vtabs_test=<base64 name> triggers a probe.
wezterm.on("user-var-changed", function(window, _, name, value)
  local probe = name == "vtabs_test" and probes[value]
  if not probe then
    return
  end
  local ok, err = xpcall(probe, debug.traceback, window)
  if not ok then
    wezterm.log_error("e2e: probe " .. value .. " failed: " .. tostring(err))
  end
end)

return config

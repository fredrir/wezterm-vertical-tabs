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
vtabs.apply_to_config(config, {
  poll_ms = 200,
  debug = true,
  confirm_close = false,
  domain = os.getenv "VTABS_E2E_DOMAIN" or "CurrentPaneDomain",
  backend = { path = os.getenv "VTABS_BIN" },
  icons = false,
})

-- Test-only hook: a pane printing SetUserVar=vtabs_test=<base64 name> triggers a plugin action.
wezterm.on("user-var-changed", function(window, _, name, value)
  if name == "vtabs_test" and value == "toggle" then
    vtabs.toggle_sidebar(window)
  end
end)

return config

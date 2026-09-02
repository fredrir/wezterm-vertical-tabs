-- Standalone config for the `just dev` sandbox WezTerm.
local wezterm = require "wezterm"
local root = os.getenv "VTABS_ROOT"
package.path = root .. "/plugin/?.lua;" .. root .. "/plugin/?/init.lua;" .. package.path

local vtabs = dofile(root .. "/plugin/init.lua")
local config = wezterm.config_builder()

config.initial_cols = 120
config.initial_rows = 34
config.window_close_confirmation = "NeverPrompt"
config.exit_behavior = "Close"
config.window_decorations = "RESIZE"
config.color_scheme = "Catppuccin Mocha"

local mux_socket = os.getenv "VTABS_DEV_MUX"
if mux_socket then
  config.unix_domains = {
    {
      name = "devmux",
      socket_path = mux_socket,
      no_serve_automatically = true,
    },
  }
  config.default_domain = "devmux"
end

vtabs.apply_to_config(config, {
  poll_ms = 200,
  debug = true,
  confirm_close = false,
  backend = { path = os.getenv "VTABS_BIN", build = false, env = { VTABS_LOG = os.getenv "VTABS_LOG" } },
})

return config

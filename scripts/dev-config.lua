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

vtabs.apply_to_config(config, {
  poll_ms = 200,
  debug = true,
  confirm_close = false,
  backend = { path = os.getenv "VTABS_BIN", build = false, env = { VTABS_LOG = os.getenv "VTABS_LOG" } },
})

return config

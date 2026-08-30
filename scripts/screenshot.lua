-- Standalone config for `just screenshot`: a fixed scene for design review.
local wezterm = require "wezterm"
local root = os.getenv "VTABS_ROOT"
package.path = root .. "/plugin/?.lua;" .. root .. "/plugin/?/init.lua;" .. package.path

local vtabs = dofile(root .. "/plugin/init.lua")
local config = wezterm.config_builder()

config.default_prog = { "/bin/sh" }
config.initial_cols = 120
config.initial_rows = 34
config.window_close_confirmation = "NeverPrompt"
config.exit_behavior = "Close"
config.window_decorations = "RESIZE"
config.color_scheme = os.getenv "VTABS_SHOT_SCHEME" or "Catppuccin Mocha"
config.font = wezterm.font "JetBrainsMono Nerd Font"
config.font_size = 12
-- `window_padding` is left unset on purpose: zeroing it here would hide whatever the plugin does
-- with it, and edge-to-edge is exactly what the shots are meant to show. The `padded` variant sets
-- it, so the pair proves the plugin leaves a user's own padding alone.

-- Only the sandbox plumbing; everything a user could tune is left at its default.
local opts = { backend = { path = os.getenv "VTABS_BIN", build = false } }
local base = { poll_ms = 200, confirm_close = false, debug = true }
local VARIANTS = {
  default = base,
  zeroconfig = {},
  elevation = { theme = { elevation = 0.06 } },
  ["elevation-1"] = { theme = { elevation = 1 } },
  press = { hover = "press" },
  hidden = { collapsed = "hidden" },
  rail = { collapsed = "rail" },
  tooltip = { hover = "follow", tooltip = true, tooltip_delay_ms = 3000 },
  anim = { animations = true, animation = { expand_ms = 600, collapse_ms = 600 } },
  padded = base,
  macos = { titlebar = "integrate" },
  ["macos-rail"] = { titlebar = "integrate", collapsed = "rail" },
}
local variant = os.getenv "VTABS_SHOT_OPTS" or "default"
if variant == "padded" then
  config.window_padding = { left = 12, right = 12, top = 8, bottom = 8 }
end
for _, layer in ipairs { variant ~= "zeroconfig" and base or {}, VARIANTS[variant] or {} } do
  for k, v in pairs(layer) do
    opts[k] = v
  end
end

-- The traffic-light reserve is keyed off the target triple, so shooting it anywhere else means
-- lying to `platform` about the platform. Only the reserve is faked; nothing else is patched.
-- `integrated_title_button_style = "MacOsNative"` is rejected off macOS, so the two facts
-- `chrome_for` reads from the effective config are forced at the one place that consumes them.
if variant:find "^macos" then
  local platform = require "vtabs.platform"
  platform.is_mac = true
  local real = platform.strip_geometry
  platform.strip_geometry = function(dims, opts)
    opts = opts or {}
    opts.is_mac = true
    opts.integrated_buttons = true
    opts.native_button_style = true
    return real(dims, opts)
  end
  config.window_decorations = "INTEGRATED_BUTTONS|RESIZE"
end

vtabs.apply_to_config(config, opts)

local probes = {
  pin = function(window, pane)
    window:perform_action(vtabs.action.pin_tab, pane)
  end,
  toggle = function(window, pane)
    window:perform_action(vtabs.action.toggle_sidebar, pane)
  end,
  private = function(window, pane)
    window:perform_action(vtabs.action.private_window, pane)
  end,
  focus = function(window, pane)
    window:perform_action(vtabs.action.focus_sidebar, pane)
  end,
}

-- Test-only hook: a pane printing SetUserVar=vtabs_shot=<base64 name> runs a probe.
wezterm.on("user-var-changed", function(window, pane, name, value)
  local probe = name == "vtabs_shot" and probes[value]
  if probe then
    local ok, err = pcall(probe, window, pane)
    if not ok then
      wezterm.log_error("shot: probe " .. value .. " failed: " .. tostring(err))
    end
  end
end)

return config

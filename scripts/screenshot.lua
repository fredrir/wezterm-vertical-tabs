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
  confirm = { confirm_close = true },
  macos = { titlebar = "macos" },
  ["macos-rail"] = { titlebar = "macos", collapsed = "rail" },
  ["macos-rail-plain"] = { titlebar = "macos", collapsed = "rail", rail_titlebar = "none" },
  zen = { frame = "zen" },
  ["zen-square"] = { frame = { zen = true, radius = 0 } },
  ["zen-rail"] = { frame = "zen", collapsed = "rail" },
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
-- `titlebar = "macos"` claims the reserve for the strip; `platform.is_mac` is what the titlebar
-- band reads, and `integrated_title_button_style = "MacOsNative"` is rejected off macOS.
if variant:find "^macos" then
  require("vtabs.platform").is_mac = true
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
  settings = function(window, pane)
    window:perform_action(vtabs.action.open_settings, pane)
  end,
  -- `popover_in` is 90 ms, shorter than one `import`; stretching it keeps the blend and the
  -- zero stagger while giving the capture a window it can hit.
  slow_popover = function()
    require("vtabs.anim").PHASES.popover_in.ms = 900
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

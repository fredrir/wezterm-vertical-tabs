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
if os.getenv "VTABS_E2E_SOFTWARE" then
  config.front_end = "Software"
end

local mux_socket = os.getenv "VTABS_E2E_MUX"
if mux_socket then
  config.unix_domains = {
    {
      name = "e2emux",
      socket_path = mux_socket,
      -- A missing fixture-owned server is a test failure, never a reason to leak a daemon.
      no_serve_automatically = true,
    },
  }
  config.default_domain = "e2emux"
end

config.unix_domains = config.unix_domains or {}

local ssh_address = os.getenv "VTABS_E2E_SSH_ADDRESS"
if ssh_address then
  config.ssh_domains = {
    {
      name = "e2essh",
      remote_address = ssh_address,
      username = os.getenv "VTABS_E2E_SSH_USER" or "vtabs",
      no_agent_auth = true,
      multiplexing = "WezTerm",
      remote_wezterm_path = os.getenv "VTABS_E2E_REMOTE_WEZTERM" or "/usr/bin/wezterm",
      ssh_option = {
        identityfile = os.getenv "VTABS_E2E_SSH_IDENTITY",
        stricthostkeychecking = "no",
        userknownhostsfile = "/dev/null",
      },
    },
  }
end

local function backend_path(domain)
  if domain == "e2essh" then
    return os.getenv "VTABS_E2E_REMOTE_BIN"
  end
  return os.getenv "VTABS_BIN"
end

-- The traffic-light reserve and the rail's own strip geometry are keyed off the target triple,
-- so they can only be exercised anywhere else by lying to `platform` about the platform.
-- `titlebar = "macos"` claims the reserve for the strip; `platform.is_mac` is what the titlebar
-- band reads, and `integrated_title_button_style = "MacOsNative"` is rejected off macOS.
if os.getenv "VTABS_E2E_MACOS" then
  require("vtabs.platform").is_mac = true
  config.window_decorations = "INTEGRATED_BUTTONS|RESIZE"
end

vtabs.apply_to_config(config, {
  -- `"macos"` is the plugin's own preview seam: it claims the reserve without asking wezterm for a
  -- button style this platform does not have, which `"integrate"` warns about on every reload.
  titlebar = os.getenv "VTABS_E2E_MACOS" and "macos" or nil,
  poll_ms = 200,
  debug = true,
  confirm_close = false,
  domain = os.getenv "VTABS_E2E_DOMAIN" or "CurrentPaneDomain",
  backend = { path = backend_path, inbox = os.getenv "VTABS_E2E_INBOX" ~= "0" },
  icons = false,
  collapsed = os.getenv "VTABS_E2E_COLLAPSED" or nil,
})

local size_before_grow = {}

-- These adapters only perform user-facing actions. Python owns every scenario and assertion.
local probes = {
  toggle = function(window)
    vtabs.toggle_sidebar(window)
  end,
  hide_sidebar = function(window)
    require("vtabs.config").get().collapsed = "hidden"
    vtabs.toggle_sidebar(window)
  end,
  split_sidebar_h = function(window)
    local sidebar = require "vtabs.sidebar"
    local pane = sidebar.find(window:mux_window():active_tab())
    if pane then
      pane:activate()
      window:perform_action(wezterm.action.SplitHorizontal { domain = "CurrentPaneDomain" }, pane)
    end
  end,
  settings = function(window)
    require("vtabs.actions").open_settings(window)
  end,
  close_settings = function(window)
    require("vtabs.settings").close(window)
  end,
  grow = function(window)
    local dimensions = window:get_dimensions()
    size_before_grow[window:window_id()] = {
      pixel_width = dimensions.pixel_width,
      pixel_height = dimensions.pixel_height,
    }
    window:set_inner_size(dimensions.pixel_width + 300, dimensions.pixel_height)
  end,
  shrink = function(window)
    local dimensions = window:get_dimensions()
    local original = size_before_grow[window:window_id()]
    window:set_inner_size(
      original and original.pixel_width or dimensions.pixel_width - 300,
      original and original.pixel_height or dimensions.pixel_height
    )
  end,
}

-- Test-only hook: a pane printing SetUserVar=vtabs_test=<base64 name> triggers an adapter.
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

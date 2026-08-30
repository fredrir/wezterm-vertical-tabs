local wezterm = require "wezterm" ---@type Wezterm

local function plugin_root()
  for _, p in ipairs(wezterm.plugin.list()) do
    if p.url:find("wez-vertical-tabs", 1, true) then
      return p.plugin_dir .. "/plugin"
    end
  end
  local found = package.searchpath("vtabs.config", package.path)
  if found then
    return found:match "^(.*)[/\\]vtabs[/\\]config%.lua$"
  end
  return nil
end

local root = plugin_root()
if root then
  local search = root .. "/?.lua;" .. root .. "/?/init.lua;"
  if not package.path:find(search, 1, true) then
    package.path = search .. package.path
  end
else
  wezterm.log_warn "vtabs: plugin root not found; add plugin dir to package.path"
end

local config_mod = require "vtabs.config"
local backend = require "vtabs.backend"
local sidebar = require "vtabs.sidebar"
local view = require "vtabs.view"
local input = require "vtabs.input"
local actions = require "vtabs.actions"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local util = require "vtabs.util"

backend.root = root

local M = {}

M.action = actions.action
M.actions = actions
M.is_sidebar_pane = sidebar.is_sidebar
M.toggle = sidebar.toggle
M.sync = view.sync

function M.is_private_window(window)
  return state.is_private(window:window_id())
end

local function alive(window)
  return type(window) ~= "userdata" or pcall(function()
    return window:mux_window()
  end)
end

local function guarded(name, fn)
  return function(window, ...)
    if not alive(window) then
      return
    end
    local ok, err = pcall(fn, window, ...)
    if not ok and not tostring(err):find("not found in mux", 1, true) then
      util.warn("%s: %s", name, tostring(err))
    end
  end
end

local registered = false

local function register_events()
  if registered then
    return
  end
  registered = true

  local last_poll = {}
  wezterm.on(
    "update-status",
    guarded("update-status", function(window)
      local wid = window:window_id()
      local now = util.now_ms()
      if last_poll[wid] and now - last_poll[wid] < 100 then
        return
      end
      last_poll[wid] = now
      sidebar.ensure(window)
      input.tick(window)
      view.sync(window)
    end)
  )

  wezterm.on(
    "user-var-changed",
    guarded("user-var-changed", function(window, pane, name, value)
      input.handle(window, pane, name, value)
    end)
  )

  wezterm.on(
    "window-resized",
    guarded("window-resized", function(window)
      view.sync(window)
    end)
  )

  wezterm.on(
    "window-config-reloaded",
    guarded("window-config-reloaded", function(window)
      view.invalidate_theme(window:window_id())
      view.sync(window, { force = true })
    end)
  )

  wezterm.on(
    "window-focus-changed",
    guarded("window-focus-changed", function(window)
      view.sync(window)
    end)
  )

  wezterm.on(
    "gui-attached",
    guarded("gui-attached", function()
      for _, mux_win in ipairs(wezterm.mux.all_windows()) do
        local gui = mux_win:gui_window()
        if gui then
          sidebar.ensure(gui)
          view.sync(gui, { force = true })
        end
      end
    end)
  )
end

---@param config Config
---@param opts table|nil
function M.apply_to_config(config, opts)
  local cfg = config_mod.setup(opts)
  if cfg.hide_native_tab_bar then
    config.enable_tab_bar = false
  end
  config.status_update_interval = math.min(config.status_update_interval or 1000, cfg.poll_ms)
  if cfg.skip_close_confirmation then
    config.skip_close_confirmation_for_processes_named = config.skip_close_confirmation_for_processes_named
      or { "bash", "sh", "zsh", "fish", "tmux", "nu", "cmd.exe", "pwsh.exe", "powershell.exe" }
    table.insert(config.skip_close_confirmation_for_processes_named, "wez-vtabs")
  end
  keys.apply(config, cfg)
  register_events()
  return config
end

return M

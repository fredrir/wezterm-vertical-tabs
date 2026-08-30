local wezterm = require "wezterm" ---@type Wezterm

local function usable(root)
  return package.searchpath("vtabs.config", root .. "/?.lua;" .. root .. "/?/init.lua") ~= nil
end

---Prefers the checkout the modules already resolve from, so a stale plugin clone never shadows it.
local function plugin_root()
  local found = package.searchpath("vtabs.config", package.path)
  if found then
    return found:match "^(.*)[/\\]vtabs[/\\]config%.lua$"
  end
  for _, p in ipairs(wezterm.plugin.list()) do
    local candidate = p.plugin_dir .. "/plugin"
    if p.url:find("vertical-tabs", 1, true) and usable(candidate) then
      return candidate
    end
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
local geometry = require "vtabs.geometry"
local input = require "vtabs.input"
local actions = require "vtabs.actions"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local util = require "vtabs.util"

backend.root = root

local M = {}

M.version = require "vtabs.version"
M.action = actions.action
M.toggle_sidebar = actions.toggle_sidebar
M.show_sidebar = actions.show_sidebar
M.sync = view.sync
M.invalidate_theme = view.invalidate_theme
M.is_sidebar_pane = sidebar.is_sidebar

function M.is_private_window(window)
  return state.is_private(window:window_id())
end

local function alive(window)
  return pcall(function()
    return window:mux_window()
  end)
end

-- WezTerm reports closed windows with this text; there is no structured error to match.
local function window_gone(err)
  return tostring(err):find("not found in mux", 1, true) ~= nil
end

local function guarded(name, fn)
  return function(window, ...)
    if not alive(window) then
      return
    end
    local ok, err = pcall(fn, window, ...)
    if not ok and not window_gone(err) then
      util.warn("%s: %s", name, tostring(err))
    end
  end
end

local registered = false

local function register_events(cfg)
  if registered then
    return
  end
  registered = true

  local last_poll = {}
  local tracked = 0
  local min_gap = math.max(50, math.floor(cfg.poll_ms / 4))

  ---Windows leave without an event; per-window tables are dropped once one goes missing from the mux.
  local function prune_windows()
    local live = {}
    for _, mux_win in ipairs(wezterm.mux.all_windows()) do
      live[mux_win:window_id()] = true
    end
    tracked = 0
    for wid in pairs(last_poll) do
      if live[wid] then
        tracked = tracked + 1
      else
        last_poll[wid] = nil
        geometry.forget_window(wid)
        view.invalidate_theme(wid)
        state.forget_window(wid)
      end
    end
  end

  wezterm.on(
    "update-status",
    guarded("update-status", function(window)
      local wid = window:window_id()
      local now = util.now_ms()
      if last_poll[wid] and now - last_poll[wid] < min_gap then
        return
      end
      if last_poll[wid] == nil then
        tracked = tracked + 1
      end
      last_poll[wid] = now
      if tracked > #wezterm.mux.all_windows() then
        prune_windows()
      end
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
      geometry.correct(window)
      view.sync(window)
    end)
  )

  wezterm.on(
    "window-config-reloaded",
    guarded("window-config-reloaded", function(window)
      local wid = window:window_id()
      view.invalidate_theme(wid)
      geometry.reset(wid)
      geometry.correct(window)
      view.sync(window, { force = true })
    end)
  )

  wezterm.on(
    "window-focus-changed",
    guarded("window-focus-changed", function(window)
      view.sync(window)
    end)
  )

  wezterm.on("gui-attached", function()
    for _, mux_win in ipairs(wezterm.mux.all_windows()) do
      local gui = mux_win:gui_window()
      if gui then
        local ok, err = pcall(function()
          sidebar.ensure(gui)
          view.sync(gui, { force = true })
        end)
        if not ok and not window_gone(err) then
          util.warn("gui-attached: %s", tostring(err))
        end
      end
    end
  end)
end

local MODULES = {
  "actions",
  "ansi",
  "backend",
  "config",
  "geometry",
  "hit",
  "icons",
  "input",
  "keys",
  "menu",
  "model",
  "platform",
  "render",
  "sidebar",
  "state",
  "theme",
  "util",
  "version",
  "view",
}

---Edits to the plugin reload the config like edits to the user's own files do.
local function watch_plugin_files()
  if not root or not wezterm.add_to_config_reload_watch_list then
    return
  end
  wezterm.add_to_config_reload_watch_list(root .. "/init.lua")
  for _, name in ipairs(MODULES) do
    wezterm.add_to_config_reload_watch_list(root .. "/vtabs/" .. name .. ".lua")
  end
end

---@param config Config
---@param opts table|nil
function M.apply_to_config(config, opts)
  local cfg = config_mod.setup(opts)
  if cfg.hide_native_tab_bar then
    config.enable_tab_bar = false
  end
  config.status_update_interval = math.min(config.status_update_interval or 1000, cfg.poll_ms)
  if cfg.hover == "follow" and config.pane_focus_follows_mouse == nil then
    config.pane_focus_follows_mouse = true
  end
  if cfg.skip_close_confirmation then
    config.skip_close_confirmation_for_processes_named = config.skip_close_confirmation_for_processes_named
      or { "bash", "sh", "zsh", "fish", "tmux", "nu", "cmd.exe", "pwsh.exe", "powershell.exe" }
    if not util.contains(config.skip_close_confirmation_for_processes_named, "wez-vtabs") then
      table.insert(config.skip_close_confirmation_for_processes_named, "wez-vtabs")
    end
  end
  backend.register_local_domains(config)
  keys.apply(config, cfg)
  watch_plugin_files()
  register_events(cfg)
  return config
end

return M

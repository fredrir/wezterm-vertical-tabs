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
local settings = require "vtabs.settings"
local sidebar = require "vtabs.sidebar"
local view = require "vtabs.view"
local geometry = require "vtabs.geometry"
local platform = require "vtabs.platform"
local theme = require "vtabs.theme"
local input = require "vtabs.input"
local actions = require "vtabs.actions"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local util = require "vtabs.util"

backend.root = root

local M = {}

local function alive(window)
  return pcall(function()
    return window:mux_window()
  end)
end

-- WezTerm reports a window, tab or pane that left mid-call with these texts; nothing structured.
local GONE = { "not found in mux", "is not valid" }

local function window_gone(err)
  local text = tostring(err)
  for _, needle in ipairs(GONE) do
    if text:find(needle, 1, true) then
      return true
    end
  end
  return false
end

local function reported(name, fn)
  return function(window, ...)
    local ok, err = pcall(fn, window, ...)
    if not ok and not window_gone(err) then
      util.warn("%s: %s", name, tostring(err))
    end
  end
end

local function guarded(name, fn)
  local report = reported(name, fn)
  return function(window, ...)
    if alive(window) then
      report(window, ...)
    end
  end
end

M.version = require "vtabs.version"
M.action = actions.action
M.toggle_sidebar = reported("toggle_sidebar", actions.toggle_sidebar)
M.show_sidebar = reported("show_sidebar", actions.show_sidebar)
M.sync = reported("sync", view.sync)
M.invalidate_theme = view.invalidate_theme
M.is_sidebar_pane = sidebar.is_backend
M.window_title = view.window_title

function M.is_private_window(window)
  local wid = util.try(function()
    return window:window_id()
  end)
  return wid ~= nil and state.is_private(wid)
end

local registered = false

local function register_events(cfg)
  if registered then
    return
  end
  registered = true

  local last_poll = {}
  local min_gap = math.max(50, math.floor(cfg.poll_ms / 4))
  table.insert(state.forget_hooks, function(wid)
    last_poll[wid] = nil
  end)

  wezterm.on(
    "update-status",
    guarded("update-status", function(window)
      local wid = window:window_id()
      local now = util.now_ms()
      if last_poll[wid] and now - last_poll[wid] < min_gap then
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

  -- Fires once per frame of a window drag; the poll picks up the last one, so only the first frame
  -- of a burst pays for a correction and a repaint.
  wezterm.on(
    "window-resized",
    guarded("window-resized", function(window)
      if geometry.on_resize(window:window_id()) then
        geometry.correct(window)
        view.sync(window)
      end
    end)
  )

  wezterm.on(
    "window-config-reloaded",
    guarded("window-config-reloaded", function(window)
      local wid = window:window_id()
      view.invalidate_theme(wid)
      -- Our own `set_config_overrides` fires this; re-entering correction mid-toggle would fight it.
      if not state.applying_recently(wid) then
        geometry.reset(wid)
      end
      view.sync(window, { force = true })
    end)
  )

  wezterm.on(
    "window-focus-changed",
    guarded("window-focus-changed", function(window)
      view.sync(window)
    end)
  )

  -- WezTerm calls only the first handler registered for this event (config/src/lua.rs:795-814).
  if cfg.window_title then
    wezterm.on("format-window-title", function(tab, pane, tabs, panes)
      return view.window_title(tab, pane, tabs, panes)
    end)
  end

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
  "anim",
  "ansi",
  "backend",
  "config",
  "frame",
  "geometry",
  "glyphs",
  "hit",
  "icons",
  "input",
  "keys",
  "layout",
  "model",
  "page",
  "platform",
  "popover",
  "render",
  "schema",
  "settings",
  "sidebar",
  "state",
  "theme",
  "util",
  "version",
  "view",
}

---Every `vtabs/*.lua`, read from disk when wezterm can list it so the static list cannot drift.
function M.module_names()
  local found = util.try(function()
    return wezterm.read_dir(root .. "/vtabs")
  end)
  if type(found) ~= "table" or #found == 0 then
    return MODULES
  end
  local names = {}
  for _, path in ipairs(found) do
    local name = tostring(path):match "([^/\\]+)%.lua$"
    if name then
      names[#names + 1] = name
    end
  end
  table.sort(names)
  return #names > 0 and names or MODULES
end

---Edits to the plugin reload the config like edits to the user's own files do.
local function watch_plugin_files()
  if not root or not wezterm.add_to_config_reload_watch_list then
    return
  end
  wezterm.add_to_config_reload_watch_list(root .. "/init.lua")
  for _, name in ipairs(M.module_names()) do
    wezterm.add_to_config_reload_watch_list(root .. "/vtabs/" .. name .. ".lua")
  end
end

---`theme.split` recolours the pane divider for every split in the window, not just ours.
local function apply_split(config, cfg)
  local want = cfg.theme.split
  if want == nil or want == "auto" then
    return
  end
  config.colors = config.colors or {}
  if config.colors.split ~= nil then
    return
  end
  if want == "hidden" then
    config.colors.split = config.colors.background or theme.page(config)
  else
    config.colors.split = want
  end
end

-- WezTerm's own default; the far side keeps it so only the sidebar's edges change.
local WEZTERM_SIDE_PADDING = "1cell"
local WEZTERM_TOP_PADDING = "0.5cell"

---`window_padding` is window-global, so wezterm's left band frames the sidebar in the terminal
---colour. Zeroing the sides the sidebar touches is the only way its page reaches the window edge;
---the air comes back as `padding.left`, painted in the sidebar's own colour.
local function apply_padding(config, cfg)
  if config.window_padding ~= nil then
    return
  end
  -- The frame margin is what supplies the air on every side while zen is on, so it supersedes the
  -- asymmetric edge-to-edge padding rather than fighting it.
  local frame = require "vtabs.frame"
  if frame.enabled(cfg) then
    local m = frame.margin(cfg)
    config.window_padding = { left = m, right = m, top = m, bottom = m }
    return
  end
  if not cfg.edge_to_edge then
    return
  end
  local outer = cfg.position == "left" and "right" or "left"
  -- window_padding is one rectangle for the whole window, so the sidebar can only reach the top and
  -- bottom edges by taking the content pane's half-cell with it; "sides" declines that trade.
  local band = cfg.edge_to_edge == "sides" and WEZTERM_TOP_PADDING or 0
  config.window_padding = { left = 0, right = 0, top = band, bottom = band }
  config.window_padding[outer] = WEZTERM_SIDE_PADDING
end

---macOS hides close/minimise/zoom unless the decorations ask for them; `RESIZE` alone also pins
---the window in place. Only a left-hand sidebar reserves cells for them, so only it opts in.
local function apply_decorations(config, cfg)
  if not platform.is_mac then
    return
  end
  local decorations = config.window_decorations
  if decorations == nil then
    if cfg.position == "left" and cfg.titlebar ~= "plain" then
      config.window_decorations = "INTEGRATED_BUTTONS|RESIZE"
    end
  elseif decorations == "RESIZE" then
    util.warn_once("decorations", 'window_decorations = "RESIZE" hides the macOS window buttons')
  end
end

---@param config Config
---@param opts table|nil
function M.apply_to_config(config, opts)
  -- read before this function writes any of its own: a non-nil entry means the host owns that key
  config_mod.host_config = {
    window_decorations = config.window_decorations,
    pane_focus_follows_mouse = config.pane_focus_follows_mouse,
    inactive_pane_hsb = config.inactive_pane_hsb,
    colors_split = config.colors and config.colors.split or nil,
    window_padding = config.window_padding,
  }
  -- defaults <- settings.json <- opts, so wezterm.lua always outranks the file. The path comes
  -- straight off `opts` rather than a first `setup`, which would warn about a typo twice.
  local stored = util.try(function()
    return settings.load { settings = (opts or {}).settings }
  end)
  local cfg = config_mod.setup(opts, stored)
  if cfg.hide_native_tab_bar then
    config.enable_tab_bar = false
  end
  config.status_update_interval = math.min(config.status_update_interval or 1000, cfg.poll_ms)
  if cfg.hover == "follow" and config.pane_focus_follows_mouse == nil then
    config.pane_focus_follows_mouse = true
  end
  -- A background layer makes every pane transparent, and `inactive_pane_hsb` has nothing left to
  -- dim, so under the frame it is noise in the config rather than a setting.
  local zen = require("vtabs.frame").enabled(cfg)
  -- The sidebar is chrome, not a pane to focus; wezterm would otherwise dim whichever one is idle.
  if not zen and cfg.dim_inactive_panes == false and config.inactive_pane_hsb == nil then
    config.inactive_pane_hsb = { brightness = 1.0, saturation = 1.0, hue = 1.0 }
  end
  apply_split(config, cfg)
  apply_padding(config, cfg)
  apply_decorations(config, cfg)
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

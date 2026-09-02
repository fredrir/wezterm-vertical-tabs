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
local host_config = require "vtabs.host_config"
local platform = require "vtabs.platform"
local input = require "vtabs.input"
local actions = require "vtabs.actions"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local mux = require "vtabs.mux"
local store = require "vtabs.store"
local util = require "vtabs.util"

backend.root = root

local M = {}

local function alive(window)
  return pcall(function()
    return window:mux_window()
  end)
end

local window_gone = util.window_gone

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
  local wid = mux.window_id(window)
  return wid ~= nil and state.is_private(wid)
end

local registered = false

local function register_events(cfg)
  if registered then
    return
  end
  registered = true

  local events = store.scope "events"
  local last_poll, shown_tab, unfocused = events.window(), events.window(), events.window()
  local beating = events.window()
  local min_gap = math.max(50, math.floor(cfg.poll_ms / 4))
  -- A window nobody is typing in still shows its sidebar, so it keeps polling, at half the rate.
  local idle_gap = cfg.poll_ms * 2

  local function poll(window)
    local wid = window:window_id()
    local now = util.now_ms()
    -- WezTerm fires `update-status` on a tab switch too, native key bindings included. That poll
    -- is never rate-gated: the new tab's sidebar attaches, corrects and highlights with the switch.
    local tab = util.active_tab(window)
    local tab_id = tab and tab:tab_id() or nil
    local switched = shown_tab[wid] ~= tab_id
    local gap = unfocused[wid] and idle_gap or min_gap
    if not switched and last_poll[wid] and now - last_poll[wid] < gap then
      return
    end
    -- A switch to a tab this window already had is what a held key repeats; a tab just spawned
    -- is new here and is served at once however fast it arrived.
    if switched and shown_tab[wid] ~= nil and (store.known_tabs[wid] or {})[tab_id] then
      geometry.on_switch(wid)
    end
    shown_tab[wid] = tab_id
    last_poll[wid] = now
    -- A tab the hand stopped on is owed its sidebar even when this pass finds the window busy.
    if tab_id and not geometry.switching(wid) then
      store.visited[tab_id] = true
    end
    sidebar.ensure(window)
    input.tick(window)
    view.sync(window)
  end

  -- WezTerm re-arms `update-status` only after a title update, so a window with nothing printing
  -- in it gets no polls at all. The plugin's own clock keeps every window at `poll_ms`, gated the
  -- same way, until the window is gone.
  local function heartbeat(window, wid)
    if beating[wid] then
      return
    end
    beating[wid] = true
    local beat
    beat = function()
      if not alive(window) then
        beating[wid] = nil
        return
      end
      reported("heartbeat", poll)(window)
      wezterm.time.call_after(cfg.poll_ms / 1000, beat)
    end
    wezterm.time.call_after(cfg.poll_ms / 1000, beat)
  end

  wezterm.on(
    "update-status",
    guarded("update-status", function(window)
      poll(window)
      heartbeat(window, window:window_id())
    end)
  )

  wezterm.on(
    "user-var-changed",
    guarded("user-var-changed", function(window, pane, name, value)
      input.handle(window, pane, name, value)
    end)
  )

  -- Fires once per frame of a window drag or an animated fill; nothing adjusts until the frames stop.
  wezterm.on(
    "window-resized",
    guarded("window-resized", function(window)
      view.on_resize(window)
    end)
  )

  wezterm.on(
    "window-config-reloaded",
    guarded("window-config-reloaded", function(window)
      local wid = window:window_id()
      local applying = state.applying_recently(wid)
      -- Installing the optional frame changes effective config but not the Rust-resolved palette.
      -- Keep that answer: the backend deliberately emits it once per semantic generation.
      view.invalidate_theme(wid, applying)
      -- Our own `set_config_overrides` fires this; re-entering correction mid-toggle would fight it.
      if not applying then
        geometry.reset(wid)
      end
      view.sync(window)
    end)
  )

  wezterm.on(
    "window-focus-changed",
    guarded("window-focus-changed", function(window)
      unfocused[window:window_id()] = mux.call(window, "is_focused") == false or nil
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
      -- A window the mux holds without a GUI (a standalone server's, or one still opening) throws
      -- here; it must not take the windows after it in the list down with it.
      local gui = mux.call(mux_win, "gui_window")
      if gui then
        local ok, err = pcall(function()
          sidebar.ensure(gui)
          view.sync(gui)
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
  "backend",
  "boot_normalize",
  "config",
  "frame",
  "geometry",
  "host_config",
  "input",
  "keys",
  "model",
  "mux",
  "page",
  "platform",
  "popover",
  "schema",
  "settings",
  "settings_model",
  "sidebar",
  "sidebar_attach",
  "sidebar_identity",
  "sidebar_rescue",
  "snapshot",
  "spaces",
  "state",
  "store",
  "theme_bridge",
  "util",
  "version",
  "view",
  "wire",
}

---Every top-level `vtabs/*.lua`, read from disk when wezterm can list it so the static list cannot
---drift. Generated mirrors live one directory deeper and are added explicitly below.
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
  for _, name in ipairs { "protocol", "schema" } do
    wezterm.add_to_config_reload_watch_list(root .. "/vtabs/gen/" .. name .. ".lua")
  end
end

---macOS hides close/minimise/zoom unless the decorations ask for them. Only a left-hand sidebar
---reserves cells for them, so only it opts in; a bare `RESIZE` is read as the wish for no title
---bar, not for no buttons, and gets them back the same way. `titlebar = "plain"` declines both.
local function apply_decorations(config, cfg)
  if not platform.is_mac or cfg.position ~= "left" or cfg.titlebar == "plain" then
    return
  end
  local decorations = config.window_decorations
  if decorations ~= nil and decorations ~= "RESIZE" then
    return
  end
  config.window_decorations = "INTEGRATED_BUTTONS|RESIZE"
  -- the key is the plugin's from here, so the page offers `titlebar` instead of locking it
  config_mod.host_config.window_decorations = nil
end

---@param config Config
---@param opts table|nil
function M.apply_to_config(config, opts)
  -- read before this function writes any of its own: a non-nil entry means the host owns that key
  config_mod.host_config = host_config.capture(config)
  -- defaults <- settings.json <- opts, so wezterm.lua always outranks the file. The path comes
  -- straight off `opts` rather than a first `setup`, which would warn about a typo twice.
  local cfg = util.try(require("vtabs.boot_normalize").try, opts)
  if not cfg then
    local stored = util.try(function()
      return settings.load { settings = (opts or {}).settings }
    end)
    cfg = config_mod.setup(opts, stored)
  end
  if cfg.hide_native_tab_bar then
    config.enable_tab_bar = false
  end
  config.status_update_interval = math.floor(math.min(config.status_update_interval or 1000, cfg.poll_ms))
  host_config.apply_boot(config, cfg, config_mod.host_config)
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

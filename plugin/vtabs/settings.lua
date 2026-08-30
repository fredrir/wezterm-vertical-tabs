local wezterm = require "wezterm" ---@type Wezterm
local backend = require "vtabs.backend"
local config = require "vtabs.config"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local util = require "vtabs.util"

local schema = require "vtabs.schema"

local M = {}

local act = wezterm.action
local VERSION = 1

local function opt(cfg, key, fallback)
  local box = cfg and cfg.settings
  if type(box) ~= "table" or box[key] == nil then
    return fallback
  end
  return box[key]
end

local function config_dir()
  local base = os.getenv "XDG_CONFIG_HOME"
  if not base or base == "" or base:sub(1, 1) ~= "/" or base:find("..", 1, true) then
    base = (wezterm.home_dir or os.getenv "HOME" or ".") .. "/.config"
  end
  return base .. "/wez-vtabs"
end

M.dir = config_dir()

---Where the file lives. A relative or traversing `settings.path` is refused rather than resolved
---against whatever the plugin's working directory happens to be.
function M.path(cfg)
  local given = opt(cfg, "path", nil)
  if type(given) == "string" and given ~= "" then
    if given:sub(1, 1) ~= "/" or given:find("..", 1, true) then
      util.warn_once("settings-path", "settings.path must be an absolute path, using the default")
    else
      return given
    end
  end
  return M.dir .. "/settings.json"
end

---True when the page is allowed to write at all.
function M.persists(cfg)
  return cfg ~= nil and cfg.settings ~= false and opt(cfg, "persist", true) ~= false
end

---Keeps the keys the schema knows, plus anything under an `open` container, whose children it
---deliberately does not enumerate. Everything else is dropped, with one warning for the lot.
local function keep_known(stored, prefix, out, dropped)
  out = out or {}
  for key, value in pairs(stored) do
    if type(key) == "string" then
      local path = prefix and (prefix .. "." .. key) or key
      local option = schema.by_key[path]
      if option and option.open then
        out[key] = value
      elseif option then
        if type(value) == "table" then
          out[key] = keep_known(value, path, {}, dropped)
        else
          out[key] = value
        end
      elseif schema.is_open(path) then
        out[key] = value
      else
        dropped[#dropped + 1] = path
      end
    end
  end
  return out
end

---Reads the file into a table of stored options, or nil. Never throws: a settings file the user
---broke by hand must not stop the sidebar from painting.
function M.load(cfg)
  local path = M.path(cfg)
  if util.is_symlink(path) then
    util.warn_once("settings-symlink", "settings file is a symlink, ignored")
    return nil
  end
  local f = io.open(path, "r")
  if not f then
    return nil
  end
  local body = f:read "a"
  f:close()
  local parsed = util.try(wezterm.json_parse, body)
  if type(parsed) ~= "table" then
    util.warn_once("settings-corrupt", "settings file unreadable, starting from the defaults")
    return nil
  end
  if parsed.version ~= VERSION then
    util.warn_once("settings-version", "settings file version %s ignored", tostring(parsed.version))
    return nil
  end
  if type(parsed.options) ~= "table" then
    return nil
  end
  local dropped = {}
  local kept = keep_known(parsed.options, nil, {}, dropped)
  if #dropped > 0 then
    table.sort(dropped)
    util.warn_once("settings-unknown", "settings file: dropped unknown %s", table.concat(dropped, ", "))
  end
  return kept
end

local function same(a, b)
  if type(a) ~= "table" or type(b) ~= "table" then
    return a == b
  end
  for k, v in pairs(a) do
    if not same(v, b[k]) then
      return false
    end
  end
  for k in pairs(b) do
    if a[k] == nil then
      return false
    end
  end
  return true
end

---Only what differs from the schema default, so a future change to a default reaches this user.
function M.changed(cfg)
  local out = {}
  for _, option in ipairs(schema.options) do
    local value = schema.get(cfg, option.key)
    local default = schema.get(config.defaults, option.key)
    if value ~= nil and type(value) ~= "function" and not same(value, default) then
      schema.set(out, option.key, value)
    end
  end
  return out
end

---Writes the non-default keys, versioned, atomically, 0600. Called by an edit, never by a timer.
function M.save(cfg)
  cfg = cfg or config.get()
  if not M.persists(cfg) then
    return false
  end
  local body = util.try(wezterm.json_encode, { version = VERSION, options = M.changed(cfg) })
  if type(body) ~= "string" then
    return false
  end
  return util.write_private(M.path(cfg), body, M.dir, "settings")
end

---The settings tab of this mux window and the pane running the page, or nil.
---One page per window: `open` looks here before it spawns anything.
function M.find(mux_window)
  local infos = util.try(function()
    return mux_window:tabs_with_info()
  end) or {}
  for _, info in ipairs(infos) do
    local panes = util.try(function()
      return info.tab:panes()
    end) or {}
    for _, pane in ipairs(panes) do
      if sidebar.is_settings(pane) then
        return info.tab, pane
      end
    end
  end
  return nil
end

local function active_content_pane(gui_window)
  local tab = util.try(function()
    return gui_window:mux_window():active_tab()
  end)
  return tab and sidebar.content_pane(tab) or nil
end

---Registers the page with the bridge exactly as a sidebar is registered: a token this process
---minted, sent over the same channel, and trusted only once the backend echoes it back.
local function register(pane)
  state.set_token(pane:pane_id(), util.random_token())
  sidebar.auth(pane)
end

---Opens the settings page, or activates the one this window already has.
---The single owner: the strip button, the key binding and the popover item all come through here.
function M.open(gui_window)
  local mux_win = gui_window:mux_window()
  local existing, pane = M.find(mux_win)
  if existing then
    existing:activate()
    if pane and not sidebar.is_ready(pane) then
      register(pane)
    end
    return existing
  end
  local cfg = config.get()
  local base = active_content_pane(gui_window)
  local pane_domain = base and util.try(function()
    return base:get_domain_name()
  end) or "local"
  local args = backend.spawn_args(cfg, pane_domain, nil, "settings")
  if not args then
    util.warn_once("settings-backend", "no backend for domain %s; settings unavailable", tostring(pane_domain))
    return nil
  end
  local ok, tab, opened = pcall(function()
    return mux_win:spawn_tab {
      args = args,
      domain = { DomainName = pane_domain },
      set_environment_variables = backend.env(cfg, pane_domain, nil, nil),
    }
  end)
  if not ok or not tab then
    util.warn("settings spawn failed: %s", tostring(tab):match "^[^\n]*")
    return nil
  end
  register(opened)
  if not state.is_collapsed(gui_window:window_id()) then
    sidebar.attach(tab)
  end
  opened:activate()
  return tab
end

---Closes the settings tab by id. `CloseCurrentTab` ignores the pane it is handed, so the tab has to
---be the active one first or the wrong tab dies.
function M.close(gui_window)
  local tab, pane = M.find(gui_window:mux_window())
  if not tab then
    return false
  end
  local tab_id = tab:tab_id()
  tab:activate()
  local active = util.try(function()
    return gui_window:mux_window():active_tab()
  end)
  if not active or active:tab_id() ~= tab_id then
    util.warn_once("settings-close", "settings tab %s would not activate; not closing", tostring(tab_id))
    return false
  end
  -- every edit commits as it is made, so there is nothing to lose to a confirmation prompt
  util.try(function()
    gui_window:perform_action(act.CloseCurrentTab { confirm = false }, pane)
  end)
  return true
end

---Keys the page answers itself; everything else is the backend's business.
function M.key(gui_window, ev)
  if ev.key == "escape" or (ev.key == "q" and (ev.mods == nil or ev.mods == "" or ev.mods == "NONE")) then
    M.close(gui_window)
    return true
  end
  return false
end

return M

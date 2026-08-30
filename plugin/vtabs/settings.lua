local wezterm = require "wezterm" ---@type Wezterm
local backend = require "vtabs.backend"
local config = require "vtabs.config"
local mux = require "vtabs.mux"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local util = require "vtabs.util"

local page = require "vtabs.page"
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

---An absolute path with no `..` segment. `find("..", 1, true)` rejected `~/my..notes/` too, which
---is a perfectly ordinary directory; only a whole segment is a traversal.
local function safe_path(path)
  if type(path) ~= "string" or path == "" or path:sub(1, 1) ~= "/" then
    return false
  end
  for segment in path:gmatch "[^/]+" do
    if segment == ".." then
      return false
    end
  end
  return true
end

M.safe_path = safe_path

local function config_dir()
  local base = os.getenv "XDG_CONFIG_HOME"
  if not safe_path(base) then
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
    if safe_path(given) then
      return given
    end
    util.warn_once("settings-path", "settings.path must be an absolute path, using the default")
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

---Only what differs from the schema default, so a future change to a default reaches this user.
---The page already walks the descriptors for exactly this set; one walk, one answer.
function M.changed(cfg)
  return page.changed(cfg)
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
  for _, info in ipairs(mux.tabs_with_info(mux_window) or {}) do
    for _, pane in ipairs(mux.panes(info.tab) or {}) do
      if sidebar.is_settings(pane) then
        return info.tab, pane
      end
    end
  end
  return nil
end

local function active_content_pane(gui_window)
  local tab = mux.active_tab(mux.call(gui_window, "mux_window"))
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
  local pane_domain = base and mux.domain(base) or "local"
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
  local active = mux.active_tab(mux.call(gui_window, "mux_window"))
  if not active or active:tab_id() ~= tab_id then
    util.warn_once("settings-close", "settings tab %s would not activate; not closing", tostring(tab_id))
    return false
  end
  -- every edit commits as it is made, so there is nothing to lose to a confirmation prompt
  mux.call(gui_window, "perform_action", act.CloseCurrentTab { confirm = false }, pane)
  return true
end

---Per-window page state: which group, which field, the scroll, the filter, what is being edited.
---`page` is pure - state in, frame out - so the registry lives here, with the rest of the host.
local pages = {}

function M.page_state(wid)
  pages[wid] = pages[wid] or { group = 1, focus = 1, scroll = 0, filter = "" }
  return pages[wid]
end

table.insert(state.forget_hooks, function(wid)
  pages[wid] = nil
end)

---True when the event carries no modifier. The wire sends `mods` as a JSON array, so comparing it
---as a string silently treats every chord as bare.
local function bare(ev)
  local mods = ev.mods
  if mods == nil then
    return true
  end
  if type(mods) == "table" then
    return #mods == 0
  end
  return mods == "" or mods == "NONE"
end

---Keys the page answers itself; everything else is the backend's business.
function M.key(gui_window, ev)
  if ev.key == "escape" or (ev.key == "q" and bare(ev)) then
    M.close(gui_window)
    return true
  end
  return false
end

return M

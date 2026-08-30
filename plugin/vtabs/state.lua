local wezterm = require "wezterm" ---@type Wezterm
local platform = require "vtabs.platform"
local util = require "vtabs.util"

local M = {}

local MAX_CLOSED = 20
local VERSION = 1

local function state_dir()
  local base = os.getenv "XDG_STATE_HOME"
  if not base or base == "" or base:sub(1, 1) ~= "/" or base:find("..", 1, true) then
    base = (wezterm.home_dir or os.getenv "HOME" or ".") .. "/.local/state"
  end
  return base .. "/wez-vtabs"
end

M.file = state_dir() .. "/state.json"

---Only id-free data survives the process; pane/tab/window ids are re-used by the next mux.
local PERSISTED = { "closed", "pinned" }

local function empty()
  return {
    pinned = {},
    sidebars = {},
    tokens = {},
    private = {},
    closed = {},
    space_of = {},
    spaces = {},
    collapsed = {},
    focus = {},
  }
end

local function shell(args)
  if platform.is_windows then
    return false
  end
  return util.try(wezterm.run_child_process, args) == true
end

local function is_symlink(path)
  return shell { "test", "-L", path }
end

local function copy_closed(list)
  local out = {}
  for _, entry in ipairs(type(list) == "table" and list or {}) do
    if type(entry) == "table" and #out < MAX_CLOSED then
      out[#out + 1] = {
        cwd = type(entry.cwd) == "string" and entry.cwd or nil,
        domain = type(entry.domain) == "string" and entry.domain or nil,
        title = type(entry.title) == "string" and entry.title or nil,
      }
    end
  end
  return out
end

local function copy_pins(tbl)
  local out = {}
  for k, v in pairs(type(tbl) == "table" and tbl or {}) do
    if type(k) == "string" and v == true then
      out[k] = true
    end
  end
  return out
end

local function read_file()
  if is_symlink(M.file) then
    util.warn_once("state-symlink", "state file is a symlink, ignored")
    return nil
  end
  local f = io.open(M.file, "r")
  if not f then
    return nil
  end
  local body = f:read "a"
  f:close()
  local parsed = util.try(wezterm.json_parse, body)
  if type(parsed) ~= "table" then
    util.warn_once("state-corrupt", "state file unreadable, starting empty")
    return nil
  end
  if parsed.version ~= VERSION then
    util.warn_once("state-version", "state file version %s ignored", tostring(parsed.version))
    return nil
  end
  return parsed
end

local last_written = nil
local tmp_suffix = nil

local function write_file(tbl)
  local body = wezterm.json_encode(tbl)
  if body == last_written then
    return
  end
  tmp_suffix = tmp_suffix or util.random_token():sub(1, 8)
  local tmp = M.file .. "." .. tmp_suffix .. ".tmp"
  local f = io.open(tmp, "w")
  if not f then
    util.try(wezterm.run_child_process, { "mkdir", "-m", "700", "-p", state_dir() })
    f = io.open(tmp, "w")
  end
  if not f then
    util.warn_once("state-file", "cannot write %s", M.file)
    return
  end
  if not platform.is_windows and not shell { "chmod", "600", tmp } then
    util.warn_once("state-chmod", "cannot restrict %s to 0600", M.file)
  end
  f:write(body)
  f:close()
  if os.rename(tmp, M.file) then
    last_written = body
  else
    os.remove(tmp)
    util.warn_once("state-rename", "cannot replace %s", M.file)
  end
end

local data = empty()
local deferred_pins = nil

---Pins are keyed by tab id, so they are only meaningful while the mux that minted them lives.
local function load()
  local saved = wezterm.GLOBAL and wezterm.GLOBAL.vtabs or nil
  data = empty()
  deferred_pins = nil
  if type(saved) == "table" then
    for k, v in pairs(saved) do
      if type(v) == "table" and data[k] then
        local copy = {}
        for kk, vv in pairs(v) do
          copy[kk] = vv
        end
        data[k] = copy
      end
    end
    return
  end
  local file = read_file()
  if not file then
    return
  end
  data.closed = copy_closed(file.closed)
  deferred_pins = copy_pins(file.pinned)
end

load()

---Re-reads `wezterm.GLOBAL` and the state file; the file is only consulted in a fresh Lua VM.
function M.reload()
  last_written = nil
  load()
end

---Per-process data that must not survive config reloads.
M.session = {
  hover = {},
  drag = {},
  scroll = {},
  user_scrolled = {},
  hits = {},
  frames = {},
  dims = {},
  ready = {},
  seen = {},
  pinged = {},
  sent_at = {},
  last_click = {},
  content_pane = {},
  tab_meta = {},
  known_tabs = {},
  moving = {},
  focus_index = {},
  applying = {},
  popover = {},
  tooltip = {},
  last_active = {},
  attaching = {},
  adopted = {},
  spawned = {},
  authed_at = {},
  auth_tries = {},
  marker = {},
  pane_domain = {},
  failed_domains = {},
  spawned_domains = {},
  given_up = {},
  logged_domains = {},
}

local WINDOW_SESSION = {
  "hover",
  "drag",
  "scroll",
  "user_scrolled",
  "last_click",
  "known_tabs",
  "focus_index",
  "last_active",
  "applying",
  "popover",
  "tooltip",
}

local PANE_SESSION = {
  "hits",
  "frames",
  "dims",
  "ready",
  "seen",
  "pinged",
  "sent_at",
  "adopted",
  "spawned",
  "authed_at",
  "auth_tries",
  "marker",
}

local function save(persist)
  if wezterm.GLOBAL then
    wezterm.GLOBAL.vtabs = data
  end
  if persist then
    local subset = { version = VERSION }
    for _, key in ipairs(PERSISTED) do
      subset[key] = data[key]
    end
    write_file(subset)
  end
end

local function key(id)
  return tostring(id)
end

function M.is_pinned(tab_id)
  return data.pinned[key(tab_id)] == true
end

function M.set_pinned(tab_id, pinned)
  data.pinned[key(tab_id)] = pinned or nil
  save(true)
end

---True while pins read from the file wait for proof that the mux that minted their tab ids survived.
function M.pins_pending()
  return deferred_pins ~= nil
end

function M.restore_pins()
  if not deferred_pins then
    return
  end
  for k, v in pairs(deferred_pins) do
    data.pinned[k] = v
  end
  deferred_pins = nil
  save(true)
end

function M.discard_pins()
  deferred_pins = nil
  save(true)
end

---True just after we applied a config override, so its reload event can be told apart from a real one.
function M.applying_recently(window_id, within_ms)
  local at = M.session.applying[window_id]
  return at ~= nil and (util.now_ms() - at) < (within_ms or 1000)
end

function M.is_collapsed(window_id)
  return data.collapsed[key(window_id)] == true
end

function M.set_collapsed(window_id, collapsed)
  data.collapsed[key(window_id)] = collapsed or nil
  save(false)
end

function M.is_private(window_id)
  return data.private[key(window_id)] == true
end

function M.set_private(window_id, private)
  data.private[key(window_id)] = private or nil
  save(false)
end

function M.sidebar_pane_id(tab_id)
  return data.sidebars[key(tab_id)]
end

function M.token_for(pane_id)
  return data.tokens[key(pane_id)]
end

---Pane id this plugin minted `token` for, or nil when the token is unknown.
function M.pane_for_token(token)
  if type(token) ~= "string" or token == "" then
    return nil
  end
  for pid, t in pairs(data.tokens) do
    if t == token then
      return tonumber(pid)
    end
  end
  return nil
end

function M.set_sidebar(tab_id, pane_id, token)
  local old = data.sidebars[key(tab_id)]
  if old and old ~= pane_id then
    data.tokens[key(old)] = nil
  end
  data.sidebars[key(tab_id)] = pane_id
  if pane_id then
    data.tokens[key(pane_id)] = token
  end
  save(false)
end

function M.has_focus(window_id)
  return data.focus[key(window_id)] == true
end

function M.set_focus(window_id, focused)
  if (data.focus[key(window_id)] == true) == (focused == true) then
    return
  end
  data.focus[key(window_id)] = focused or nil
  save(false)
end

function M.push_closed(entry)
  table.insert(data.closed, 1, entry)
  while #data.closed > MAX_CLOSED do
    table.remove(data.closed)
  end
  save(true)
end

function M.pop_closed()
  local entry = table.remove(data.closed, 1)
  if entry then
    save(true)
  end
  return entry
end

function M.space_of(tab_id)
  return data.space_of[key(tab_id)]
end

function M.set_space(tab_id, space_id)
  data.space_of[key(tab_id)] = space_id
  save(false)
end

function M.forget_tab(tab_id)
  for _, fn in ipairs(M.forget_tab_hooks) do
    fn(tab_id)
  end
  local k = key(tab_id)
  local pane_id = data.sidebars[k]
  if pane_id then
    data.tokens[key(pane_id)] = nil
  end
  data.pinned[k] = nil
  data.sidebars[k] = nil
  data.space_of[k] = nil
  M.session.content_pane[tab_id] = nil
  M.session.tab_meta[tab_id] = nil
  M.session.moving[tab_id] = nil
  M.session.attaching[tab_id] = nil
  save(true)
end

function M.forget_pane(pane_id)
  for _, name in ipairs(PANE_SESSION) do
    M.session[name][pane_id] = nil
  end
  M.session.pane_domain[pane_id] = nil
  M.session.given_up[pane_id] = nil
end

---Modules with their own per-window caches register a cleaner here; state must not require them.
M.forget_hooks = {}

---The same, per tab.
M.forget_tab_hooks = {}

function M.forget_window(window_id)
  for _, name in ipairs(WINDOW_SESSION) do
    M.session[name][window_id] = nil
  end
  data.collapsed[key(window_id)] = nil
  data.focus[key(window_id)] = nil
  data.private[key(window_id)] = nil
  for _, fn in ipairs(M.forget_hooks) do
    util.try(fn, window_id)
  end
  save(false)
end

---Drops state for windows the mux no longer knows; `live` is a set of window ids.
function M.forget_windows_except(live)
  local ids = {}
  for _, name in ipairs { "collapsed", "focus", "private" } do
    for k in pairs(data[name]) do
      ids[tonumber(k)] = true
    end
  end
  for _, name in ipairs(WINDOW_SESSION) do
    for k in pairs(M.session[name]) do
      ids[k] = true
    end
  end
  for id in pairs(ids) do
    if not live[id] then
      M.forget_window(id)
    end
  end
end

return M

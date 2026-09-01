local wezterm = require "wezterm" ---@type Wezterm
local store = require "vtabs.store"
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

---Only id-free data survives the process; pane/tab/window ids are re-used by the next mux. Pins
---and space assignments are keyed by tab id and only restored once a surviving pane proves the mux
---that minted them lived on.
local PERSISTED = { "closed", "pinned", "space_of", "space_manual" }

local function empty()
  return {
    pinned = {},
    sidebars = {},
    tokens = {},
    private = {},
    closed = {},
    space_of = {},
    space_manual = {},
    dynamic_spaces = {},
    active_space = {},
    collapsed = {},
    focus = {},
  }
end

local function short_string(value)
  return type(value) == "string" and #value <= 64 and value or nil
end

local function copy_closed(list)
  local out = {}
  for _, entry in ipairs(type(list) == "table" and list or {}) do
    if type(entry) == "table" and #out < MAX_CLOSED then
      out[#out + 1] = {
        cwd = type(entry.cwd) == "string" and entry.cwd or nil,
        domain = type(entry.domain) == "string" and entry.domain or nil,
        title = type(entry.title) == "string" and entry.title or nil,
        space = short_string(entry.space),
        space_manual = entry.space_manual == true or nil,
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

local function copy_ids(tbl)
  local out = {}
  for k, v in pairs(type(tbl) == "table" and tbl or {}) do
    if type(k) == "string" and short_string(v) then
      out[k] = v
    end
  end
  return out
end

local function read_file()
  if util.is_symlink(M.file) then
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

local function write_file(tbl)
  local body = wezterm.json_encode(tbl)
  if body == last_written then
    return
  end
  if util.write_private(M.file, body, state_dir(), "state") then
    last_written = body
  end
end

local data = empty()
local deferred = nil

---Pins and spaces are keyed by tab id, so they are only meaningful while the mux that minted them lives.
local function load()
  local saved = wezterm.GLOBAL and wezterm.GLOBAL.vtabs or nil
  data = empty()
  deferred = nil
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
  deferred = {
    pinned = copy_pins(file.pinned),
    space_of = copy_ids(file.space_of),
    space_manual = copy_pins(file.space_manual),
  }
end

load()

---Re-reads `wezterm.GLOBAL` and the state file; the file is only consulted in a fresh Lua VM.
function M.reload()
  last_written = nil
  load()
end

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

---True while pins and spaces read from the file wait for proof that the mux that minted their tab
---ids survived.
function M.pins_pending()
  return deferred ~= nil
end

---The file wins over anything assigned provisionally while the proof was pending.
function M.restore_pins()
  if not deferred then
    return
  end
  for _, name in ipairs { "pinned", "space_of", "space_manual" } do
    for k, v in pairs(deferred[name]) do
      data[name][k] = v
    end
  end
  deferred = nil
  save(true)
end

function M.discard_pins()
  deferred = nil
  save(true)
end

---True just after we applied a config override, so its reload event can be told apart from a real one.
function M.applying_recently(window_id, within_ms)
  local at = store.applying[window_id]
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

---A token for a backend pane that is not a tab's sidebar - the settings page. The bridge trusts
---the echo, not the role, so registration is the same; only the tab mapping is not.
function M.set_token(pane_id, token)
  data.tokens[key(pane_id)] = token
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

function M.space_manual(tab_id)
  return data.space_manual[key(tab_id)] == true
end

---A hand move reaches disk at once; a rule's verdict is re-derived by the same rule after a
---restart, so it rides along with the next write instead of costing one of its own.
function M.set_space(tab_id, space_id, manual)
  data.space_of[key(tab_id)] = space_id
  data.space_manual[key(tab_id)] = manual == true or nil
  save(manual == true)
end

function M.active_space(window_id)
  return data.active_space[key(window_id)]
end

function M.set_active_space(window_id, space_id)
  if data.active_space[key(window_id)] == space_id then
    return
  end
  data.active_space[key(window_id)] = space_id
  save(false)
end

---Spaces no entry declares under their id: the ones a template or the route hook produced.
function M.dynamic_space(space_id)
  return data.dynamic_spaces[space_id]
end

function M.dynamic_spaces()
  return data.dynamic_spaces
end

function M.set_dynamic_space(space_id, meta)
  data.dynamic_spaces[space_id] = meta
  save(false)
end

function M.forget_tab(tab_id)
  local k = key(tab_id)
  local pane_id = data.sidebars[k]
  if pane_id then
    data.tokens[key(pane_id)] = nil
  end
  data.pinned[k] = nil
  data.sidebars[k] = nil
  data.space_of[k] = nil
  data.space_manual[k] = nil
  store.forget_tab(tab_id)
  save(true)
end

function M.forget_pane(pane_id)
  store.forget_pane(pane_id)
end

---The per-process bus lives in `store` now, where each field is declared with its own scope. This
---is the name the test suite still reaches it by; production code requires `store` directly.
M.session = store.fields

---For cleanup a table cannot express: `frame` unlinks the window's PNG here. A cache that only
---needs forgetting declares its scope through `store` instead and registers nothing.
M.forget_hooks = {}

function M.forget_window(window_id)
  store.forget_window(window_id)
  data.collapsed[key(window_id)] = nil
  data.focus[key(window_id)] = nil
  data.private[key(window_id)] = nil
  data.active_space[key(window_id)] = nil
  for _, fn in ipairs(M.forget_hooks) do
    util.try(fn, window_id)
  end
  save(false)
end

---Drops state for windows the mux no longer knows; `live` is a set of window ids.
function M.forget_windows_except(live)
  local ids = {}
  for _, name in ipairs { "collapsed", "focus", "private", "active_space" } do
    for k in pairs(data[name]) do
      ids[tonumber(k)] = true
    end
  end
  for id in pairs(store.window_ids()) do
    ids[id] = true
  end
  for id in pairs(ids) do
    if not live[id] then
      M.forget_window(id)
    end
  end
end

return M

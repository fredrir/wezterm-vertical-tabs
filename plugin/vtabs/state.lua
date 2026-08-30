local wezterm = require "wezterm" ---@type Wezterm
local util = require "vtabs.util"

local M = {}

local MAX_CLOSED = 20

local function state_dir()
  local base = os.getenv "XDG_STATE_HOME"
  if not base or base == "" then
    base = (wezterm.home_dir or os.getenv "HOME" or ".") .. "/.local/state"
  end
  return base .. "/wez-vtabs"
end

M.file = state_dir() .. "/state.json"

local PERSISTED = { "pinned", "sidebars", "tokens", "private", "closed", "space_of", "spaces" }

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

local function read_file()
  local f = io.open(M.file, "r")
  if not f then
    return nil
  end
  local body = f:read "a"
  f:close()
  local parsed = util.try(wezterm.json_parse, body)
  return type(parsed) == "table" and parsed or nil
end

local function write_file(tbl)
  local f = io.open(M.file, "w")
  if not f then
    util.try(wezterm.run_child_process, { "mkdir", "-p", state_dir() })
    f = io.open(M.file, "w")
  end
  if not f then
    util.warn_once("state-file", "cannot write %s", M.file)
    return
  end
  f:write(wezterm.json_encode(tbl))
  f:close()
end

local function load()
  local s = empty()
  local sources = { read_file(), wezterm.GLOBAL and wezterm.GLOBAL.vtabs or nil }
  for _, saved in ipairs(sources) do
    if type(saved) == "table" then
      for k, v in pairs(saved) do
        if type(v) == "table" and s[k] then
          local copy = {}
          for kk, vv in pairs(v) do
            copy[kk] = vv
          end
          s[k] = copy
        end
      end
    end
  end
  return s
end

local data = load()

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
  last_active = {},
  attaching = {},
  pane_domain = {},
  failed_domains = {},
}

local function save(persist)
  if wezterm.GLOBAL then
    wezterm.GLOBAL.vtabs = data
  end
  if persist then
    local subset = {}
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
  save(true)
end

function M.sidebar_pane_id(tab_id)
  return data.sidebars[key(tab_id)]
end

function M.token_for(pane_id)
  return data.tokens[key(pane_id)]
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
  save(true)
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
  save(true)
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
  M.session.content_pane[tab_id] = nil
  M.session.tab_meta[tab_id] = nil
  M.session.moving[tab_id] = nil
  M.session.attaching[tab_id] = nil
  save(true)
end

function M.forget_pane(pane_id)
  for _, name in ipairs { "hits", "frames", "dims", "ready", "seen", "pinged", "sent_at" } do
    M.session[name][pane_id] = nil
  end
end

function M.forget_window(window_id)
  for _, name in ipairs {
    "hover",
    "drag",
    "scroll",
    "user_scrolled",
    "last_click",
    "known_tabs",
    "focus_index",
    "last_active",
  } do
    M.session[name][window_id] = nil
  end
  data.collapsed[key(window_id)] = nil
  data.focus[key(window_id)] = nil
  data.private[key(window_id)] = nil
  save(true)
end

return M

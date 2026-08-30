local wezterm = require "wezterm" ---@type Wezterm

local M = {}

local MAX_CLOSED = 20

local function load()
  local saved = wezterm.GLOBAL and wezterm.GLOBAL.vtabs or nil
  local s = {
    pinned = {},
    collapsed = {},
    private = {},
    sidebars = {},
    closed = {},
    focus = {},
  }
  if type(saved) == "table" then
    for k, v in pairs(saved) do
      if type(v) == "table" then
        local copy = {}
        for kk, vv in pairs(v) do
          copy[kk] = vv
        end
        s[k] = copy
      end
    end
  end
  return s
end

local data = load()

---Transient per-session data that must not survive config reloads.
M.session = {
  hover = {},
  drag = {},
  scroll = {},
  hits = {},
  frames = {},
  last_click = {},
  content_pane = {},
  tab_meta = {},
  dims = {},
}

function M.save()
  if wezterm.GLOBAL then
    wezterm.GLOBAL.vtabs = data
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
  M.save()
end

function M.is_collapsed(window_id)
  return data.collapsed[key(window_id)] == true
end

function M.set_collapsed(window_id, collapsed)
  data.collapsed[key(window_id)] = collapsed or nil
  M.save()
end

function M.is_private(window_id)
  return data.private[key(window_id)] == true
end

function M.set_private(window_id, private)
  data.private[key(window_id)] = private or nil
  M.save()
end

function M.sidebar_pane_id(tab_id)
  return data.sidebars[key(tab_id)]
end

function M.set_sidebar(tab_id, pane_id)
  data.sidebars[key(tab_id)] = pane_id
  M.save()
end

function M.has_focus(window_id)
  return data.focus[key(window_id)] == true
end

function M.set_focus(window_id, focused)
  data.focus[key(window_id)] = focused or nil
  M.save()
end

function M.push_closed(entry)
  table.insert(data.closed, 1, entry)
  while #data.closed > MAX_CLOSED do
    table.remove(data.closed)
  end
  M.save()
end

function M.pop_closed()
  local entry = table.remove(data.closed, 1)
  if entry then
    M.save()
  end
  return entry
end

function M.forget_tab(tab_id)
  local k = key(tab_id)
  data.pinned[k] = nil
  data.sidebars[k] = nil
  M.save()
end

function M.raw()
  return data
end

return M

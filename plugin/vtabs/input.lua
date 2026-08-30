local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local view = require "vtabs.view"
local actions = require "vtabs.actions"
local menu = require "vtabs.menu"
local hit = require "vtabs.hit"
local util = require "vtabs.util"

local M = {}

local DRAG_TIMEOUT_MS = 3000
local DRAG_START_ROWS = 1
local DRAG_START_COLS = 2
local TEAR_OFF_TRAVEL = 3

local session = state.session

local function blur(gui_window)
  actions.blur_sidebar(gui_window)
end

local function on_down(gui_window, pane, ev, cfg)
  local wid = gui_window:window_id()
  local h = hit.at(session.hits[pane:pane_id()], ev.y)
  local now = util.now_ms()
  session.hover[wid] = { x = ev.x, y = ev.y, at = now }
  session.drag[wid] = nil
  state.set_focus(wid, false)
  if cfg.debug then
    util.log("down hit=%s tab=%s slot=%s", h.kind, tostring(h.tab_id), tostring(h.slot))
  end
  local target = h.kind == "tab" and ("tab:" .. h.tab_id) or h.kind
  local double, last = hit.double_click(session.last_click[wid], target, now, cfg.double_click_ms)
  session.last_click[wid] = last

  if ev.b == "left" then
    if h.kind == "tab" then
      if hit.in_close(h, ev.x) then
        actions.close_tab(gui_window, h.tab_id)
      else
        actions.activate_tab(gui_window, h.tab_id)
        session.drag[wid] = { tab_id = h.tab_id, origin_x = ev.x, origin_y = ev.y, active = false, at = now }
      end
    elseif h.kind == "new_tab" then
      actions.new_tab(gui_window)
    elseif h.kind == "footer" and h.entry.on_click then
      pcall(h.entry.on_click, gui_window, h.entry)
    elseif h.kind == "space" and double then
      actions.new_tab(gui_window)
    end
  elseif ev.b == "middle" and h.kind == "tab" then
    actions.close_tab(gui_window, h.tab_id)
  elseif ev.b == "right" and h.kind == "tab" then
    menu.open(gui_window, h.tab_id)
    return
  end
  blur(gui_window)
end

local function on_drag(gui_window, pane, ev, cfg)
  local wid = gui_window:window_id()
  local drag = session.drag[wid]
  if not drag or ev.b ~= "left" then
    return
  end
  drag.at = util.now_ms()
  local dx = math.abs(ev.x - drag.origin_x)
  if not drag.active and (math.abs(ev.y - drag.origin_y) >= DRAG_START_ROWS or dx >= DRAG_START_COLS) then
    drag.active = true
  end
  if drag.active then
    local pid = pane:pane_id()
    local dims = session.dims[pid] or { cols = cfg.width, rows = ev.y }
    drag.over_index = hit.drop_slot(session.hits[pid], ev.y, dims.rows, cfg.padding.top)
    drag.outside = cfg.tear_off and dx >= TEAR_OFF_TRAVEL and hit.on_inner_edge(ev.x, dims.cols, cfg.position)
  end
  session.hover[wid] = { x = ev.x, y = ev.y, at = drag.at }
end

local function on_up(gui_window, pane, ev, cfg)
  local wid = gui_window:window_id()
  local drag = session.drag[wid]
  session.drag[wid] = nil
  if not drag or not drag.active then
    return
  end
  local dims = session.dims[pane:pane_id()] or { cols = cfg.width }
  local travelled = math.abs(ev.x - drag.origin_x) >= TEAR_OFF_TRAVEL
  if drag.outside or (cfg.tear_off and travelled and hit.on_inner_edge(ev.x, dims.cols, cfg.position)) then
    if actions.tear_off(gui_window, drag.tab_id) then
      return
    end
  elseif drag.over_index then
    actions.move_tab_to_slot(gui_window, drag.tab_id, drag.over_index)
  end
  blur(gui_window)
end

local function on_wheel(gui_window, ev, cfg)
  local wid = gui_window:window_id()
  if cfg.wheel == "switch" then
    actions.activate_relative(gui_window, ev.dy)
    return
  end
  session.scroll[wid] = (session.scroll[wid] or 0) + ev.dy
  session.user_scrolled[wid] = true
end

function M.mouse(gui_window, pane, ev)
  local cfg = config.get()
  local wid = gui_window:window_id()
  if ev.k == "move" then
    session.hover[wid] = { x = ev.x, y = ev.y, at = util.now_ms() }
  elseif ev.k == "down" then
    on_down(gui_window, pane, ev, cfg)
  elseif ev.k == "drag" then
    on_drag(gui_window, pane, ev, cfg)
  elseif ev.k == "up" then
    on_up(gui_window, pane, ev, cfg)
  elseif ev.k == "wheel" then
    on_wheel(gui_window, ev, cfg)
  end
  view.sync(gui_window)
end

local MOVE = { down = 1, j = 1, tab = 1, up = -1, k = -1 }

local function with_focused(fn)
  return function(gui_window, items, index)
    local item = items[index]
    if item then
      fn(gui_window, item.tab_id, index)
    end
  end
end

local KEYS = {
  enter = with_focused(function(gui_window, id)
    actions.activate_tab(gui_window, id)
    blur(gui_window)
  end),
  x = with_focused(function(gui_window, id)
    actions.close_tab(gui_window, id)
  end),
  p = with_focused(function(gui_window, id)
    actions.toggle_pin(gui_window, id)
  end),
  r = with_focused(function(gui_window, id)
    actions.rename_tab(gui_window, id)
  end),
  m = with_focused(function(gui_window, id)
    menu.open(gui_window, id)
  end),
  J = with_focused(function(gui_window, id, index)
    actions.move_tab_to_slot(gui_window, id, index + 1)
    session.focus_index[gui_window:window_id()] = index + 1
  end),
  K = with_focused(function(gui_window, id, index)
    actions.move_tab_to_slot(gui_window, id, index - 1)
    session.focus_index[gui_window:window_id()] = math.max(index - 1, 1)
  end),
  n = function(gui_window)
    actions.new_tab(gui_window)
    blur(gui_window)
  end,
}
KEYS.space, KEYS.d, KEYS.delete = KEYS.enter, KEYS.x, KEYS.x

function M.key(gui_window, ev)
  local wid = gui_window:window_id()
  if not state.has_focus(wid) then
    blur(gui_window)
    return
  end
  local items = model.ordered(model.build(gui_window))
  local count = math.max(#items, 1)
  local index = math.max(1, math.min(session.focus_index[wid] or 1, count))
  local key = ev.key
  local shift = util.contains(ev.mods, "shift")
  local ctrl = util.contains(ev.mods, "ctrl")

  if key == "escape" or key == "q" or (ctrl and key == "c") then
    blur(gui_window)
  elseif key == "tab" and shift then
    session.focus_index[wid] = math.max(index - 1, 1)
  elseif MOVE[key] then
    session.focus_index[wid] = math.max(1, math.min(index + MOVE[key], count))
  elseif key == "home" or key == "g" then
    session.focus_index[wid] = 1
  elseif key == "end" or key == "G" then
    session.focus_index[wid] = count
  elseif key:match "^[1-9]$" then
    local item = items[tonumber(key)]
    if item then
      actions.activate_tab(gui_window, item.tab_id)
      blur(gui_window)
    end
  elseif KEYS[key] then
    KEYS[key](gui_window, items, index)
  end
  view.sync(gui_window)
end

---Entry point for the `user-var-changed` event; only registered sidebar panes are trusted.
function M.handle(gui_window, pane, name, value)
  local cfg = config.get()
  if name ~= cfg.backend.uservar or not sidebar.is_sidebar(pane) then
    return
  end
  local ok, ev = pcall(wezterm.json_parse, value)
  if cfg.debug then
    util.log("event %s parsed=%s", tostring(value), tostring(ok))
  end
  if not ok or type(ev) ~= "table" then
    return
  end
  session.seen[pane:pane_id()] = util.now_ms()
  if cfg.debug then
    util.log("handle: %s from pane %d", tostring(ev.t), pane:pane_id())
  end
  if ev.t == "ready" then
    sidebar.auth(pane)
    sidebar.ensure(gui_window)
    view.sync(gui_window, { force = true })
  elseif ev.t == "resize" then
    view.sync(gui_window, { force = true })
  elseif ev.t == "mouse" then
    M.mouse(gui_window, pane, ev)
  elseif ev.t == "key" then
    M.key(gui_window, ev)
  end
end

---Expires stale hover/drag state; called from the status poll.
function M.tick(gui_window)
  local cfg = config.get()
  local wid = gui_window:window_id()
  local now = util.now_ms()
  local hover = session.hover[wid]
  if hover and cfg.hover_timeout_ms > 0 and now - hover.at > cfg.hover_timeout_ms then
    session.hover[wid] = nil
  end
  local drag = session.drag[wid]
  if drag and now - drag.at > DRAG_TIMEOUT_MS then
    session.drag[wid] = nil
  end
  local active = util.active_tab(gui_window)
  local active_id = active and active:tab_id() or nil
  if session.last_active[wid] ~= active_id then
    session.last_active[wid] = active_id
    session.user_scrolled[wid] = nil
  end
end

return M

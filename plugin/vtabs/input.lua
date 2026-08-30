local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local view = require "vtabs.view"
local actions = require "vtabs.actions"
local menu = require "vtabs.menu"
local util = require "vtabs.util"

local M = {}

local session = state.session

local function hit_at(pane, y)
  local hits = session.hits[pane:pane_id()]
  return hits and hits[y] or { kind = "space" }
end

local function in_close(hit, x)
  return hit.close ~= nil and x >= hit.close.from and x <= hit.close.to
end

local function is_double_click(wid, key, now, cfg)
  local last = session.last_click[wid]
  session.last_click[wid] = { key = key, at = now }
  return last ~= nil and last.key == key and now - last.at <= cfg.double_click_ms
end

local function blur(gui_window)
  actions.blur_sidebar(gui_window)
end

local function outside_sidebar(x, cols, cfg)
  if not cfg.tear_off then
    return false
  end
  if cfg.position == "left" then
    return cfg.tear_off == "edge" and x >= cols or x > cols
  end
  return cfg.tear_off == "edge" and x <= 1 or x < 1
end

local function drag_target(pane, y, cfg)
  local hit = hit_at(pane, y)
  if hit.kind == "tab" then
    return hit.slot
  end
  local dims = session.dims[pane:pane_id()] or { rows = y }
  local last_slot = 0
  for row = 1, dims.rows do
    local h = hit_at(pane, row)
    if h.kind == "tab" then
      last_slot = math.max(last_slot, h.slot)
      if row > y then
        return h.slot
      end
    end
  end
  return y < cfg.padding.top + 1 and 1 or last_slot + 1
end

local function on_down(gui_window, pane, ev, cfg)
  local wid = gui_window:window_id()
  local hit = hit_at(pane, ev.y)
  local now = util.now_ms()
  if cfg.debug then
    util.log("down hit=%s tab=%s slot=%s", hit.kind, tostring(hit.tab_id), tostring(hit.slot))
  end
  session.hover[wid] = { x = ev.x, y = ev.y, at = now }
  session.drag[wid] = nil
  state.set_focus(wid, false)

  if ev.b == "left" then
    if hit.kind == "tab" then
      if in_close(hit, ev.x) then
        actions.close_tab(gui_window, hit.tab_id)
      else
        actions.activate_tab(gui_window, hit.tab_id)
        session.drag[wid] = { tab_id = hit.tab_id, origin_x = ev.x, origin_y = ev.y, active = false, at = now }
      end
    elseif hit.kind == "new_tab" then
      actions.new_tab(gui_window)
    elseif hit.kind == "space" and is_double_click(wid, "space", now, cfg) then
      actions.new_tab(gui_window)
    end
  elseif ev.b == "middle" and hit.kind == "tab" then
    actions.close_tab(gui_window, hit.tab_id)
  elseif ev.b == "right" then
    if hit.kind == "tab" then
      menu.open(gui_window, hit.tab_id)
      return
    end
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
  if not drag.active and (math.abs(ev.y - drag.origin_y) >= 1 or dx >= 2) then
    drag.active = true
  end
  if drag.active then
    local dims = session.dims[pane:pane_id()] or { cols = cfg.width }
    drag.over_index = drag_target(pane, ev.y, cfg)
    drag.outside = dx >= 3 and outside_sidebar(ev.x, dims.cols, cfg)
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
  local travelled = math.abs(ev.x - drag.origin_x) >= 3
  if drag.outside or (travelled and outside_sidebar(ev.x, dims.cols, cfg)) then
    actions.tear_off(gui_window, drag.tab_id)
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
  session.user_scrolled = session.user_scrolled or {}
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

local function focused_tab_id(gui_window, index)
  local items = model.ordered(model.build(gui_window))
  local item = items[index]
  return item and item.tab_id or nil, #items
end

function M.key(gui_window, ev)
  local wid = gui_window:window_id()
  if not state.has_focus(wid) then
    blur(gui_window)
    return
  end
  session.focus_index = session.focus_index or {}
  local index = session.focus_index[wid] or 1
  local key = ev.key
  local ctrl = util.contains(ev.mods, "ctrl")
  local _, count = focused_tab_id(gui_window, index)

  if key == "escape" or key == "q" or (ctrl and key == "c") then
    blur(gui_window)
  elseif key == "down" or key == "j" or key == "tab" then
    session.focus_index[wid] = math.min(index + 1, math.max(count, 1))
  elseif key == "up" or key == "k" then
    session.focus_index[wid] = math.max(index - 1, 1)
  elseif key == "home" or key == "g" then
    session.focus_index[wid] = 1
  elseif key == "end" or key == "G" then
    session.focus_index[wid] = math.max(count, 1)
  elseif key == "enter" or key == "space" then
    local id = focused_tab_id(gui_window, index)
    if id then
      actions.activate_tab(gui_window, id)
    end
    blur(gui_window)
  elseif key == "x" or key == "d" or key == "delete" then
    local id = focused_tab_id(gui_window, index)
    if id then
      actions.close_tab(gui_window, id)
    end
  elseif key == "p" then
    local id = focused_tab_id(gui_window, index)
    if id then
      actions.toggle_pin(gui_window, id)
    end
  elseif key == "n" then
    actions.new_tab(gui_window)
    blur(gui_window)
  elseif key == "r" then
    local id = focused_tab_id(gui_window, index)
    if id then
      actions.rename_tab(gui_window, id)
    end
  elseif key == "m" then
    local id = focused_tab_id(gui_window, index)
    if id then
      menu.open(gui_window, id)
    end
  elseif key == "J" then
    local id = focused_tab_id(gui_window, index)
    if id then
      actions.move_tab_to_slot(gui_window, id, index + 1)
      session.focus_index[wid] = math.min(index + 1, count)
    end
  elseif key == "K" then
    local id = focused_tab_id(gui_window, index)
    if id then
      actions.move_tab_to_slot(gui_window, id, index - 1)
      session.focus_index[wid] = math.max(index - 1, 1)
    end
  end
  view.sync(gui_window)
end

---Entry point for the `user-var-changed` event.
function M.handle(gui_window, pane, name, value)
  local cfg = config.get()
  if name ~= cfg.backend.uservar then
    return
  end
  local ok, ev = pcall(wezterm.json_parse, value)
  if cfg.debug then
    util.log("event %s parsed=%s", tostring(value), tostring(ok))
  end
  if not ok or type(ev) ~= "table" then
    return
  end
  if ev.t == "ready" or ev.t == "resize" then
    sidebar.ensure(gui_window)
    view.sync(gui_window, { force = true })
  elseif ev.t == "focus" then
    if not ev["in"] then
      session.hover[gui_window:window_id()] = nil
      view.sync(gui_window)
    end
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
  if drag and now - drag.at > 3000 then
    session.drag[wid] = nil
  end
  local active = gui_window:mux_window():active_tab()
  local active_id = active and active:tab_id() or nil
  session.last_active = session.last_active or {}
  if session.last_active[wid] ~= active_id then
    session.last_active[wid] = active_id
    if session.user_scrolled then
      session.user_scrolled[wid] = nil
    end
  end
end

return M

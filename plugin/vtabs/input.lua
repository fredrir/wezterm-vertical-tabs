local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local view = require "vtabs.view"
local geometry = require "vtabs.geometry"
local actions = require "vtabs.actions"
local popover = require "vtabs.popover"
local hit = require "vtabs.hit"
local layout = require "vtabs.layout"
local settings = require "vtabs.settings"
local page = require "vtabs.page"
local util = require "vtabs.util"

local M = {}

local DRAG_TIMEOUT_MS = 3000

---Runs the strip button under the pointer. `settings` and `search` are the user's to define, so they
---reach `hooks`; a table entry in `strip_actions` carries its own callback.
local function strip_action(gui_window, id)
  if id == nil then
    return
  end
  if id == "toggle" then
    return actions.toggle_sidebar(gui_window)
  end
  if id == "new_tab" then
    return actions.new_tab(gui_window)
  end
  local cfg = config.get()
  local hook = (cfg.hooks or {})[id]
  if type(hook) == "function" then
    return util.try(hook, gui_window)
  end
  for _, entry in ipairs(cfg.strip_actions or {}) do
    if type(entry) == "table" and (entry.id or "custom") == id and type(entry.on_click) == "function" then
      return util.try(entry.on_click, gui_window)
    end
  end
end
local DRAG_START_ROWS = 3
local DRAG_START_COLS = 2
local DRAG_DWELL_MS = 120
local TEAR_OFF_TRAVEL = 3

local session = state.session
local pending_menu = {}
local pending_close = {}

local function blur(gui_window)
  actions.blur_sidebar(gui_window)
end

---Press keeps the sidebar as the tab's active pane so the drag and the release reach it too.
---While a popover is open it takes the whole sidebar: left acts, right retargets, middle is inert.
local function on_popover_down(gui_window, pane, h, ev)
  if ev.b == "left" then
    if h.kind == "scrim" then
      popover.close(gui_window)
      view.invalidate_frames(pane:pane_id())
    elseif h.kind == "popover" and h.id and not h.disabled then
      popover.run(gui_window, h.id)
      view.invalidate_frames(pane:pane_id())
    end
  elseif ev.b == "right" and h.kind == "scrim" then
    -- Close, repaint, then let the release open one for whatever row is now under the pointer.
    popover.close(gui_window)
    view.invalidate_frames(pane:pane_id())
    view.sync(gui_window)
    return true
  end
end

local function on_down(gui_window, pane, ev, cfg)
  local wid = gui_window:window_id()
  local pid = pane:pane_id()
  local h = hit.at(session.hits[pid], ev.y)
  local now = util.now_ms()
  if popover.get(wid) then
    if not on_popover_down(gui_window, pane, h, ev) then
      return
    end
    h = hit.at(session.hits[pid], ev.y)
  end
  session.hover[wid] = { x = ev.x, y = ev.y, at = now }
  session.drag[wid] = nil
  pending_menu[wid] = nil
  pending_close[wid] = nil
  state.set_focus(wid, false)
  if cfg.debug then
    util.log("down hit=%s tab=%s slot=%s", h.kind, tostring(h.id), tostring(h.slot))
  end
  -- Cols 1 and 28 carry no card surface, so a click there is empty space, not the row's tab.
  local on_card = h.kind == "tab" and hit.in_card(h, ev.x)
  local target = on_card and ("tab:" .. h.id) or h.kind
  local double, last = hit.double_click(session.last_click[wid], target, now, cfg.double_click_ms)
  session.last_click[wid] = last

  if ev.b == "left" then
    if on_card then
      local span = hit.span(h, ev.x)
      if span == "close" then
        pending_close[wid] = { tab_id = h.id, at = now, row = ev.y, col = ev.x }
      elseif span == "pin" then
        actions.toggle_pin(gui_window, h.id)
      else
        local focused = actions.activate_tab(gui_window, h.id, "sidebar")
        session.drag[wid] = {
          tab_id = h.id,
          origin_x = ev.x,
          origin_y = ev.y,
          pane_id = focused and focused:pane_id() or pid,
          active = false,
          began = now,
          at = now,
        }
      end
    elseif h.kind == "action" and hit.in_card(h, ev.x) then
      strip_action(gui_window, hit.span(h, ev.x))
    elseif h.kind == "new_tab" then
      actions.new_tab(gui_window)
    elseif h.kind == "footer" and h.entry and h.entry.on_click then
      pcall(h.entry.on_click, gui_window, h.entry)
    elseif h.kind ~= "footer" and double and (h.kind == "space" or h.kind == "strip" or not on_card) then
      actions.new_tab(gui_window)
    end
  elseif ev.b == "middle" and on_card then
    pending_close[wid] = { tab_id = h.id, at = now, row = ev.y, col = ev.x }
  elseif ev.b == "right" and on_card and cfg.context == "popover" then
    pending_menu[wid] = { tab_id = h.id, at = now, row = ev.y, col = ev.x }
  end
end

local function on_drag(gui_window, pane, ev, cfg)
  local wid = gui_window:window_id()
  local pid = pane:pane_id()
  if popover.get(wid) then
    return
  end
  local drag = session.drag[wid]
  local hits = session.hits[pid]
  if not drag or ev.b ~= "left" or drag.pane_id ~= pid or not hits then
    return
  end
  drag.at = util.now_ms()
  local dx = math.abs(ev.x - drag.origin_x)
  local dy = math.abs(ev.y - drag.origin_y)
  -- a short card must not put its neighbour out of reach: never ask for more than one slot of travel
  local rows_needed = math.max(2, math.min(DRAG_START_ROWS, layout.slot_rows(cfg) - 1))
  local past_threshold = dy >= rows_needed or dx >= DRAG_START_COLS
  if not drag.active and past_threshold and drag.at - drag.began >= DRAG_DWELL_MS then
    drag.active = true
  end
  if drag.active then
    local dims = session.dims[pid] or { cols = cfg.width, rows = ev.y }
    drag.over_index = hit.drop_slot(hits, ev.y, dims.rows)
    drag.outside = cfg.tear_off and dx >= TEAR_OFF_TRAVEL and hit.on_inner_edge(ev.x, dims.cols, cfg.position)
  end
  session.hover[wid] = { x = ev.x, y = ev.y, at = drag.at }
end

---A press on the ✕ or a middle click closes only when the release lands on the same target again.
local function released_on(hits, ev, pending)
  local h = hit.at(hits, ev.y)
  if h.kind ~= "tab" or h.id ~= pending.tab_id or not hit.in_card(h, ev.x) then
    return false
  end
  return ev.b == "middle" or hit.span(h, ev.x) == "close"
end

---Everything that closes or opens a level acts on the release; a held button cancels an overlay.
local function on_up(gui_window, pane, ev, cfg)
  local wid = gui_window:window_id()
  local pid = pane:pane_id()
  local drag = session.drag[wid]
  local menu_for = pending_menu[wid]
  local close_for = pending_close[wid]
  session.drag[wid] = nil
  pending_menu[wid] = nil
  pending_close[wid] = nil
  if popover.get(wid) and ev.b ~= "right" then
    return
  end
  if ev.b == "right" then
    if menu_for then
      popover.open(gui_window, menu_for.tab_id, menu_for.row, menu_for.col)
      view.invalidate_frames(pid)
    end
    return
  end
  if close_for and released_on(session.hits[pid], ev, close_for) then
    actions.request_close(gui_window, close_for.tab_id, close_for.row, close_for.col)
    view.invalidate_frames(pid)
    return
  end
  if drag and drag.active and drag.pane_id == pid and session.hits[pid] then
    local dims = session.dims[pid] or { cols = cfg.width }
    local travelled = math.abs(ev.x - drag.origin_x) >= TEAR_OFF_TRAVEL
    if drag.outside or (cfg.tear_off and travelled and hit.on_inner_edge(ev.x, dims.cols, cfg.position)) then
      if actions.tear_off(gui_window, drag.tab_id) then
        return
      end
    elseif drag.over_index then
      actions.move_tab_to_slot(gui_window, drag.tab_id, drag.over_index)
    end
  end
  if cfg.hover == "press" then
    blur(gui_window)
  end
end

local function on_wheel(gui_window, ev, cfg)
  local wid = gui_window:window_id()
  if popover.get(wid) then
    popover.move(gui_window, ev.dy > 0 and 1 or -1)
    return
  end
  if cfg.wheel == "switch" then
    actions.activate_relative(gui_window, ev.dy)
    return
  end
  session.scroll[wid] = (session.scroll[wid] or 0) + ev.dy
  session.user_scrolled[wid] = true
end

---Motion only needs a repaint when it crosses a row or a sub-target span of the row it is on.
local function hover_moved(previous, ev, pid)
  if not previous or previous.y ~= ev.y then
    return true
  end
  local h = hit.at(session.hits[pid], ev.y)
  return hit.span(h, previous.x) ~= hit.span(h, ev.x)
end

function M.mouse(gui_window, pane, ev)
  local cfg = config.get()
  local wid = gui_window:window_id()
  local had_popover = popover.get(wid) ~= nil
  if ev.k == "move" then
    local pid = pane:pane_id()
    local moved = hover_moved(session.hover[wid], ev, pid)
    session.hover[wid] = { x = ev.x, y = ev.y, at = util.now_ms() }
    -- An open menu owns the pointer: motion moves its selection instead of the list's hover.
    if had_popover then
      if not popover.point_at(gui_window, hit.at(session.hits[pid], ev.y), ev.x) then
        return
      end
      view.invalidate_frames(pid)
    elseif not moved then
      return
    end
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
  -- The fade needs the frame that put the menu on screen, so it plays after that sync, not before.
  if not had_popover and popover.get(wid) then
    view.animate_popover(gui_window)
  end
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
  x = with_focused(function(gui_window, id, index)
    actions.request_close(gui_window, id, index)
  end),
  p = with_focused(function(gui_window, id)
    actions.toggle_pin(gui_window, id)
  end),
  r = with_focused(function(gui_window, id)
    actions.rename_tab(gui_window, id)
  end),
  m = with_focused(function(gui_window, id, index)
    popover.open(gui_window, id, index)
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

local FORWARD_MAX_RAW = 64
local FORWARD_MAX_BYTES = 16
local FORWARD_BURST = 20
local FORWARD_PER_SEC = 60
local PASTE_MAX_RAW = 96 * 1024
local PASTE_MAX_BYTES = 64 * 1024
local BUDGET_TTL_MS = 60000
local budget = {}

---Token bucket per source pane, charged by size, so one big paste borrows against the next second.
local function affordable(pid, now, cost)
  local b = budget[pid]
  if not b then
    b = { tokens = FORWARD_BURST, at = now }
    budget[pid] = b
  end
  b.tokens = math.min(FORWARD_BURST, b.tokens + (now - b.at) * FORWARD_PER_SEC / 1000)
  b.at = now
  if b.tokens < 1 then
    return false
  end
  b.tokens = b.tokens - cost
  return true
end

-- ESC-prefixed key shapes: CSI (params, intermediates, final 0x40-0x7e), SS3, and the alt-key form.
local KEY_SHAPES = { "^\27%[[\48-\63]*[\32-\47]*[\64-\126]$", "^\27O.$", "^\27.$" }
local PASTE_BRACKETS = { "\27[200~", "\27[201~" }

---`text` when it is structurally one key press, else nil.
function M.safe_key_bytes(text)
  if type(text) ~= "string" or text == "" or #text > FORWARD_MAX_BYTES then
    return nil
  end
  for _, bracket in ipairs(PASTE_BRACKETS) do
    if text:find(bracket, 1, true) then
      return nil
    end
  end
  if utf8.len(text) == 1 then
    return text
  end
  for _, shape in ipairs(KEY_SHAPES) do
    if text:find(shape) then
      return text
    end
  end
  return nil
end

local function decoded_key(ev)
  if type(ev.raw) == "string" then
    return #ev.raw <= FORWARD_MAX_RAW and util.base64_decode(ev.raw) or nil
  end
  local key = ev.key
  if type(key) ~= "string" or utf8.len(key) ~= 1 then
    return nil
  end
  if util.contains(ev.mods, "ctrl") or util.contains(ev.mods, "alt") then
    return nil
  end
  local code = utf8.codepoint(key)
  return code >= 32 and code ~= 127 and key or nil
end

---The content pane a hovered sidebar may hand input to, or nil when any part of that claim fails.
local function handover_target(gui_window, pane, cfg)
  local wid = gui_window:window_id()
  local tab = util.try(function()
    return pane:tab()
  end)
  local active = util.active_tab(gui_window)
  if not sidebar.is_ready(pane) or not tab or not active or tab:tab_id() ~= active:tab_id() then
    return nil
  end
  if cfg.hover == "press" and not session.drag[wid] then
    return nil
  end
  local content = sidebar.content_pane(tab)
  if not content or content:pane_id() == pane:pane_id() then
    return nil
  end
  if sidebar.is_backend(content) or sidebar.is_overlay(content) then
    return nil
  end
  local domain = util.try(function()
    return pane:get_domain_name()
  end)
  if domain == nil or domain ~= util.try(function()
    return content:get_domain_name()
  end) then
    return nil
  end
  return content
end

---Focus follows the handover whether or not bytes went with it, so no second key is lost the same way.
local function hand_over(gui_window, content, deliver)
  if deliver then
    pcall(deliver)
  end
  content:activate()
  state.set_focus(gui_window:window_id(), false)
end

---A key at a sidebar that is not in keyboard mode is the user typing at their shell: hand it over.
local function forward_key(gui_window, pane, ev, cfg)
  local content = handover_target(gui_window, pane, cfg)
  if not content then
    return
  end
  local text = M.safe_key_bytes(decoded_key(ev))
  local pid = pane:pane_id()
  if text and not affordable(pid, util.now_ms(), 1) then
    if cfg.debug then
      util.log("key forward over budget on pane %d", pid)
    end
    text = nil
  end
  hand_over(gui_window, content, text and function()
    content:send_text(text)
  end)
end

---A bracketed paste captured by the backend, delivered whole so the shell brackets it once.
local function forward_paste(gui_window, pane, ev, cfg)
  local content = handover_target(gui_window, pane, cfg)
  if not content then
    return
  end
  local text = nil
  if type(ev.data) == "string" and #ev.data <= PASTE_MAX_RAW then
    text = util.base64_decode(ev.data)
  end
  if text == "" or (text and #text > PASTE_MAX_BYTES) then
    text = nil
  end
  if text and not affordable(pane:pane_id(), util.now_ms(), 1 + #text // 1024) then
    text = nil
  end
  hand_over(gui_window, content, text and function()
    content:paste(text)
  end)
end

local POP_MOVE = { down = 1, j = 1, tab = 1, up = -1, k = -1 }

---Keys belong to the popover while it is open; nothing is forwarded to the shell.
local function popover_key(gui_window, pane, ev)
  local pop = popover.get(gui_window:window_id())
  local key, mods = ev.key, ev.mods
  view.invalidate_frames(pane:pane_id())
  if pop.level == "rename" then
    local done = popover.edit(pop, key, mods)
    if done == "commit" then
      popover.commit_rename(gui_window)
    elseif done == "cancel" then
      popover.back(gui_window)
    end
    return
  end
  if key == "escape" or (util.contains(mods, "ctrl") and key == "c") then
    popover.back(gui_window)
  elseif key == "enter" or key == "space" then
    local item = popover.selected(gui_window)
    if item then
      popover.run(gui_window, item.id)
    end
  elseif POP_MOVE[key] then
    popover.move(gui_window, util.contains(mods, "shift") and -POP_MOVE[key] or POP_MOVE[key])
  elseif type(key) == "string" and utf8.len(key) == 1 then
    popover.jump(gui_window, key)
  end
end

function M.key(gui_window, pane, ev)
  local wid = gui_window:window_id()
  if popover.get(wid) then
    popover_key(gui_window, pane, ev)
    view.sync(gui_window)
    return
  end
  if not state.has_focus(wid) then
    forward_key(gui_window, pane, ev, config.get())
    view.sync(gui_window)
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
  if popover.get(wid) then
    view.animate_popover(gui_window)
  end
end

---Entry point for the `user-var-changed` event; only registered sidebar panes are trusted.
function M.handle(gui_window, pane, name, value)
  local cfg = config.get()
  if name ~= cfg.backend.uservar or not sidebar.is_ready(pane) then
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
  -- the settings page shares the bridge but none of the sidebar's hit map; it answers for itself
  if sidebar.is_settings(pane) then
    if ev.t == "ready" then
      sidebar.auth(pane)
      view.sync(gui_window, { force = true })
    elseif ev.t == "key" or ev.t == "mouse" then
      local dims = session.dims[pane:pane_id()] or { cols = 100, rows = 24 }
      local page_view =
        { cols = dims.cols, rows = dims.rows, cfg = cfg, st = settings.page_state(gui_window:window_id()) }
      local handled
      if ev.t == "key" then
        handled = settings.key(gui_window, ev) or page.key(gui_window, page_view, ev)
      elseif ev.k == "down" and ev.b == "left" then
        handled = page.click(gui_window, page_view, hit.at(session.hits[pane:pane_id()], ev.y), ev.x)
      end
      if handled then
        view.sync(gui_window, { force = true })
      end
    end
    return
  end
  if ev.t == "ready" then
    sidebar.auth(pane)
    sidebar.ensure(gui_window)
    view.sync(gui_window, { force = true })
  elseif ev.t == "resize" then
    -- The sidebar reporting its own new size is the one signal that is never a poll behind the mux.
    geometry.correct(gui_window)
    view.sync(gui_window, { force = true })
  elseif ev.t == "mouse" then
    M.mouse(gui_window, pane, ev)
  elseif ev.t == "key" then
    M.key(gui_window, pane, ev)
  elseif ev.t == "paste" then
    forward_paste(gui_window, pane, ev, cfg)
    view.sync(gui_window)
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
    if cfg.hover == "press" then
      blur(gui_window)
    end
  end
  if pending_menu[wid] and now - pending_menu[wid].at > DRAG_TIMEOUT_MS then
    pending_menu[wid] = nil
  end
  if pending_close[wid] and now - pending_close[wid].at > DRAG_TIMEOUT_MS then
    pending_close[wid] = nil
  end
  for pid, b in pairs(budget) do
    if now - b.at > BUDGET_TTL_MS then
      budget[pid] = nil
    end
  end
  local active = util.active_tab(gui_window)
  local active_id = active and active:tab_id() or nil
  if session.last_active[wid] ~= active_id then
    session.last_active[wid] = active_id
    session.user_scrolled[wid] = nil
  end
end

return M

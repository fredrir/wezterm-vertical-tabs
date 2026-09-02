local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local view = require "vtabs.view"
local geometry = require "vtabs.geometry"
local actions = require "vtabs.actions"
local popover = require "vtabs.popover"
local settings = require "vtabs.settings"
local mux = require "vtabs.mux"
local util = require "vtabs.util"

local M = {}

local DRAG_TIMEOUT_MS = 3000

---Runs the strip button under the pointer, through the one dispatch table every other caller reads.
---A `hooked` button is the user's to point elsewhere, so its hook gets first refusal; the rest are
---the strip's own controls. A table entry in `strip_actions` carries its own callback.
local function strip_action(gui_window, id)
  if id == nil then
    return
  end
  local button = nil
  for _, entry in ipairs(actions.strip) do
    button = entry.id == id and entry or button
  end
  if button and not button.hooked then
    return actions.run(button.action, gui_window)
  end
  local cfg = config.get()
  local hook = (cfg.hooks or {})[id]
  if type(hook) == "function" then
    return util.try(hook, gui_window)
  end
  if button and button.action then
    return actions.run(button.action, gui_window)
  end
  for _, entry in ipairs(cfg.strip_actions or {}) do
    if type(entry) == "table" and (entry.id or "custom") == id and type(entry.on_click) == "function" then
      return util.try(entry.on_click, gui_window)
    end
  end
end
local scope = store.scope "input"

local function blur(gui_window)
  actions.blur_sidebar(gui_window)
end

local DO = {}

function DO.press_card(gui_window, pane, id, args)
  local wid = gui_window:window_id()
  local now = util.now_ms()
  store.drag[wid] = nil
  state.set_focus(wid, false)
  local focused = actions.activate_tab(gui_window, id, "sidebar")
  store.drag[wid] = {
    tab_id = id,
    origin_x = args.x,
    origin_y = args.y,
    pane_id = focused and focused:pane_id() or pane:pane_id(),
    active = false,
    began = now,
    at = now,
  }
end

function DO.drag_to(gui_window, _, _, args)
  local drag = store.drag[gui_window:window_id()]
  if not drag then
    return
  end
  drag.at = util.now_ms()
  drag.active = true
  drag.over_index = args.slot
  drag.outside = args.outside == true
end

function DO.drag_end(gui_window, _, _, args)
  local wid = gui_window:window_id()
  local drag = store.drag[wid]
  store.drag[wid] = nil
  if not drag then
    return
  end
  if args.outside or drag.outside then
    actions.tear_off(gui_window, drag.tab_id)
  elseif args.slot or drag.over_index then
    actions.move_tab_to_slot(gui_window, drag.tab_id, args.slot or drag.over_index)
  end
end

function DO.request_close(gui_window, _, id, args)
  actions.request_close(gui_window, id, args.row, args.col, args.from_key == true)
end

function DO.toggle_pin(gui_window, _, id)
  actions.toggle_pin(gui_window, id)
end

function DO.open_menu(gui_window, _, id, args)
  popover.open(gui_window, id, args.row, args.col)
end

function DO.new_tab(gui_window)
  actions.new_tab(gui_window)
end

function DO.strip(gui_window, _, id)
  strip_action(gui_window, id)
end

function DO.set_scroll(gui_window, _, _, args)
  local wid = gui_window:window_id()
  store.scroll[wid] = args.top or 0
  store.user_scrolled[wid] = args.user == true or nil
end

function DO.wheel_tab(gui_window, _, _, args)
  actions.activate_relative(gui_window, args.dy or 1)
end

function DO.set_focus_index(gui_window, _, _, args)
  store.focus_index[gui_window:window_id()] = args.index
end

function DO.activate_tab_by_id(gui_window, _, id)
  actions.activate_tab(gui_window, id)
  blur(gui_window)
end

function DO.blur_sidebar(gui_window)
  blur(gui_window)
end

function DO.menu_pick(gui_window, _, _, args)
  popover.run(gui_window, args.id)
end

function DO.switch_space(gui_window, _, id)
  if type(id) == "string" then
    actions.switch_space(gui_window, id)
  end
end

function DO.menu_back(gui_window)
  popover.back(gui_window)
end

function DO.menu_closed(gui_window)
  popover.close(gui_window)
end

function DO.rename_commit(gui_window, _, _, args)
  local pop = popover.get(gui_window:window_id())
  if pop and pop.level == "rename" then
    pop.buffer = tostring(args.text or "")
    popover.commit_rename(gui_window)
  end
end

---Footer entries can be id-less closures, so the wire addresses them by index in the sent model.
function DO.footer(gui_window, _, _, args)
  local hook = (config.get().hooks or {}).footer
  if type(hook) ~= "function" then
    return
  end
  local ok, rows = pcall(hook, gui_window:mux_window())
  local entry = ok and type(rows) == "table" and rows[args.index] or nil
  if type(entry) == "table" and type(entry.on_click) == "function" then
    pcall(entry.on_click, gui_window, entry)
  end
end

M.DO = DO

local QUIET_DO = { set_scroll = true, drag_to = true }

local MENU_DO = {
  menu_pick = true,
  menu_back = true,
  menu_closed = true,
  rename_commit = true,
}

---A menu level the backend could not draw: a refused confirm falls through to WezTerm's own
---overlay for a close, and any other refused level simply closes, so the pane never deadlocks.
local function on_note(gui_window, ev)
  if ev.k ~= "menu_refused" then
    util.log("backend note: %s", tostring(ev.k))
    return
  end
  local wid = gui_window:window_id()
  local pop = popover.get(wid)
  popover.close(gui_window)
  if pop and ev.a == "confirm" and pop.confirm == "close" then
    actions.close_with_overlay(gui_window, pop.tab_id)
  end
  view.sync(gui_window)
end

---The outcome of a `kill` or `rescue` the backend ran on its server. A moved pane re-splits the
---tab and the width is corrected around it; a failure is said once and the next poll goes on.
local function on_cli(gui_window, pane, ev)
  if ev.ok ~= true then
    util.warn_once(
      "cli-" .. tostring(ev.op) .. "-" .. pane:pane_id(),
      "backend %s failed: %s",
      tostring(ev.op),
      tostring(ev.detail)
    )
    return
  end
  if ev.op == "rescue" then
    local tab = mux.tab_of(pane)
    if tab then
      sidebar.forget_split(tab:tab_id())
    end
    util.try(geometry.correct, gui_window, true)
  end
end

---`do` events from a painting backend; the popover-open guard mirrors v1's click-through dismiss.
local function on_do(gui_window, pane, ev)
  local handler = DO[ev.a]
  if not handler then
    return
  end
  local wid = gui_window:window_id()
  if popover.get(wid) and not MENU_DO[ev.a] then
    popover.close(gui_window)
  end
  handler(gui_window, pane, ev.id, ev.args or {})
  if not QUIET_DO[ev.a] then
    view.sync(gui_window)
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
    actions.request_close(gui_window, id, index, nil, true)
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
    store.focus_index[gui_window:window_id()] = index + 1
  end),
  K = with_focused(function(gui_window, id, index)
    actions.move_tab_to_slot(gui_window, id, index - 1)
    store.focus_index[gui_window:window_id()] = math.max(index - 1, 1)
  end),
  n = function(gui_window)
    actions.new_tab(gui_window)
    blur(gui_window)
  end,
  ["]"] = function(gui_window)
    actions.cycle_space(gui_window, 1)
  end,
  ["["] = function(gui_window)
    actions.cycle_space(gui_window, -1)
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
local budget = scope.pane()

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
  local tab = mux.tab_of(pane)
  local active = util.active_tab(gui_window)
  if not sidebar.is_ready(pane) or not tab or not active or tab:tab_id() ~= active:tab_id() then
    return nil
  end
  if cfg.hover == "press" and not store.drag[wid] then
    return nil
  end
  local content = sidebar.content_pane(tab)
  if not content or content:pane_id() == pane:pane_id() then
    return nil
  end
  if sidebar.is_backend(content) or sidebar.is_settings(content) or sidebar.is_overlay(content) then
    return nil
  end
  local domain = mux.domain(pane)
  if domain == nil or domain ~= mux.domain(content) then
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

function M.key(gui_window, pane, ev)
  local wid = gui_window:window_id()
  if not state.has_focus(wid) then
    forward_key(gui_window, pane, ev, config.get())
    view.sync(gui_window)
    return
  end
  local items = model.ordered(model.build(gui_window))
  local count = math.max(#items, 1)
  local index = math.max(1, math.min(store.focus_index[wid] or 1, count))
  local key = ev.key
  local shift = util.contains(ev.mods, "shift")
  local ctrl = util.contains(ev.mods, "ctrl")

  if key == "escape" or key == "q" or (ctrl and key == "c") then
    blur(gui_window)
  elseif key == "tab" and shift then
    store.focus_index[wid] = math.max(index - 1, 1)
  elseif MOVE[key] then
    store.focus_index[wid] = math.max(1, math.min(index + MOVE[key], count))
  elseif key == "home" or key == "g" then
    store.focus_index[wid] = 1
  elseif key == "end" or key == "G" then
    store.focus_index[wid] = count
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
  store.seen[pane:pane_id()] = util.now_ms()
  if cfg.debug then
    util.log("handle: %s from pane %d", tostring(ev.t), pane:pane_id())
  end

  if sidebar.is_settings(pane) then
    if ev.t == "ready" then
      local v = tonumber(ev.v) or 1
      if v < 2 or ev.paints ~= true then
        sidebar.refuse_v1(pane, v)
        return
      end
      store.proto[pane:pane_id()] = v
      store.paints[pane:pane_id()] = true
      sidebar.auth(pane)
      view.sync(gui_window)
    elseif ev.t == "do" then
      local st = settings.page_state(gui_window:window_id())
      if require("vtabs.settings_model").act(gui_window, st, ev.a, ev.args) then
        view.sync(gui_window)
      end
    end
    return
  end
  if ev.t == "ready" then
    local v = tonumber(ev.v) or 1
    if v < 2 or ev.paints ~= true then
      sidebar.refuse_v1(pane, v)
      return
    end
    store.proto[pane:pane_id()] = v
    store.paints[pane:pane_id()] = true
    sidebar.auth(pane)
    sidebar.ensure(gui_window)
    view.sync(gui_window)
  elseif ev.t == "resize" then
    -- The sidebar reporting its own new size is the one signal that is never a poll behind the mux,
    -- so it is also the proof that the adjust in flight has landed, and the width to correct from.
    geometry.landed(gui_window:window_id())
    geometry.correct(gui_window, nil, { pane_id = pane:pane_id(), cols = tonumber(ev.cols) })
    view.sync(gui_window)
  elseif ev.t == "do" then
    on_do(gui_window, pane, ev)
  elseif ev.t == "cli" then
    on_cli(gui_window, pane, ev)
  elseif ev.t == "note" then
    on_note(gui_window, ev)
  elseif ev.t == "key" then
    M.key(gui_window, pane, ev)
  elseif ev.t == "paste" then
    forward_paste(gui_window, pane, ev, cfg)
    view.sync(gui_window)
  end
end

---Expires a stale drag; called from the status poll.
function M.tick(gui_window)
  local cfg = config.get()
  local wid = gui_window:window_id()
  local now = util.now_ms()
  local drag = store.drag[wid]
  if drag and now - drag.at > DRAG_TIMEOUT_MS then
    store.drag[wid] = nil
    if cfg.hover == "press" then
      blur(gui_window)
    end
  end
  for pid, b in pairs(budget) do
    if now - b.at > BUDGET_TTL_MS then
      budget[pid] = nil
    end
  end
  local active = util.active_tab(gui_window)
  local active_id = active and active:tab_id() or nil
  if store.last_active[wid] ~= active_id then
    store.last_active[wid] = active_id
    store.user_scrolled[wid] = nil
  end
end

return M

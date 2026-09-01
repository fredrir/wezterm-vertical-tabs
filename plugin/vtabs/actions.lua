local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local hit = require "vtabs.hit"
local mux = require "vtabs.mux"
local util = require "vtabs.util"

local M = {}

function M.tab_by_id(gui_window, tab_id)
  for _, info in ipairs(gui_window:mux_window():tabs_with_info()) do
    if info.tab:tab_id() == tab_id then
      return info.tab, info.index
    end
  end
  return nil
end

local function active_content_pane(gui_window)
  local tab = util.active_tab(gui_window)
  return tab and sidebar.content_pane(tab) or nil
end

local function visible(gui_window)
  return model.ordered(model.build(gui_window))
end

local function spawn_env(gui_window)
  if state.is_private(gui_window:window_id()) then
    return config.get().private.env
  end
  return nil
end

---`focus` picks which pane of the activated tab keeps input: "sidebar" or, by default, the content.
---Sidebars attach lazily on activation and widths are corrected on the active tab only, so leaving
---both to the next poll costs a frame of a tab with no sidebar, or one at the wrong width -- and on
---a mux, a round trip on top of it. The switch does its own, in the same action.
function M.activate_tab(gui_window, tab_id, focus)
  local tab = M.tab_by_id(gui_window, tab_id)
  if not tab then
    return nil
  end
  tab:activate()
  local hidden = state.is_collapsed(gui_window:window_id()) and config.get().collapsed == "hidden"
  local sb = sidebar.find(tab)
  if not sb and not hidden then
    sb = sidebar.attach(tab)
  end
  -- Correction activates the sidebar to land its adjust and hands focus back; doing it before the
  -- switch's own activation keeps that dance from being the last word on which pane holds input.
  require("vtabs.geometry").correct(gui_window)
  local target = (focus == "sidebar" and sb) or sidebar.content_pane(tab)
  if target then
    target:activate()
  end
  return target
end

---`MoveTab` moves the window's active tab, so the target has to be activated first.
local function move_to_index(gui_window, tab_id, index)
  local tab, current = M.tab_by_id(gui_window, tab_id)
  local content = tab and sidebar.content_pane(tab)
  if not content or current == index then
    return false
  end
  tab:activate()
  gui_window:perform_action(act.MoveTab(index), content)
  return true
end

local function restore_tab(gui_window, tab_id)
  local tab = tab_id and M.tab_by_id(gui_window, tab_id)
  if tab then
    tab:activate()
  end
end

---Moves a tab to a physical index (0-based), leaving the active tab as it was.
function M.move_tab_to_index(gui_window, tab_id, index)
  local previous = util.active_tab(gui_window)
  if move_to_index(gui_window, tab_id, index) then
    restore_tab(gui_window, previous and previous:tab_id())
  end
end

---Applies a new order for the visible tabs; hidden tabs keep their relative positions.
function M.reorder(gui_window, visible_ids)
  local wanted = {}
  for _, id in ipairs(visible_ids) do
    wanted[id] = true
  end
  local order = {}
  local next_visible = 1
  for _, info in ipairs(gui_window:mux_window():tabs_with_info()) do
    local id = info.tab:tab_id()
    if wanted[id] then
      order[#order + 1] = visible_ids[next_visible]
      next_visible = next_visible + 1
    else
      order[#order + 1] = id
    end
  end
  local previous = util.active_tab(gui_window)
  local moved = false
  for i, id in ipairs(order) do
    moved = move_to_index(gui_window, id, i - 1) or moved
  end
  if moved then
    restore_tab(gui_window, previous and previous:tab_id())
  end
end

---Physically reorders tabs so the pinned block comes first.
function M.normalize_order(gui_window)
  M.reorder(gui_window, model.ids(visible(gui_window)))
end

function M.set_pinned(gui_window, tab_id, pinned)
  state.set_pinned(tab_id, pinned)
  M.normalize_order(gui_window)
end

function M.toggle_pin(gui_window, tab_id)
  M.set_pinned(gui_window, tab_id, not state.is_pinned(tab_id))
end

---Moves a tab to a slot in the rendered order, pinning or unpinning as it crosses the separator.
function M.move_tab_to_slot(gui_window, tab_id, slot)
  local items = model.build(gui_window)
  local pinned_others = 0
  for _, item in ipairs(items) do
    if item.is_pinned and item.tab_id ~= tab_id then
      pinned_others = pinned_others + 1
    end
  end
  local should_pin = hit.should_pin(slot, pinned_others, state.is_pinned(tab_id))
  if state.is_pinned(tab_id) ~= should_pin then
    state.set_pinned(tab_id, should_pin)
    for _, item in ipairs(items) do
      if item.tab_id == tab_id then
        item.is_pinned = should_pin
      end
    end
  end
  local ids = {}
  for _, item in ipairs(model.ordered(items)) do
    if item.tab_id ~= tab_id then
      ids[#ids + 1] = item.tab_id
    end
  end
  table.insert(ids, math.max(1, math.min(slot, #ids + 1)), tab_id)
  M.reorder(gui_window, ids)
end

---True when WezTerm would prompt before closing any of these panes.
local function needs_prompt(gui_window, panes)
  local skip = (mux.effective_config(gui_window) or {}).skip_close_confirmation_for_processes_named or {}
  for _, p in ipairs(panes) do
    local name = util.basename(mux.foreground(p))
    if not name or not util.contains(skip, name) then
      return true
    end
  end
  return false
end

---The unpinned tabs `close_others` would take, in rendered order.
function M.others(gui_window, tab_id)
  local out = {}
  for _, item in ipairs(visible(gui_window)) do
    if item.tab_id ~= tab_id and not item.is_pinned then
      out[#out + 1] = item.tab_id
    end
  end
  return out
end

local function content_of(gui_window, tab_id)
  local tab = M.tab_by_id(gui_window, tab_id)
  return tab, tab and sidebar.classify(tab) or {}
end

---Whether closing would prompt. `kind` is "close" or "close_others"; a mux pane reports no
---foreground process at all, which the skip list can never name, so it always counts.
function M.needs_confirm(gui_window, tab_id, kind)
  if config.get().confirm_close == false then
    return false
  end
  local ids = kind == "close_others" and M.others(gui_window, tab_id) or { tab_id }
  for _, id in ipairs(ids) do
    local _, content = content_of(gui_window, id)
    if #content > 0 and needs_prompt(gui_window, content) then
      return true
    end
  end
  return false
end

local function close_now(gui_window, tab_id, defer, overlay)
  local tab, content = content_of(gui_window, tab_id)
  if #content == 0 then
    return false
  end
  store.tab_meta[tab_id] = sidebar.tab_meta(tab, content[1])
  local previous = util.active_tab(gui_window)
  local switching = previous and previous:tab_id() ~= tab_id
  if switching then
    tab:activate()
  end
  gui_window:perform_action(act.CloseCurrentTab { confirm = overlay == true }, content[1])
  if switching and not overlay and not defer then
    previous:activate()
  end
  return true
end

---Closes any tab and restores the previous one unless `defer` batches that. Never prompts: the
---confirmation belongs to the popover, so callers ask through `request_close`.
function M.close_tab(gui_window, tab_id, defer)
  return close_now(gui_window, tab_id, defer, false)
end

---WezTerm's own confirm overlay, for a sidebar that reported it cannot draw the question.
function M.close_with_overlay(gui_window, tab_id)
  return close_now(gui_window, tab_id, false, true)
end

---Closes every unpinned tab but `tab_id`, restoring the kept tab once rather than after each close.
function M.close_others(gui_window, tab_id)
  local previous = util.active_tab(gui_window)
  local previous_id = previous and previous:tab_id() or nil
  for _, id in ipairs(M.others(gui_window, tab_id)) do
    M.close_tab(gui_window, id, true)
  end
  restore_tab(gui_window, previous_id)
end

---A popover needs an expanded, authenticated sidebar to be drawn in; without one there is nothing
---to ask in, and wezterm's overlay survives a keyboard close.
-- Two six-cell items, their left margin and a border either side: below this the question cannot be
-- read, and an unreadable confirmation is worse than wezterm's own.
local CONFIRM_COLS = 15
local CONFIRM_ROWS = 5

local function can_confirm(gui_window)
  local tab = util.active_tab(gui_window)
  local sb = tab and sidebar.find(tab)
  if not sb or not sidebar.is_ready(sb) or state.is_collapsed(gui_window:window_id()) then
    return false
  end
  local d = mux.dims(sb)
  return type(d) == "table" and (d.cols or 0) >= CONFIRM_COLS and (d.viewport_rows or 0) >= CONFIRM_ROWS
end

---WezTerm's `CloseCurrentTab { confirm = true }` opens a per-tab overlay that the mouse-up after
---the click dismisses again, so the sidebar asks in a popover level of its own instead.
---@param from_key boolean|nil the pointer is not in the sidebar, so the question must take the pane
function M.request_close(gui_window, tab_id, anchor_row, anchor_col, from_key)
  if not M.needs_confirm(gui_window, tab_id, "close") then
    return M.close_tab(gui_window, tab_id)
  end
  if not can_confirm(gui_window) then
    return close_now(gui_window, tab_id, false, true)
  end
  local popover = require "vtabs.popover"
  popover.open(gui_window, tab_id, anchor_row or 0, anchor_col)
  popover.to_confirm(gui_window, "close")
  if from_key then
    popover.grab_focus(gui_window)
  end
  return false
end

function M.new_tab(gui_window, spawn)
  spawn = spawn or {}
  local mux_win = gui_window:mux_window()
  local current = active_content_pane(gui_window)
  if current and not spawn.domain then
    local domain = mux.domain(current)
    if domain then
      spawn.domain = { DomainName = domain }
    end
  end
  if current and spawn.cwd == nil then
    spawn.cwd = sidebar.tab_meta(mux_win:active_tab(), current).cwd
  end
  spawn.set_environment_variables = spawn.set_environment_variables or spawn_env(gui_window)
  local title = spawn.title
  spawn.title = nil
  local ok, tab, pane = pcall(function()
    return mux_win:spawn_tab(spawn)
  end)
  if not ok then
    util.warn("spawn_tab failed: %s", tostring(tab):match "^[^\n]*")
    return nil
  end
  if title then
    tab:set_title(title)
  end
  if not state.is_collapsed(gui_window:window_id()) then
    sidebar.attach(tab)
  end
  pane:activate()
  return tab
end

---The settings page's single owner: the strip button, the key binding and the popover item all
---call this, so none of them carries a spawn path or a "is one open already?" check of its own.
function M.open_settings(gui_window)
  return require("vtabs.settings").open(gui_window)
end

function M.reopen_closed(gui_window)
  local entry = state.pop_closed()
  if not entry then
    return
  end
  local tab = M.new_tab(gui_window, {
    cwd = entry.cwd,
    domain = entry.domain and { DomainName = entry.domain } or nil,
    title = entry.title,
  })
  if not tab and entry.domain then
    tab = M.new_tab(gui_window, { cwd = entry.cwd, title = entry.title })
  end
  if not tab then
    state.push_closed(entry)
    return
  end
  if entry.pinned then
    M.set_pinned(gui_window, tab:tab_id(), true)
  end
end

function M.new_window(gui_window, private)
  local cfg = config.get()
  local current = active_content_pane(gui_window)
  local spawn = {}
  if current then
    local domain = mux.domain(current)
    if domain then
      spawn.domain = { DomainName = domain }
    end
  end
  if private then
    spawn.set_environment_variables = cfg.private.env
  end
  local ok, tab, pane, mux_win = pcall(function()
    return wezterm.mux.spawn_window(spawn)
  end)
  if not ok then
    util.warn("spawn_window failed: %s", tostring(tab):match "^[^\n]*")
    return
  end
  if private then
    state.set_private(mux_win:window_id(), true)
  end
  sidebar.attach(tab)
  pane:activate()
end

---Moves a single-pane tab into a new window, keeping title, pin and private state.
function M.tear_off(gui_window, tab_id)
  local tab = M.tab_by_id(gui_window, tab_id)
  if not tab then
    return
  end
  local content = sidebar.classify(tab)
  if #content == 0 then
    return
  end
  if #content > 1 then
    mux.call(gui_window, "toast_notification", "vtabs", "move to new window needs a single pane", nil, 3000)
    return
  end
  local private = state.is_private(gui_window:window_id())
  local title = tab:get_title()
  local pinned = state.is_pinned(tab_id)
  store.moving[tab_id] = true
  local ok, new_tab, new_win = pcall(function()
    return content[1]:move_to_new_window()
  end)
  if not ok or not new_tab then
    util.warn("tear-off failed: %s", tostring(new_tab):match "^[^\n]*")
    store.moving[tab_id] = nil
    return
  end
  if private and new_win then
    state.set_private(new_win:window_id(), true)
  end
  if pinned then
    state.set_pinned(new_tab:tab_id(), true)
  end
  if title ~= "" then
    new_tab:set_title(title)
  end
  sidebar.attach(new_tab)
  if title ~= "" then
    new_tab:set_title(title)
  end
  sidebar.ensure(gui_window)
  return true
end

---Opens the popover's inline rename level; nothing here covers the content pane any more.
function M.rename_tab(gui_window, tab_id)
  if not M.tab_by_id(gui_window, tab_id) then
    return
  end
  local popover = require "vtabs.popover"
  local _, index = model.find(visible(gui_window), tab_id)
  popover.open(gui_window, tab_id, index or 0)
  popover.run(gui_window, "rename")
end

---A toggle changes the target width, and nothing else would drive the resize until the next poll.
---The fade brackets that single resize; the sidebar never slides.
local function resize_now(gui_window, collapsing)
  local geometry = require "vtabs.geometry"
  local view = require "vtabs.view"
  view.animate(gui_window, collapsing and "collapse_out" or "expand_out")
  view.apply_titlebar_band(gui_window)
  geometry.correct(gui_window)
  view.sync(gui_window, { force = true })
  view.animate(gui_window, collapsing and "collapse_in" or "expand_in")
end

function M.toggle_sidebar(gui_window)
  sidebar.toggle(gui_window)
  resize_now(gui_window, state.is_collapsed(gui_window:window_id()))
end

function M.show_sidebar(gui_window, shown)
  sidebar.set_collapsed(gui_window, not shown)
  resize_now(gui_window, not shown)
end

function M.focus_sidebar(gui_window)
  local wid = gui_window:window_id()
  local tab = util.active_tab(gui_window)
  local sb = tab and sidebar.find(tab)
  if not sb then
    return
  end
  local _, index = model.find(visible(gui_window), tab:tab_id())
  store.focus_index[wid] = index or 1
  state.set_focus(wid, true)
  sb:activate()
end

function M.blur_sidebar(gui_window)
  state.set_focus(gui_window:window_id(), false)
  local content = active_content_pane(gui_window)
  if content then
    content:activate()
  end
end

---Cycles through visible tabs only, wrapping at both ends.
function M.activate_relative(gui_window, delta)
  local items = visible(gui_window)
  if #items == 0 then
    return
  end
  local current = util.active_tab(gui_window)
  local _, index = model.find(items, current and current:tab_id())
  local target = ((index or 1) - 1 + delta) % #items + 1
  M.activate_tab(gui_window, items[target].tab_id)
end

---Activates the visible tab at a 0-based index; -1 selects the last one.
function M.activate_index(gui_window, index)
  local items = visible(gui_window)
  local item = index < 0 and items[#items + 1 + index] or items[index + 1]
  if item then
    M.activate_tab(gui_window, item.tab_id)
  end
end

function M.move_relative(gui_window, delta)
  local items = visible(gui_window)
  local current = util.active_tab(gui_window)
  local _, index = model.find(items, current and current:tab_id())
  if index then
    M.move_tab_to_slot(gui_window, current:tab_id(), math.max(1, math.min(index + delta, #items)))
  end
end

-- WezTerm's own `SplitPane` names them Top/Bottom; Up/Down is what everyone types.
local SPLIT = { Right = "Right", Left = "Left", Top = "Top", Bottom = "Bottom", Up = "Top", Down = "Bottom" }

---Splits the tab's content pane, never the sidebar. WezTerm's own `SplitPane` acts on whichever
---pane is active, which under `hover = "follow"` is the sidebar whenever the pointer is over it.
function M.split(gui_window, direction)
  local where = SPLIT[direction]
  if not where then
    util.warn("split direction must be Right, Left, Top/Up or Bottom/Down, got %s", tostring(direction))
    return nil
  end
  local content = active_content_pane(gui_window)
  if not content then
    return nil
  end
  local ok, pane = pcall(function()
    return content:split { direction = where }
  end)
  if not ok or not pane then
    util.warn("split failed: %s", tostring(pane):match "^[^\n]*")
    return nil
  end
  pane:activate()
  return pane
end

function M.activate_pane_direction(gui_window, direction)
  local content = active_content_pane(gui_window)
  if not content then
    return
  end
  local target = mux.call(mux.tab_of(content), "get_pane_direction", direction)
  if target and not sidebar.is_backend(target) then
    target:activate()
  end
end

local function current_tab_id(gui_window)
  local tab = util.active_tab(gui_window)
  return tab and tab:tab_id() or nil
end

local function callback(fn)
  return wezterm.action_callback(function(window, pane)
    local ok, err = pcall(fn, window, pane)
    if not ok then
      util.warn("action failed: %s", tostring(err))
    end
  end)
end

local function on_current_tab(fn)
  return callback(function(window)
    local id = current_tab_id(window)
    if id then
      fn(window, id)
    end
  end)
end

---One row per named behaviour, and the only place a name becomes one. `run(window, tab_id)` is the
---whole contract; `needs = "tab"` means a caller holding no tab resolves the current one first.
---Everything that dispatches by name reads this table -- the key bindings, the strip buttons and the
---popover items -- so a name cannot mean one thing in one of them and nothing in another, which is
---how the strip's ⚙ came to be painted by default and do nothing when clicked.
M.dispatch = {
  toggle_sidebar = { run = M.toggle_sidebar, label = "Toggle sidebar" },
  focus_sidebar = { run = M.focus_sidebar, label = "Focus sidebar" },
  new_tab = {
    run = function(window)
      M.new_tab(window)
    end,
    label = "New tab",
  },
  new_window = {
    run = function(window)
      M.new_window(window, false)
    end,
    label = "New window",
  },
  private_window = {
    run = function(window)
      M.new_window(window, true)
    end,
    label = "New private window",
  },
  reopen_closed = { run = M.reopen_closed, label = "Reopen closed tab" },
  open_settings = { run = M.open_settings, label = "Settings…" },
  next_tab = {
    run = function(window)
      M.activate_relative(window, 1)
    end,
    label = "Next tab",
  },
  prev_tab = {
    run = function(window)
      M.activate_relative(window, -1)
    end,
    label = "Previous tab",
  },
  move_tab_up = {
    run = function(window)
      M.move_relative(window, -1)
    end,
    label = "Move tab up",
  },
  move_tab_down = {
    run = function(window)
      M.move_relative(window, 1)
    end,
    label = "Move tab down",
  },
  activate_tab = { run = M.activate_tab, needs = "tab", label = "Switch to tab" },
  pin_tab = { run = M.toggle_pin, needs = "tab", label = "Pin tab" },
  tear_off = { run = M.tear_off, needs = "tab", label = "Move to new window" },
  rename_tab = { run = M.rename_tab, needs = "tab", label = "Rename…" },
  close_tab = {
    run = function(window, id)
      M.request_close(window, id, nil, nil, true)
    end,
    needs = "tab",
    label = "Close tab",
  },
  -- `_now` skips the confirmation its caller has already resolved; `close_tab` is the one that asks.
  -- `internal` keeps them off the key-binding surface: only a caller that already asked may use them.
  close_tab_now = { run = M.close_tab, needs = "tab", label = "Close tab", internal = true },
  close_others_now = { run = M.close_others, needs = "tab", label = "Close other tabs", internal = true },
}

---Names the user may write that are not the behaviour's own: `settings` is what the strip button and
---the key binding are called.
local ALIASES = { settings = "open_settings", toggle = "toggle_sidebar" }

---The behaviour name a user-facing name stands for.
function M.canonical(name)
  return ALIASES[name] or name
end

---The dispatch row for a name, or nil when nothing answers to it.
function M.resolve(name)
  if type(name) ~= "string" then
    return nil
  end
  return M.dispatch[ALIASES[name] or name]
end

---Runs a named behaviour. `tab_id` is optional: a row that needs one and is not given one takes the
---window's current tab, and does nothing when there is none.
function M.run(name, window, tab_id)
  local row = M.resolve(name)
  if not row then
    return false
  end
  if row.needs ~= "tab" then
    row.run(window)
    return true
  end
  local id = tab_id or current_tab_id(window)
  if not id then
    return false
  end
  row.run(window, id)
  return true
end

---The strip's buttons, in default order. `hooked` marks the ones a hook may point elsewhere, and an
---id with no `action` has no built-in behaviour at all, so it is only drawn when a hook answers it.
M.strip = {
  { id = "toggle", action = "toggle_sidebar", default = true },
  { id = "new_tab", action = "new_tab", default = true },
  { id = "settings", action = "open_settings", default = true, hooked = true },
  { id = "search", hooked = true },
}

M.action = {}
for name, row in pairs(M.dispatch) do
  if not row.internal then
    M.action[name] = row.needs == "tab" and on_current_tab(row.run) or callback(row.run)
  end
end

-- Parametrised: a name plus an argument, so they are factories rather than behaviours to dispatch.
M.action.activate_tab = function(index)
  return callback(function(window)
    M.activate_index(window, index)
  end)
end
M.action.activate_pane_direction = function(direction)
  return callback(function(window)
    M.activate_pane_direction(window, direction)
  end)
end
M.action.split = function(direction)
  return callback(function(window)
    M.split(window, direction)
  end)
end

return M

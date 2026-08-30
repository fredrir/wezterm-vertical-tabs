local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local hit = require "vtabs.hit"
local util = require "vtabs.util"

local M = {}

local session = state.session

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
function M.activate_tab(gui_window, tab_id, focus)
  local tab = M.tab_by_id(gui_window, tab_id)
  if not tab then
    return nil
  end
  tab:activate()
  local target = (focus == "sidebar" and sidebar.find(tab)) or sidebar.content_pane(tab)
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
  local skip = util.try(function()
    return gui_window:effective_config().skip_close_confirmation_for_processes_named
  end) or {}
  for _, p in ipairs(panes) do
    local name = util.basename(util.try(function()
      return p:get_foreground_process_name()
    end))
    if not name or not util.contains(skip, name) then
      return true
    end
  end
  return false
end

---Closes any tab, restoring the previous one unless `defer` batches that or a prompt is showing.
---Returns true when WezTerm is now showing a close prompt.
function M.close_tab(gui_window, tab_id, defer)
  local tab = M.tab_by_id(gui_window, tab_id)
  local content = tab and sidebar.classify(tab) or {}
  if #content == 0 then
    return false
  end
  session.tab_meta[tab_id] = sidebar.tab_meta(tab, content[1])
  local confirm = config.get().confirm_close ~= false and needs_prompt(gui_window, content)
  local previous = util.active_tab(gui_window)
  local switching = previous and previous:tab_id() ~= tab_id
  if switching then
    tab:activate()
  end
  gui_window:perform_action(act.CloseCurrentTab { confirm = confirm }, content[1])
  if switching and not confirm and not defer then
    previous:activate()
  end
  return confirm
end

function M.close_others(gui_window, tab_id)
  local previous = util.active_tab(gui_window)
  local previous_id = previous and previous:tab_id() or nil
  local prompted = false
  for _, item in ipairs(visible(gui_window)) do
    if item.tab_id ~= tab_id and not item.is_pinned then
      prompted = M.close_tab(gui_window, item.tab_id, true) or prompted
    end
  end
  if not prompted then
    restore_tab(gui_window, previous_id)
  end
end

function M.new_tab(gui_window, spawn)
  spawn = spawn or {}
  local mux_win = gui_window:mux_window()
  local current = active_content_pane(gui_window)
  if current and not spawn.domain then
    local domain = util.try(function()
      return current:get_domain_name()
    end)
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
    local domain = util.try(function()
      return current:get_domain_name()
    end)
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
    util.try(function()
      gui_window:toast_notification("vtabs", "move to new window needs a single pane", nil, 3000)
    end)
    return
  end
  local private = state.is_private(gui_window:window_id())
  local title = tab:get_title()
  local pinned = state.is_pinned(tab_id)
  session.moving[tab_id] = true
  local ok, new_tab, new_win = pcall(function()
    return content[1]:move_to_new_window()
  end)
  if not ok or not new_tab then
    util.warn("tear-off failed: %s", tostring(new_tab):match "^[^\n]*")
    session.moving[tab_id] = nil
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

function M.rename_tab(gui_window, tab_id)
  local tab = M.tab_by_id(gui_window, tab_id)
  local content = active_content_pane(gui_window)
  if not tab or not content then
    return
  end
  gui_window:perform_action(
    act.PromptInputLine {
      description = "Rename tab",
      initial_value = tab:get_title(),
      action = wezterm.action_callback(function(_, _, line)
        if line then
          tab:set_title(line)
        end
      end),
    },
    content
  )
end

function M.toggle_sidebar(gui_window)
  sidebar.toggle(gui_window)
end

function M.show_sidebar(gui_window, shown)
  sidebar.set_collapsed(gui_window, not shown)
end

function M.focus_sidebar(gui_window)
  local wid = gui_window:window_id()
  local tab = util.active_tab(gui_window)
  local sb = tab and sidebar.find(tab)
  if not sb then
    return
  end
  local _, index = model.find(visible(gui_window), tab:tab_id())
  session.focus_index[wid] = index or 1
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

function M.activate_pane_direction(gui_window, direction)
  local content = active_content_pane(gui_window)
  if not content then
    return
  end
  local target = util.try(function()
    return content:tab():get_pane_direction(direction)
  end)
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

M.action = {
  toggle_sidebar = callback(M.toggle_sidebar),
  focus_sidebar = callback(M.focus_sidebar),
  new_tab = callback(function(window)
    M.new_tab(window)
  end),
  close_tab = on_current_tab(M.close_tab),
  reopen_closed = callback(M.reopen_closed),
  pin_tab = on_current_tab(M.toggle_pin),
  private_window = callback(function(window)
    M.new_window(window, true)
  end),
  new_window = callback(function(window)
    M.new_window(window, false)
  end),
  tear_off = on_current_tab(M.tear_off),
  rename_tab = on_current_tab(M.rename_tab),
  next_tab = callback(function(window)
    M.activate_relative(window, 1)
  end),
  prev_tab = callback(function(window)
    M.activate_relative(window, -1)
  end),
  move_tab_up = callback(function(window)
    M.move_relative(window, -1)
  end),
  move_tab_down = callback(function(window)
    M.move_relative(window, 1)
  end),
  activate_tab = function(index)
    return callback(function(window)
      M.activate_index(window, index)
    end)
  end,
  activate_pane_direction = function(direction)
    return callback(function(window)
      M.activate_pane_direction(window, direction)
    end)
  end,
}

return M

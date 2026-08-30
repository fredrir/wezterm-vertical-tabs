local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local util = require "vtabs.util"

local M = {}

local function tab_by_id(gui_window, tab_id)
  for _, info in ipairs(gui_window:mux_window():tabs_with_info()) do
    if info.tab:tab_id() == tab_id then
      return info.tab, info.index
    end
  end
  return nil
end

local function active_content_pane(gui_window)
  local tab = gui_window:mux_window():active_tab()
  return tab and sidebar.content_pane(tab) or nil
end

local function spawn_env(gui_window)
  local cfg = config.get()
  if state.is_private(gui_window:window_id()) then
    return cfg.private.env
  end
  return nil
end

function M.activate_tab(gui_window, tab_id)
  local tab = tab_by_id(gui_window, tab_id)
  if tab then
    tab:activate()
    local content = sidebar.content_pane(tab)
    if content then
      content:activate()
    end
    if config.get().debug then
      local active = gui_window:mux_window():active_tab()
      util.log("activate_tab %s -> active now %s", tostring(tab_id), tostring(active and active:tab_id()))
    end
  end
end

---Moves a tab to a physical index (0-based) without leaving it active unless it already was.
function M.move_tab_to_index(gui_window, tab_id, index)
  local tab, current = tab_by_id(gui_window, tab_id)
  if not tab or current == index then
    return
  end
  local previous = gui_window:mux_window():active_tab()
  local content = sidebar.content_pane(tab)
  if not content then
    return
  end
  tab:activate()
  gui_window:perform_action(act.MoveTab(index), content)
  if previous and previous:tab_id() ~= tab_id then
    previous:activate()
  end
end

local function desired_order(gui_window)
  return model.ordered(model.build(gui_window))
end

---Physically reorders tabs so the pinned block comes first.
function M.normalize_order(gui_window)
  local order = desired_order(gui_window)
  for i, item in ipairs(order) do
    M.move_tab_to_index(gui_window, item.tab_id, i - 1)
  end
end

function M.set_pinned(gui_window, tab_id, pinned)
  state.set_pinned(tab_id, pinned)
  M.normalize_order(gui_window)
end

function M.toggle_pin(gui_window, tab_id)
  M.set_pinned(gui_window, tab_id, not state.is_pinned(tab_id))
end

---Moves a tab to a slot in the rendered (pinned-first) order, pinning or unpinning as it crosses the separator.
function M.move_tab_to_slot(gui_window, tab_id, slot)
  local items = model.build(gui_window)
  local pinned_count = 0
  for _, item in ipairs(items) do
    if item.is_pinned and item.tab_id ~= tab_id then
      pinned_count = pinned_count + 1
    end
  end
  local should_pin = slot <= pinned_count
  if state.is_pinned(tab_id) ~= should_pin then
    state.set_pinned(tab_id, should_pin)
  end
  local order = {}
  for _, item in ipairs(model.ordered(model.build(gui_window))) do
    if item.tab_id ~= tab_id then
      order[#order + 1] = item.tab_id
    end
  end
  table.insert(order, math.max(1, math.min(slot, #order + 1)), tab_id)
  for i, id in ipairs(order) do
    M.move_tab_to_index(gui_window, id, i - 1)
  end
end

---Closes a tab that may not be active: activate, close, then restore the previous tab.
function M.close_tab(gui_window, tab_id, confirm)
  local tab = tab_by_id(gui_window, tab_id)
  if not tab then
    return
  end
  local content = sidebar.content_pane(tab)
  if not content then
    return
  end
  state.session.tab_meta[tab_id] = sidebar.tab_meta(tab, content)
  if confirm == nil then
    confirm = config.get().confirm_close ~= false
  end
  local previous = gui_window:mux_window():active_tab()
  local switching = previous and previous:tab_id() ~= tab_id
  if switching then
    tab:activate()
  end
  gui_window:perform_action(act.CloseCurrentTab { confirm = confirm }, content)
  if switching and not confirm then
    previous:activate()
  end
end

function M.close_others(gui_window, tab_id)
  for _, info in ipairs(gui_window:mux_window():tabs_with_info()) do
    local id = info.tab:tab_id()
    if id ~= tab_id and not state.is_pinned(id) then
      M.close_tab(gui_window, id)
    end
  end
end

function M.new_tab(gui_window, spawn)
  spawn = spawn or {}
  local mux_win = gui_window:mux_window()
  local current = active_content_pane(gui_window)
  if current and not spawn.domain then
    local ok, domain = pcall(function()
      return current:get_domain_name()
    end)
    if ok and domain then
      spawn.domain = { DomainName = domain }
    end
  end
  if current and spawn.cwd == nil then
    spawn.cwd = sidebar.tab_meta(mux_win:active_tab(), current).cwd
  end
  spawn.set_environment_variables = spawn.set_environment_variables or spawn_env(gui_window)
  local ok, tab, pane = pcall(function()
    return mux_win:spawn_tab(spawn)
  end)
  if not ok then
    util.warn("spawn_tab failed: %s", tostring(tab))
    return nil
  end
  if spawn.title then
    tab:set_title(spawn.title)
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
  if tab and entry.pinned then
    M.set_pinned(gui_window, tab:tab_id(), true)
  end
end

function M.new_window(gui_window, private)
  local cfg = config.get()
  local current = active_content_pane(gui_window)
  local spawn = {}
  if current then
    local ok, domain = pcall(function()
      return current:get_domain_name()
    end)
    if ok and domain then
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
    util.warn("spawn_window failed: %s", tostring(tab))
    return
  end
  if private then
    state.set_private(mux_win:window_id(), true)
  end
  sidebar.attach(tab)
  pane:activate()
end

---Moves the tab's panes into a new window; extra panes are re-split via the wezterm CLI.
function M.tear_off(gui_window, tab_id)
  local tab = tab_by_id(gui_window, tab_id)
  if not tab then
    return
  end
  local content = sidebar.classify(tab)
  if #content == 0 then
    return
  end
  local private = state.is_private(gui_window:window_id())
  local title = tab:get_title()
  state.session.moving = state.session.moving or {}
  state.session.moving[tab_id] = true
  local ok, new_tab, new_win = pcall(function()
    return content[1]:move_to_new_window()
  end)
  if not ok then
    util.warn("tear-off failed: %s", tostring(new_tab))
    state.session.moving[tab_id] = nil
    return
  end
  local anchor = content[1]:pane_id()
  for i = 2, #content do
    local direction = i % 2 == 0 and "--right" or "--bottom"
    wezterm.run_child_process {
      "wezterm",
      "cli",
      "split-pane",
      direction,
      "--pane-id",
      tostring(anchor),
      "--move-pane-id",
      tostring(content[i]:pane_id()),
    }
  end
  if new_tab and new_win then
    if private then
      state.set_private(new_win:window_id(), true)
    end
    if state.is_pinned(tab_id) then
      state.set_pinned(new_tab:tab_id(), true)
    end
    if title ~= "" then
      new_tab:set_title(title)
    end
    sidebar.attach(new_tab)
    content[1]:activate()
  end
  sidebar.ensure(gui_window)
end

function M.rename_tab(gui_window, tab_id)
  local tab = tab_by_id(gui_window, tab_id)
  local content = tab and sidebar.content_pane(tab)
  if not content then
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

function M.focus_sidebar(gui_window)
  local wid = gui_window:window_id()
  local tab = gui_window:mux_window():active_tab()
  local sb = tab and sidebar.find(tab)
  if not sb then
    return
  end
  local items = model.ordered(model.build(gui_window))
  local _, index = model.find(items, tab:tab_id())
  state.session.focus_index = state.session.focus_index or {}
  state.session.focus_index[wid] = index or 1
  state.set_focus(wid, true)
  sb:activate()
end

function M.blur_sidebar(gui_window)
  local wid = gui_window:window_id()
  state.set_focus(wid, false)
  local content = active_content_pane(gui_window)
  if content then
    content:activate()
  end
end

function M.activate_relative(gui_window, delta)
  local content = active_content_pane(gui_window)
  if content then
    gui_window:perform_action(act.ActivateTabRelative(delta), content)
  end
end

function M.move_relative(gui_window, delta)
  local content = active_content_pane(gui_window)
  if content then
    gui_window:perform_action(act.MoveTabRelative(delta), content)
  end
end

function M.activate_index(gui_window, index)
  local content = active_content_pane(gui_window)
  if content then
    gui_window:perform_action(act.ActivateTab(index), content)
  end
end

function M.activate_pane_direction(gui_window, direction)
  local content = active_content_pane(gui_window)
  if not content then
    return
  end
  local tab = content:tab()
  local ok, target = pcall(function()
    return tab:get_pane_direction(direction)
  end)
  if ok and target and not sidebar.is_sidebar(target) then
    target:activate()
  end
end

local function current_tab_id(gui_window)
  local tab = gui_window:mux_window():active_tab()
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

M.action = {
  toggle_sidebar = callback(M.toggle_sidebar),
  focus_sidebar = callback(M.focus_sidebar),
  new_tab = callback(function(window)
    M.new_tab(window)
  end),
  close_tab = callback(function(window)
    local id = current_tab_id(window)
    if id then
      M.close_tab(window, id)
    end
  end),
  reopen_closed = callback(M.reopen_closed),
  pin_tab = callback(function(window)
    local id = current_tab_id(window)
    if id then
      M.toggle_pin(window, id)
    end
  end),
  private_window = callback(function(window)
    M.new_window(window, true)
  end),
  new_window = callback(function(window)
    M.new_window(window, false)
  end),
  tear_off = callback(function(window)
    local id = current_tab_id(window)
    if id then
      M.tear_off(window, id)
    end
  end),
  rename_tab = callback(function(window)
    local id = current_tab_id(window)
    if id then
      M.rename_tab(window, id)
    end
  end),
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
}

function M.action.activate_tab(index)
  return callback(function(window)
    M.activate_index(window, index)
  end)
end

function M.action.activate_pane_direction(direction)
  return callback(function(window)
    M.activate_pane_direction(window, direction)
  end)
end

return M

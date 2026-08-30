-- Minimal in-memory stand-ins for WezTerm's mux objects, enough to drive sidebar/actions.
-- luacheck: ignore 212
local M = {}

local next_id = { pane = 0, tab = 0, window = 0 }
local function alloc(kind)
  local id = next_id[kind]
  next_id[kind] = id + 1
  return id
end

local Pane = {}
Pane.__index = Pane

function M.pane(tab, opts)
  opts = opts or {}
  local p = setmetatable({
    id = alloc "pane",
    _tab = tab,
    vars = opts.vars or {},
    process = opts.process or "/bin/zsh",
    domain = opts.domain or "local",
    sent = {},
    pasted = {},
    title = opts.title or "zsh",
    cols = opts.cols or 80,
  }, Pane)
  return p
end

function Pane:pane_id()
  return self.id
end
function Pane:tab()
  return self._tab
end
function Pane:get_user_vars()
  return self.vars
end
function Pane:get_domain_name()
  return self.domain
end
function Pane:get_foreground_process_name()
  -- mux panes never report one; mux/src/pane.rs:331-333 returns None.
  return self.domain == "local" and self.process or nil
end
function Pane:get_title()
  return self.title
end
function Pane:get_current_working_dir()
  return { file_path = "/tmp" }
end
function Pane:has_unseen_output()
  return false
end
function Pane:get_dimensions()
  local cell = self.cell_width or 10
  return { cols = self.cols, viewport_rows = 24, pixel_width = self.cols * cell, dpi = self.dpi or 96 }
end
function Pane:send_text(text)
  self.sent[#self.sent + 1] = text
  local title = text:match "\27%]0;(.-)\7" or text:match "\27%]2;(.-)\7"
  if title then
    self.title = title
  end
end
function Pane:paste(text)
  self.pasted[#self.pasted + 1] = text
end
function Pane:activate()
  self._tab.active = self
  self._tab._window.active_tab_ref = self._tab
end
function Pane:split(args)
  local tab = self._tab
  local size = args.size or math.floor(tab:width() / 2)
  local sb = M.pane(tab, { process = args.args and args.args[1] or "sh", cols = size })
  sb.split_args = args
  if args.direction == "Right" or args.direction == "Bottom" then
    tab.pane_list[#tab.pane_list + 1] = sb
    tab:set_split(tab:width() - size - 1)
  else
    table.insert(tab.pane_list, 1, sb)
    tab:set_split(size)
  end
  tab.active = sb
  return sb
end
function Pane:move_to_new_window()
  local win = M.window()
  local tab = win:add_tab { existing = self }
  return tab, win
end

local Tab = {}
Tab.__index = Tab

function Tab:tab_id()
  return self.id
end
function Tab:panes()
  return self.pane_list
end
function Tab:active_pane()
  return self.active
end
function Tab:get_title()
  return self.title
end
function Tab:set_title(t)
  self.title = t
end
function Tab:activate()
  self._window.active_tab_ref = self
end
function Tab:window()
  return self._window
end
function Tab:width()
  return self._window.cols
end

---The fake models one top-level horizontal split: leaf 1 is `first`, every other leaf shares `second`.
local function second_leaves(tab)
  local rest = {}
  for i = 2, #tab.pane_list do
    rest[#rest + 1] = tab.pane_list[i]
  end
  return rest
end

function Tab:set_split(first_cols)
  local rest = second_leaves(self)
  if #rest == 0 then
    self.pane_list[1].cols = self:width()
    return
  end
  self.pane_list[1].cols = first_cols
  for _, p in ipairs(rest) do
    p.cols = self:width() - first_cols - 1
  end
end

---True when the root split is the nearest horizontal node above the active leaf, which is what
---`adjust_pane_size` walks up to. Only leaf 1 and the two-leaf shape can prove that here.
function Tab:root_split_holds_active()
  return #self.pane_list == 2 or self.active == self.pane_list[1]
end

---Pane rectangles and zoom state, left to right across the one modelled horizontal split.
function Tab:panes_with_info()
  local out = {}
  local first = self.pane_list[1]
  for i, p in ipairs(self.pane_list) do
    out[i] = {
      index = i - 1,
      is_active = p == self.active,
      is_zoomed = p.zoomed == true,
      left = i == 1 and 0 or first.cols + 1,
      top = 0,
      width = p.cols,
      height = 24,
      pane = p,
    }
  end
  return out
end

---Mirrors `mux/src/tab.rs adjust_x_size`: one column at a time, alternating first and second.
function Tab:adjust_x_size(delta)
  local rest = second_leaves(self)
  if #rest == 0 then
    self.pane_list[1].cols = math.max(1, self.pane_list[1].cols + delta)
    return
  end
  local first_cols, second_cols = self.pane_list[1].cols, rest[1].cols
  while delta ~= 0 do
    local moved = false
    if delta > 0 then
      first_cols, delta, moved = first_cols + 1, delta - 1, true
      if delta > 0 then
        second_cols, delta = second_cols + 1, delta - 1
      end
    else
      if first_cols > 1 then
        first_cols, delta, moved = first_cols - 1, delta + 1, true
      end
      if delta < 0 and second_cols > 1 then
        second_cols, delta, moved = second_cols - 1, delta + 1, true
      end
    end
    if not moved then
      break
    end
  end
  self.pane_list[1].cols = first_cols
  for _, p in ipairs(rest) do
    p.cols = second_cols
  end
end

local Window = {}
Window.__index = Window

function M.window(cols)
  local w = setmetatable({ id = alloc "window", tab_list = {}, actions = {}, cols = cols or 80 }, Window)
  w.gui = M.gui(w)
  return w
end

---Window resize: every tab's split absorbs the delta the way `adjust_x_size` deals it out.
function Window:resize(dcols)
  self.cols = self.cols + dcols
  for _, tab in ipairs(self.tab_list) do
    tab:adjust_x_size(dcols)
  end
end

function Window:add_tab(opts)
  opts = opts or {}
  local tab = setmetatable({ id = alloc "tab", pane_list = {}, title = opts.title or "", _window = self }, Tab)
  local pane = opts.existing or M.pane(tab, opts)
  pane._tab = tab
  pane.cols = opts.cols or self.cols
  tab.pane_list[1] = pane
  tab.active = pane
  self.tab_list[#self.tab_list + 1] = tab
  self.active_tab_ref = self.active_tab_ref or tab
  return tab
end

function Window:remove_tab(tab)
  for i, t in ipairs(self.tab_list) do
    if t == tab then
      table.remove(self.tab_list, i)
    end
  end
  if self.active_tab_ref == tab then
    self.active_tab_ref = self.tab_list[1]
  end
end

---A GUI reconnect to a surviving mux: panes and titles live on, user vars start empty.
function Window:reattach()
  for _, tab in ipairs(self.tab_list) do
    for _, pane in ipairs(tab.pane_list) do
      pane.vars = {}
    end
  end
end

function Window:window_id()
  return self.id
end
function Window:tabs_with_info()
  local out = {}
  for i, tab in ipairs(self.tab_list) do
    out[i] = { index = i - 1, is_active = tab == self.active_tab_ref, tab = tab }
  end
  return out
end
function Window:active_tab()
  return self.active_tab_ref
end
function Window:gui_window()
  return self.gui
end
function Window:spawn_tab(spawn)
  local tab = self:add_tab { process = "/bin/zsh" }
  tab.spawn = spawn
  return tab, tab.pane_list[1], self
end

local Gui = {}
Gui.__index = Gui

function M.gui(window)
  return setmetatable({ _mux = window }, Gui)
end

function Gui:mux_window()
  return self._mux
end
function Gui:window_id()
  return self._mux.id
end
-- Catppuccin Mocha, the shape wezterm hands back in effective_config().resolved_palette.
M.palette = {
  background = "#1e1e2e",
  foreground = "#cdd6f4",
  cursor_bg = "#f5e0dc",
  cursor_fg = "#1e1e2e",
  selection_bg = "#585b70",
  ansi = { "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de" },
  brights = { "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8" },
}

function Gui:effective_config()
  return {
    resolved_palette = M.palette,
    skip_close_confirmation_for_processes_named = { "zsh", "wez-vtabs" },
  }
end
function Gui:get_dimensions()
  return { pixel_width = self._mux.cols * 10, pixel_height = 600 }
end
function Gui:set_inner_size() end
function Gui:toast_notification() end
function Gui:perform_action(action, pane)
  self._mux.actions[#self._mux.actions + 1] = { action = action, pane = pane }
  local name = action.action
  if name == "CloseCurrentTab" then
    self._mux:remove_tab(self._mux.active_tab_ref)
  elseif name == "CloseCurrentPane" then
    -- Mirrors WezTerm: the pane argument is ignored, the active pane of the active tab closes.
    local tab = self._mux.active_tab_ref
    local victim = tab.active
    for i, p in ipairs(tab.pane_list) do
      if p == victim then
        table.remove(tab.pane_list, i)
      end
    end
    tab.active = tab.pane_list[1]
    if #tab.pane_list == 0 then
      self._mux:remove_tab(tab)
    end
  elseif name == "AdjustPaneSize" then
    local tab = self._mux.active_tab_ref
    local dir, amount = action.arg[1], action.arg[2]
    if (dir == "Left" or dir == "Right") and tab:root_split_holds_active() then
      local delta = dir == "Right" and amount or -amount
      local width = tab:width()
      tab:set_split(math.max(1, math.min(tab.pane_list[1].cols + delta, width - 2)))
    end
  elseif name == "MoveTab" then
    local tab = self._mux.active_tab_ref
    self._mux:remove_tab(tab)
    table.insert(self._mux.tab_list, action.arg + 1, tab)
    self._mux.active_tab_ref = tab
  end
end

return M

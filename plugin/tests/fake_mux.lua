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
  return self.process
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
  return { cols = self.cols, viewport_rows = 24 }
end
function Pane:send_text(text)
  self.sent[#self.sent + 1] = text
end
function Pane:activate()
  self._tab.active = self
  self._tab._window.active_tab_ref = self._tab
end
function Pane:split(args)
  local sb = M.pane(self._tab, { process = args.args and args.args[1] or "sh", cols = args.size })
  sb.split_args = args
  table.insert(self._tab.pane_list, 1, sb)
  self._tab.active = sb
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

local Window = {}
Window.__index = Window

function M.window()
  local w = setmetatable({ id = alloc "window", tab_list = {}, actions = {} }, Window)
  w.gui = M.gui(w)
  return w
end

function Window:add_tab(opts)
  opts = opts or {}
  local tab = setmetatable({ id = alloc "tab", pane_list = {}, title = opts.title or "", _window = self }, Tab)
  local pane = opts.existing or M.pane(tab, opts)
  pane._tab = tab
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
  return { pixel_width = 800, pixel_height = 600 }
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
  elseif name == "MoveTab" then
    local tab = self._mux.active_tab_ref
    self._mux:remove_tab(tab)
    table.insert(self._mux.tab_list, action.arg + 1, tab)
    self._mux.active_tab_ref = tab
  end
end

return M

-- Minimal in-memory stand-ins for WezTerm's mux objects, enough to drive sidebar/actions.
-- luacheck: ignore 212
local async = require "support.async"
local protocol = require "vtabs.gen.protocol"

local M = {}

-- When true, split/spawn/move yield before touching the tree, where a real mux leaves it mid-await.
M.deferred = false
local function await(tag)
  if M.deferred then
    async.yield(tag)
  end
end

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
  return p:register()
end

function Pane:pane_id()
  return self.id
end
---`wezterm.mux.get_pane(id)` has to resolve, the way it does against a real mux.
function Pane:register()
  local wezterm = require "wezterm"
  wezterm.panes[self.id] = self
  return self
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
  if self.cwd == false then
    return nil
  end
  return self.cwd or { file_path = "/tmp" }
end
function Pane:has_unseen_output()
  return false
end
function Pane:get_dimensions()
  local cell = self.cell_width or 10
  return {
    cols = self.cols,
    viewport_rows = 24,
    pixel_width = self.cols * cell,
    pixel_height = 24 * 20,
    dpi = self.dpi or 96,
  }
end
---Panes of `tab` inside the sidebar band, the way the backend's `rescue_plan` sees them.
local function intruders_in(tab, own, band)
  local out = {}
  for _, info in ipairs(tab:panes_with_info()) do
    if info.pane ~= own and info.left <= band then
      out[#out + 1] = info.pane
    end
  end
  return out
end

---What the real backend does with one authenticated framed payload; a `hung` pane hears nothing,
---like a dead one. `quit` exits, and an exited pane closes; `kill` and `rescue` run the CLI.
local function obey_payload(pane, frame_token, payload)
  local verb = payload:match '"t":"(%w+)"'
  if not verb then
    return
  end
  local active = pane.control_token or pane.vars.vtabs_token
  if verb == "auth" then
    local claimed = payload:match '"token":"([^"]+)"'
    if claimed and ((active == nil and claimed == frame_token) or active == frame_token) then
      -- The real echo arrives asynchronously through a user var; keep transport state separate so
      -- lifecycle tests do not accidentally gain trust merely because `send_text` returned.
      pane.control_token = claimed
    end
  elseif active ~= frame_token then
    return
  elseif verb == "quit" then
    M.kill_pane(pane)
  elseif verb == "kill" then
    -- the server lists every pane it holds: here, every pane of this window's tabs
    local title = payload:match '"title":"([^"]*)"'
    for _, tab in ipairs(pane._tab._window.tab_list) do
      for _, p in ipairs { table.unpack(tab.pane_list) } do
        if p.title == title then
          M.kill_pane(p)
          pane.killed = (pane.killed or 0) + 1
        end
      end
    end
  elseif verb == "rescue" then
    local band = tonumber(payload:match '"band":(%d+)') or 0
    local tab = pane._tab
    for _, moved in ipairs(intruders_in(tab, pane, band)) do
      local host = nil
      for _, p in ipairs(tab.pane_list) do
        if p ~= pane and p ~= moved and not tostring(p.title):find "^wez%-vtabs" then
          host = p
          break
        end
      end
      if host then
        M.cli {
          "wezterm",
          "cli",
          "--no-auto-start",
          "split-pane",
          "--move-pane-id",
          tostring(moved.id),
          "--pane-id",
          tostring(host.id),
        }
        moved.left = nil
        pane.moved = (pane.moved or 0) + 1
      end
    end
  end
end

local function obey(pane, text)
  if pane.hung then
    return
  end
  for line in text:gmatch "[^\n]+" do
    if line:sub(1, #protocol.CONTROL_PREFIX) == protocol.CONTROL_PREFIX then
      local separator = line:find(" ", #protocol.CONTROL_PREFIX + 1, true)
      local token = separator and line:sub(#protocol.CONTROL_PREFIX + 1, separator - 1) or nil
      if token and token ~= "" then
        obey_payload(pane, token, line:sub(separator + 1))
      end
    end
  end
end

function Pane:send_text(text)
  self.send_attempts = (self.send_attempts or 0) + 1
  self.last_send_attempt = text
  if self.fail_send then
    error "injected send failure"
  end
  self.sent[#self.sent + 1] = text
  local title = text:match "\27%]0;(.-)\7" or text:match "\27%]2;(.-)\7"
  if title then
    self.title = title
  end
  obey(self, text)
end
function Pane:paste(text)
  self.pasted[#self.pasted + 1] = text
end
function Pane:activate()
  self._tab.active = self
  self._tab._window.active_tab_ref = self._tab
end
function Pane:split(args)
  await "split"
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
  await "move"
  local cols = self._tab:width()
  M.detach_pane(self)
  local win = M.window(cols)
  local tab = win:add_tab { existing = self }
  return tab, win
end

---Takes a pane out of its tab; an emptied tab leaves its window, as the mux does.
function M.detach_pane(pane)
  local tab = pane._tab
  for i, p in ipairs(tab.pane_list) do
    if p == pane then
      table.remove(tab.pane_list, i)
      break
    end
  end
  if #tab.pane_list == 0 then
    tab._window:remove_tab(tab)
  elseif tab.active == pane then
    tab.active = tab.pane_list[1]
  end
end

function M.kill_pane(pane)
  M.detach_pane(pane)
  require("wezterm").panes[pane.id] = nil
end

local Tab = {}
Tab.__index = Tab

function Tab:tab_id()
  return self.id
end
function Tab:panes()
  return self.pane_list
end
---A pane that left the tab cannot stay active; the mux moves focus to a surviving one.
function Tab:active_pane()
  for _, p in ipairs(self.pane_list) do
    if p == self.active then
      return p
    end
  end
  return self.pane_list[1]
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
---`adjust_pane_size` walks up to: leaf 1, a two-leaf tab, or content stacked in one column band.
function Tab:root_split_holds_active()
  if #self.pane_list <= 2 or self.active == self.pane_list[1] then
    return true
  end
  local band = nil
  for i = 2, #self.pane_list do
    local left = self.pane_list[i].left or (self.pane_list[1].cols + 1)
    if band ~= nil and left ~= band then
      return false
    end
    band = left
  end
  return true
end

---What `AdjustPaneSize` and `wezterm cli adjust-pane-size` both do: resize around the active leaf.
function Tab:adjust_from_active(dir, amount)
  if (dir == "Left" or dir == "Right") and self:root_split_holds_active() then
    local delta = dir == "Right" and amount or -amount
    self:set_split(math.max(1, math.min(self.pane_list[1].cols + delta, self:width() - 2)))
  end
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
      -- `left`/`width` are overridable per pane, so a test can place one inside another's columns.
      left = p.left or (i == 1 and 0 or first.cols + 1),
      top = p.top or 0,
      width = p.width or p.cols,
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
  local windows = require("wezterm").windows
  windows[#windows + 1] = w
  return w
end

function M.close_window(win)
  local windows = require("wezterm").windows
  for i, w in ipairs(windows) do
    if w == win then
      table.remove(windows, i)
      break
    end
  end
end

---A whole tab moved between windows, as a native tab drag does.
function M.move_tab(src, tab, dest)
  src:remove_tab(tab)
  tab._window = dest
  dest.tab_list[#dest.tab_list + 1] = tab
  dest.active_tab_ref = dest.active_tab_ref or tab
end

---Answers `wezterm cli` argvs the way the GUI's own socket would; wired in by `H.with_cli`.
function M.cli(args)
  local panes = require("wezterm").panes
  local flags = {}
  for i = 5, #args, 2 do
    flags[args[i]] = args[i + 1]
  end
  local sub = args[4]
  local pane = panes[tonumber(flags["--pane-id"] or "")]
  if sub == "kill-pane" and pane then
    M.kill_pane(pane)
    return true
  elseif sub == "split-pane" and pane then
    local moved = panes[tonumber(flags["--move-pane-id"] or "")]
    if not moved then
      return false
    end
    M.detach_pane(moved)
    local tab = pane._tab
    for i, p in ipairs(tab.pane_list) do
      if p == pane then
        table.insert(tab.pane_list, i + 1, moved)
        break
      end
    end
    moved._tab = tab
    return true
  end
  return false
end

---A mux client's view: the window reports its new size now, the panes catch up on `settle_mux`.
function Window:resize_mux(dcols)
  self.cols = self.cols + dcols
  self.pending_cols = (self.pending_cols or 0) + dcols
end

function Window:settle_mux()
  local pending = self.pending_cols or 0
  self.pending_cols = nil
  for _, tab in ipairs(self.tab_list) do
    tab:adjust_x_size(pending)
  end
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
  self.tab_enumerations = (self.tab_enumerations or 0) + 1
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
  await "spawn"
  local tab = self:add_tab { process = "/bin/zsh" }
  tab.spawn = spawn
  local pane = tab.pane_list[1]
  -- the real backend sets this title itself once it starts; the fake stands in for that
  local args = (spawn or {}).args or {}
  for i, arg in ipairs(args) do
    if arg == "--role" and args[i + 1] == "settings" then
      pane.title = string.format("wez-vtabs-settings:%08x", pane.id)
    end
  end
  return tab, pane, self
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
  local out = {
    resolved_palette = M.palette,
    skip_close_confirmation_for_processes_named = { "zsh", "wez-vtabs" },
    window_decorations = self.decorations,
    integrated_title_button_style = self.button_style,
    window_padding = self.window_padding,
  }
  for key, value in pairs(self.overrides or {}) do
    out[key] = value
  end
  return out
end
function Gui:get_config_overrides()
  return self.overrides or {}
end
function Gui:set_config_overrides(value)
  self.overrides = value
  self._mux.overrides_set = (self._mux.overrides_set or 0) + 1
end
function Gui:get_dimensions()
  return { pixel_width = self._mux.cols * 10, pixel_height = 600, is_full_screen = self.full_screen == true }
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
    self._mux.active_tab_ref:adjust_from_active(action.arg[1], action.arg[2])
  elseif name == "MoveTab" then
    local tab = self._mux.active_tab_ref
    self._mux:remove_tab(tab)
    table.insert(self._mux.tab_list, action.arg + 1, tab)
    self._mux.active_tab_ref = tab
  end
end

return M

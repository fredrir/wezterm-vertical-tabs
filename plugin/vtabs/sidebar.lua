local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local backend = require "vtabs.backend"
local util = require "vtabs.util"

local M = {}

local function pane_tab_id(pane)
  local ok, tab = pcall(function()
    return pane:tab()
  end)
  if ok and tab then
    return tab:tab_id()
  end
  return nil
end

---GUI-managed panes (connection UI, debug overlay) cannot host splits.
function M.is_overlay(pane)
  local ok, domain = pcall(function()
    return pane:get_domain_name()
  end)
  return not ok or type(domain) ~= "string" or domain:find "TermWiz" ~= nil
end

function M.is_sidebar(pane)
  if not pane then
    return false
  end
  local ok, vars = pcall(function()
    return pane:get_user_vars()
  end)
  if ok and vars and vars.vtabs_role == "sidebar" then
    return true
  end
  local tab_id = pane_tab_id(pane)
  return tab_id ~= nil and state.sidebar_pane_id(tab_id) == pane:pane_id()
end

---Splits a tab's panes into { content = Pane[], sidebar = Pane|nil }.
function M.classify(tab)
  local content, sidebar = {}, nil
  for _, p in ipairs(tab:panes()) do
    if M.is_sidebar(p) then
      sidebar = sidebar or p
    else
      content[#content + 1] = p
    end
  end
  return content, sidebar
end

function M.find(tab)
  local _, sb = M.classify(tab)
  return sb
end

function M.content_pane(tab)
  local active = tab:active_pane()
  if active and not M.is_sidebar(active) then
    return active
  end
  local content = M.classify(tab)
  local remembered = state.session.content_pane[tab:tab_id()]
  for _, p in ipairs(content) do
    if p:pane_id() == remembered then
      return p
    end
  end
  return content[1]
end

local function cwd_path(pane)
  local ok, cwd = pcall(function()
    return pane:get_current_working_dir()
  end)
  if not ok or not cwd then
    return nil
  end
  if type(cwd) == "string" then
    return cwd:gsub("^file://[^/]*", "")
  end
  return cwd.file_path
end

function M.tab_meta(tab, pane)
  local ok, domain = pcall(function()
    return pane:get_domain_name()
  end)
  local title = tab:get_title()
  return {
    cwd = cwd_path(pane),
    domain = ok and domain or nil,
    title = title ~= "" and title or nil,
    pinned = state.is_pinned(tab:tab_id()),
  }
end

local attaching = {}

---Splits off the sidebar pane; guarded because splits are async on mux domains.
function M.attach(tab)
  local cfg = config.get()
  local tab_id = tab:tab_id()
  local now = util.now_ms()
  if attaching[tab_id] and now - attaching[tab_id] < 5000 then
    return nil
  end
  local base = M.content_pane(tab)
  if not base or M.is_overlay(base) then
    return nil
  end
  attaching[tab_id] = now
  local domain = cfg.domain == "CurrentPaneDomain" and "CurrentPaneDomain" or { DomainName = cfg.domain }
  local ok, sb = pcall(function()
    return base:split {
      direction = cfg.position == "left" and "Left" or "Right",
      top_level = true,
      size = cfg.width,
      args = backend.spawn_args(cfg),
      set_environment_variables = backend.env(cfg),
      domain = domain,
    }
  end)
  if not ok or not sb then
    util.warn("sidebar split failed: %s", tostring(sb):match "^[^\n]*")
    return nil
  end
  attaching[tab_id] = nil
  state.set_sidebar(tab_id, sb:pane_id())
  base:activate()
  M.fit_to_window(tab)
  return sb
end

---Mux-domain splits can grow the tab past the window; re-sending the window size makes the mux refit it.
function M.fit_to_window(tab)
  pcall(function()
    local gui = tab:window():gui_window()
    if gui then
      local dims = gui:get_dimensions()
      gui:set_inner_size(dims.pixel_width, dims.pixel_height)
    end
  end)
end

---Closes a tab that only holds a sidebar without disturbing the active tab.
function M.close_orphan(gui_window, tab, sb)
  local previous = gui_window:mux_window():active_tab()
  local switching = previous and previous:tab_id() ~= tab:tab_id()
  if switching then
    tab:activate()
  end
  gui_window:perform_action(act.CloseCurrentTab { confirm = false }, sb)
  if switching then
    previous:activate()
  end
end

local function record_closed_tabs(wid, seen)
  local known = state.session.known_tabs or {}
  state.session.known_tabs = state.session.known_tabs or {}
  local previous = known[wid] or {}
  for tab_id in pairs(previous) do
    if not seen[tab_id] then
      local meta = state.session.tab_meta[tab_id]
      local moving = state.session.moving and state.session.moving[tab_id]
      if meta and not moving then
        state.push_closed(meta)
      end
      state.session.tab_meta[tab_id] = nil
      if state.session.moving then
        state.session.moving[tab_id] = nil
      end
      state.forget_tab(tab_id)
    end
  end
  state.session.known_tabs[wid] = seen
end

---Makes every tab match the collapsed/expanded state and closes tabs left with only a sidebar.
function M.ensure(gui_window)
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  local collapsed = state.is_collapsed(wid)
  local seen = {}

  for _, info in ipairs(mux_win:tabs_with_info()) do
    local tab = info.tab
    local tab_id = tab:tab_id()
    local content, sb = M.classify(tab)
    if #content == 0 then
      if sb then
        M.close_orphan(gui_window, tab, sb)
      end
    else
      seen[tab_id] = true
      local active = tab:active_pane()
      if active and not M.is_sidebar(active) then
        state.session.content_pane[tab_id] = active:pane_id()
      end
      state.session.tab_meta[tab_id] = M.tab_meta(tab, M.content_pane(tab))
      if collapsed and sb then
        M.detach(gui_window, tab)
      elseif not collapsed and not sb then
        M.attach(tab)
      end
    end
  end
  record_closed_tabs(wid, seen)

  local active_tab = mux_win:active_tab()
  if active_tab and not state.has_focus(wid) then
    local active = active_tab:active_pane()
    if active and M.is_sidebar(active) then
      local content = M.content_pane(active_tab)
      if content then
        content:activate()
      end
    end
  end
end

function M.set_collapsed(gui_window, collapsed)
  state.set_collapsed(gui_window:window_id(), collapsed)
  state.set_focus(gui_window:window_id(), false)
  M.ensure(gui_window)
end

function M.toggle(gui_window)
  M.set_collapsed(gui_window, not state.is_collapsed(gui_window:window_id()))
end

return M

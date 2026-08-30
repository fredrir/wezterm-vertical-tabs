local wezterm = require "wezterm" ---@type Wezterm
local backend = require "vtabs.backend"
local config = require "vtabs.config"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local util = require "vtabs.util"

local M = {}

local act = wezterm.action

---The settings tab of this mux window and the pane running the page, or nil.
---One page per window: `open` looks here before it spawns anything.
function M.find(mux_window)
  local infos = util.try(function()
    return mux_window:tabs_with_info()
  end) or {}
  for _, info in ipairs(infos) do
    local panes = util.try(function()
      return info.tab:panes()
    end) or {}
    for _, pane in ipairs(panes) do
      if sidebar.is_settings(pane) then
        return info.tab, pane
      end
    end
  end
  return nil
end

local function active_content_pane(gui_window)
  local tab = util.try(function()
    return gui_window:mux_window():active_tab()
  end)
  return tab and sidebar.content_pane(tab) or nil
end

---Registers the page with the bridge exactly as a sidebar is registered: a token this process
---minted, sent over the same channel, and trusted only once the backend echoes it back.
local function register(pane)
  state.set_token(pane:pane_id(), util.random_token())
  sidebar.auth(pane)
end

---Opens the settings page, or activates the one this window already has.
---The single owner: the strip button, the key binding and the popover item all come through here.
function M.open(gui_window)
  local mux_win = gui_window:mux_window()
  local existing, pane = M.find(mux_win)
  if existing then
    existing:activate()
    if pane and not sidebar.is_ready(pane) then
      register(pane)
    end
    return existing
  end
  local cfg = config.get()
  local base = active_content_pane(gui_window)
  local pane_domain = base and util.try(function()
    return base:get_domain_name()
  end) or "local"
  local args = backend.spawn_args(cfg, pane_domain, nil, "settings")
  if not args then
    util.warn_once("settings-backend", "no backend for domain %s; settings unavailable", tostring(pane_domain))
    return nil
  end
  local ok, tab, opened = pcall(function()
    return mux_win:spawn_tab {
      args = args,
      domain = { DomainName = pane_domain },
      set_environment_variables = backend.env(cfg, pane_domain, nil, nil),
    }
  end)
  if not ok or not tab then
    util.warn("settings spawn failed: %s", tostring(tab):match "^[^\n]*")
    return nil
  end
  register(opened)
  if not state.is_collapsed(gui_window:window_id()) then
    sidebar.attach(tab)
  end
  opened:activate()
  return tab
end

---Closes the settings tab by id. `CloseCurrentTab` ignores the pane it is handed, so the tab has to
---be the active one first or the wrong tab dies.
function M.close(gui_window)
  local tab, pane = M.find(gui_window:mux_window())
  if not tab then
    return false
  end
  local tab_id = tab:tab_id()
  tab:activate()
  local active = util.try(function()
    return gui_window:mux_window():active_tab()
  end)
  if not active or active:tab_id() ~= tab_id then
    util.warn_once("settings-close", "settings tab %s would not activate; not closing", tostring(tab_id))
    return false
  end
  -- every edit commits as it is made, so there is nothing to lose to a confirmation prompt
  util.try(function()
    gui_window:perform_action(act.CloseCurrentTab { confirm = false }, pane)
  end)
  return true
end

---Keys the page answers itself; everything else is the backend's business.
function M.key(gui_window, ev)
  if ev.key == "escape" or (ev.key == "q" and (ev.mods == nil or ev.mods == "" or ev.mods == "NONE")) then
    M.close(gui_window)
    return true
  end
  return false
end

return M

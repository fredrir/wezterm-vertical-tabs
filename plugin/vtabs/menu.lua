local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local util = require "vtabs.util"

local M = {}

local handlers = {
  activate = actions.activate_tab,
  pin = actions.toggle_pin,
  rename = actions.rename_tab,
  tear_off = actions.tear_off,
  close = actions.close_tab,
  close_others = actions.close_others,
  new_tab = function(window)
    actions.new_tab(window)
  end,
}

---Opens the tab menu as an overlay in the current content pane, without switching tabs.
function M.open(gui_window, tab_id, opts)
  local tab = actions.tab_by_id(gui_window, tab_id)
  local current = util.active_tab(gui_window)
  local content = current and sidebar.content_pane(current)
  if not tab or not content then
    return
  end
  local title = tab:get_title()
  local choices = {
    { id = "activate", label = "Switch to tab" },
    { id = "pin", label = state.is_pinned(tab_id) and "Unpin tab" or "Pin tab" },
    { id = "rename", label = "Rename tab" },
    { id = "tear_off", label = "Move to new window" },
    { id = "new_tab", label = "New tab" },
    { id = "close_others", label = "Close other tabs" },
    { id = "close", label = "Close tab" },
  }
  if not (opts and opts.keep_focus) then
    content:activate()
  end
  gui_window:perform_action(
    act.InputSelector {
      title = title ~= "" and title or "Tab",
      choices = choices,
      fuzzy = false,
      action = wezterm.action_callback(function(window, _, id)
        local handler = id and handlers[id]
        if handler then
          handler(window, tab_id)
        end
      end),
    },
    content
  )
end

return M

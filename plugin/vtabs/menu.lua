local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"

local M = {}

local function tab_by_id(gui_window, tab_id)
  for _, info in ipairs(gui_window:mux_window():tabs_with_info()) do
    if info.tab:tab_id() == tab_id then
      return info.tab
    end
  end
  return nil
end

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

function M.open(gui_window, tab_id)
  local tab = tab_by_id(gui_window, tab_id)
  local content = tab and sidebar.content_pane(tab)
  if not content then
    return
  end
  local choices = {
    { id = "activate", label = "Switch to tab" },
    { id = "pin", label = state.is_pinned(tab_id) and "Unpin tab" or "Pin tab" },
    { id = "rename", label = "Rename tab" },
    { id = "tear_off", label = "Move to new window" },
    { id = "new_tab", label = "New tab" },
    { id = "close_others", label = "Close other tabs" },
    { id = "close", label = "Close tab" },
  }
  content:activate()
  gui_window:perform_action(
    act.InputSelector {
      title = tab:get_title() ~= "" and tab:get_title() or "Tab",
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

local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local icons = require "vtabs.icons"
local util = require "vtabs.util"

local M = {}

local function title_for(tab, pane, cfg)
  if cfg.title then
    local ok, custom = pcall(cfg.title, tab, pane)
    if ok and custom and custom ~= "" then
      return custom
    end
  end
  local title = tab:get_title()
  if title ~= "" then
    return title
  end
  local ok, pane_title = pcall(function()
    return pane:get_title()
  end)
  if ok and pane_title and pane_title ~= "" then
    return pane_title
  end
  return "tab " .. tostring(tab:tab_id())
end

local function has_unseen(pane)
  local ok, unseen = pcall(function()
    return pane:has_unseen_output()
  end)
  return ok and unseen == true
end

---Builds the ordered list of sidebar items for a window.
function M.build(gui_window)
  local cfg = config.get()
  local mux_win = gui_window:mux_window()
  local private = state.is_private(gui_window:window_id())
  local items = {}
  for _, info in ipairs(mux_win:tabs_with_info()) do
    local tab = info.tab
    local include = true
    if cfg.hooks.filter then
      local ok, keep = pcall(cfg.hooks.filter, tab, mux_win)
      include = not ok or keep ~= false
    end
    local pane = include and sidebar.content_pane(tab) or nil
    if pane then
      local tab_id = tab:tab_id()
      items[#items + 1] = {
        tab_id = tab_id,
        index = info.index + 1,
        is_active = info.is_active,
        is_pinned = state.is_pinned(tab_id),
        is_private = private,
        title = title_for(tab, pane, cfg),
        icon = cfg.icons and icons.for_pane(pane, cfg.icon_map) or "",
        has_unseen = has_unseen(pane),
      }
    end
  end
  return items
end

---Rendered order: pinned first, then the rest, both in physical order.
function M.ordered(items)
  local pinned, rest = {}, {}
  for _, item in ipairs(items) do
    if item.is_pinned then
      pinned[#pinned + 1] = item
    else
      rest[#rest + 1] = item
    end
  end
  for _, item in ipairs(rest) do
    pinned[#pinned + 1] = item
  end
  return pinned
end

function M.find(items, tab_id)
  for i, item in ipairs(items) do
    if item.tab_id == tab_id then
      return item, i
    end
  end
  return nil
end

M.util = util

return M

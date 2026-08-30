local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local icons = require "vtabs.icons"
local util = require "vtabs.util"

local M = {}

local function title_for(tab, pane, cfg)
  if cfg.title then
    local ok, custom = pcall(cfg.title, tab, pane)
    if not ok then
      util.warn_once("hook-title", "title hook failed: %s", tostring(custom))
    elseif custom and custom ~= "" then
      return custom
    end
  end
  local title = tab:get_title()
  if title ~= "" then
    return title
  end
  local pane_title = util.try(function()
    return pane:get_title()
  end)
  if pane_title and pane_title ~= "" then
    return pane_title
  end
  return "tab " .. tostring(tab:tab_id())
end

local function included(cfg, tab, mux_win)
  if not cfg.hooks.filter then
    return true
  end
  local ok, keep = pcall(cfg.hooks.filter, tab, mux_win)
  if not ok then
    util.warn_once("hook-filter", "filter hook failed: %s", tostring(keep))
    return true
  end
  return keep ~= false
end

---Builds the list of visible sidebar items for a window, in physical order.
function M.build(gui_window)
  local cfg = config.get()
  local mux_win = gui_window:mux_window()
  local private = state.is_private(gui_window:window_id())
  local items = {}
  for _, info in ipairs(mux_win:tabs_with_info()) do
    local tab = info.tab
    local pane = included(cfg, tab, mux_win) and sidebar.content_pane(tab) or nil
    if pane then
      local tab_id = tab:tab_id()
      items[#items + 1] = {
        tab_id = tab_id,
        index = info.index + 1,
        is_active = info.is_active,
        is_pinned = state.is_pinned(tab_id),
        is_private = private,
        title = util.sanitize(title_for(tab, pane, cfg)),
        icon = cfg.icons and icons.for_pane(pane, cfg.glyphs) or "",
        has_unseen = util.try(function()
          return pane:has_unseen_output()
        end) == true,
      }
    end
  end
  return items
end

---Rendered order: pinned first, then the rest, both in physical order.
function M.ordered(items)
  local pinned, rest = util.partition(items, function(item)
    return item.is_pinned
  end)
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

function M.ids(items)
  local ids = {}
  for i, item in ipairs(items) do
    ids[i] = item.tab_id
  end
  return ids
end

return M

local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local util = require "vtabs.util"

local M = {}

local session = state.session
local adopted = {}
local observed = {}

---Width the sidebar of `window_id` should have: what the user last dragged it to, else `cfg.width`.
function M.desired(window_id)
  return adopted[window_id] or config.get().width
end

function M.forget_window(window_id)
  adopted[window_id] = nil
  observed[window_id] = nil
end

M.reset = M.forget_window
table.insert(state.forget_hooks, M.forget_window)

local function pane_cols(pane)
  local d = util.try(function()
    return pane:get_dimensions()
  end)
  return d and d.cols or nil
end

local function window_px(gui_window)
  local d = util.try(function()
    return gui_window:get_dimensions()
  end)
  return d and d.pixel_width or nil
end

---`AdjustPaneSize` shifts the split node's FIRST child: `Right` by `+n`, `Left` by `-n`.
local function direction_for(position, delta)
  if (position == "left") == (delta > 0) then
    return "Right"
  end
  return "Left"
end

---Re-asserts the sidebar width on the active tab; background tabs are corrected once they activate.
function M.correct(gui_window)
  local cfg = config.get()
  local wid = gui_window:window_id()
  if session.drag[wid] then
    return false
  end
  local tab = util.active_tab(gui_window)
  local sb = tab and sidebar.find(tab)
  local cols = sb and pane_cols(sb)
  if not cols then
    return false
  end
  local tab_id = tab:tab_id()
  local px = window_px(gui_window)
  local seen = observed[wid]
  observed[wid] = { tab_id = tab_id, cols = cols, px = px }
  if seen and seen.tab_id == tab_id and seen.px == px and seen.cols ~= cols then
    adopted[wid] = cols
    return false
  end

  local delta = M.desired(wid) - cols
  if delta == 0 then
    return false
  end
  local content = sidebar.classify(tab)
  local active = #content > 1 and tab:active_pane() or nil
  local restore = active and active:pane_id() ~= sb:pane_id() and active or nil
  if restore then
    sb:activate()
  end
  local adjusted = util.try(function()
    return gui_window:perform_action(act.AdjustPaneSize { direction_for(cfg.position, delta), math.abs(delta) }, sb)
      or true
  end)
  if restore then
    restore:activate()
  end
  observed[wid].cols = pane_cols(sb) or cols
  return adjusted ~= nil
end

return M

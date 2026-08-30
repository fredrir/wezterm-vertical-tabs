local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local util = require "vtabs.util"

local M = {}

local MIN_WIDTH = 8
local EDGE_MARGIN = 20
local OBSERVE_MS = 400

local session = state.session
local adopted = {}
local observed = {}
local checked = {}

function M.desired(window_id)
  return adopted[window_id] or config.get().width
end

function M.forget_window(window_id)
  adopted[window_id] = nil
  observed[window_id] = nil
  checked[window_id] = nil
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

---Tab width in cells and whether any pane is zoomed; `panes_with_info` reports the unzoomed layout.
local function tab_metrics(tab)
  local infos = util.try(function()
    return tab:panes_with_info()
  end)
  if type(infos) ~= "table" or #infos == 0 then
    return nil, false
  end
  local cols, zoomed = 0, false
  for _, info in ipairs(infos) do
    cols = math.max(cols, (info.left or 0) + (info.width or 0))
    zoomed = zoomed or info.is_zoomed == true
  end
  return cols, zoomed
end

---Only a plausible sidebar width may be latched; a stale or oversized read must not stick.
local function plausible(cols, tab_cols)
  local ceiling = tab_cols and math.max(MIN_WIDTH, tab_cols - EDGE_MARGIN) or nil
  local out = math.max(cols, MIN_WIDTH)
  return ceiling and math.min(out, ceiling) or out
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
  local tab = util.active_tab(gui_window)
  checked[wid] = { tab_id = tab and tab:tab_id() or nil, at = util.now_ms() }
  if session.drag[wid] then
    return false
  end
  local sb = tab and sidebar.find(tab)
  local cols = sb and sidebar.is_ready(sb) and pane_cols(sb)
  if not cols then
    return false
  end
  local tab_cols, zoomed = tab_metrics(tab)
  if zoomed then
    return false
  end
  local tab_id = tab:tab_id()
  local px = window_px(gui_window)
  local seen = observed[wid]
  observed[wid] = { tab_id = tab_id, cols = cols, px = px }
  if seen and seen.tab_id == tab_id and seen.px == px and seen.cols ~= cols then
    adopted[wid] = plausible(cols, tab_cols)
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

---Per-poll entry point: corrects at once when the active tab changed, otherwise at most every 400 ms.
function M.sync(gui_window, active_tab_id)
  local last = checked[gui_window:window_id()]
  if last and last.tab_id == active_tab_id and util.now_ms() - last.at < OBSERVE_MS then
    return false
  end
  return M.correct(gui_window)
end

return M

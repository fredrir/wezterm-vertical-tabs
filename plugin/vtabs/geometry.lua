local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local util = require "vtabs.util"

local M = {}

local MIN_WIDTH = 8
local MIN_CONTENT = 20
local OBSERVE_MS = 400
local SETTLE_MS = 1000
local STUCK_TRIES = 3

local session = state.session
local adopted = {}
local observed = {}
local checked = {}
local unreachable = {}
local settling = {}
local attempted = {}

function M.desired(window_id)
  return adopted[window_id] or config.get().width
end

function M.forget_window(window_id)
  adopted[window_id] = nil
  observed[window_id] = nil
  checked[window_id] = nil
  unreachable[window_id] = nil
  settling[window_id] = nil
  attempted[window_id] = nil
end

---A mux client sees the new window size a poll before the new pane sizes; no adoption until both land.
function M.settle(window_id, at)
  settling[window_id] = at or util.now_ms()
end

M.reset = M.forget_window
table.insert(state.forget_hooks, M.forget_window)

---Columns, dpi and cell width of a pane; the last two tell a divider drag from a font or DPI change.
local function pane_metrics(pane)
  local d = util.try(function()
    return pane:get_dimensions()
  end)
  if type(d) ~= "table" or not d.cols or d.cols < 1 then
    return nil
  end
  return d.cols, d.dpi, d.pixel_width and d.pixel_width // d.cols or nil
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

---Width the split can actually hold: `adjust_node_at_cursor` clamps `first.cols` to `[1, width-2]`.
local function fits(cols, tab_cols)
  local out = math.max(cols, MIN_WIDTH)
  if not tab_cols then
    return out
  end
  return math.min(out, math.max(MIN_WIDTH, tab_cols - MIN_CONTENT))
end

---`AdjustPaneSize` shifts the split node's FIRST child: `Right` by `+n`, `Left` by `-n`.
local function direction_for(position, delta)
  if (position == "left") == (delta > 0) then
    return "Right"
  end
  return "Left"
end

local function same_attempt(a, b)
  return a and b and a.tab_id == b.tab_id and a.tab_cols == b.tab_cols and a.target == b.target
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
  if not sb or not sidebar.is_ready(sb) then
    return false
  end
  local cols, dpi, cell = pane_metrics(sb)
  if not cols then
    return false
  end
  local tab_cols, zoomed = tab_metrics(tab)
  if zoomed then
    return false
  end
  local tab_id = tab:tab_id()
  local px = window_px(gui_window)
  local now = util.now_ms()
  local seen = observed[wid]
  observed[wid] = { tab_id = tab_id, cols = cols, px = px, dpi = dpi, cell = cell, tab_cols = tab_cols }
  if seen and seen.tab_cols ~= tab_cols then
    settling[wid] = now
  end
  -- A divider drag moves the sidebar within a tab whose own width, pixels and cells all stay put.
  local steady = seen
    and seen.tab_id == tab_id
    and seen.px == px
    and seen.dpi == dpi
    and seen.cell == cell
    and seen.tab_cols == tab_cols
    and now - (settling[wid] or 0) >= SETTLE_MS
  if steady and seen.cols ~= cols then
    adopted[wid] = fits(cols, tab_cols)
    unreachable[wid] = nil
    return false
  end

  local target = fits(M.desired(wid), tab_cols)
  local attempt = { tab_id = tab_id, tab_cols = tab_cols, target = target, cols = cols }
  local last = attempted[wid]
  -- A mux applies the adjust a poll late, so a width counts as unreachable only after it sits still.
  attempt.stuck = (last and same_attempt(last, attempt) and last.cols == cols) and last.stuck + 1 or 0
  if attempt.stuck >= STUCK_TRIES then
    unreachable[wid] = attempt
  end
  if target == cols or same_attempt(unreachable[wid], attempt) then
    attempted[wid] = nil
    return false
  end
  local content = sidebar.classify(tab)
  local active = #content > 1 and tab:active_pane() or nil
  local restore = active and active:pane_id() ~= sb:pane_id() and active or nil
  if restore then
    sb:activate()
  end
  util.try(function()
    gui_window:perform_action(
      act.AdjustPaneSize { direction_for(cfg.position, target - cols), math.abs(target - cols) },
      sb
    )
  end)
  if restore then
    restore:activate()
  end
  attempted[wid] = attempt
  settling[wid] = now
  return true
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

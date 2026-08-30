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
-- A mux applies an adjust a poll late. Re-issuing it meanwhile stacks adjusts that all land, and
-- the overshoot then reads as a divider drag and is adopted, so one is outstanding at a time.
local ADJUST_WAIT_MS = 1000
-- `window-resized` fires once per frame of a drag; correcting per frame costs an adjust and a
-- repaint each. The leading edge is corrected, the rest waits for the poll after the drag stops.
local RESIZE_QUIET_MS = 150

local session = state.session
local adopted = {}
local adopted_for = {}
local observed = {}
local checked = {}
local unreachable = {}
local settling = {}
local attempted = {}
local in_flight = {}
local resized_at = {}
local rail_reserve = {}

---Target width: the rail when collapsed, else what the user last dragged it to, else `cfg.width`.
function M.desired(window_id)
  local cfg = config.get()
  if cfg.collapsed == "rail" and state.is_collapsed(window_id) then
    if cfg.rail_titlebar == "widen" then
      return math.max(cfg.rail_width, rail_reserve[window_id] or 0)
    end
    return cfg.rail_width
  end
  return adopted[window_id] or cfg.width
end

---The macOS light reserve the last frame measured; the rail is widened to it so the lights land on
---the sidebar rather than on the shell. `view` writes it, because only a render has a cell size.
function M.set_rail_cols(window_id, cols)
  rail_reserve[window_id] = (cols or 0) > 0 and cols or nil
end

function M.forget_window(window_id)
  adopted[window_id] = nil
  adopted_for[window_id] = nil
  observed[window_id] = nil
  checked[window_id] = nil
  unreachable[window_id] = nil
  settling[window_id] = nil
  attempted[window_id] = nil
  in_flight[window_id] = nil
  resized_at[window_id] = nil
  rail_reserve[window_id] = nil
end

---A mux client sees the new window size a poll before the new pane sizes; no adoption until both land.
function M.settle(window_id, at)
  settling[window_id] = at or util.now_ms()
end

---The sidebar reporting its own new size is proof the adjust landed, so the next one need not wait.
function M.landed(window_id)
  in_flight[window_id] = nil
end

---True on the first resize event of a burst. A drag is one burst, so it corrects once at the start
---and once from the poll after it stops, rather than once per frame in between.
function M.on_resize(window_id)
  local now = util.now_ms()
  local last = resized_at[window_id]
  resized_at[window_id] = now
  M.settle(window_id, now)
  return last == nil or now - last >= RESIZE_QUIET_MS
end

---A config reload only invalidates a dragged width when `width` itself changed; every edit to
---`wezterm.lua` reloads, and the plugin watches its own files too. The settle stamp is kept either
---way, or the first pair of observations after the reload would look steady and adopt what it finds.
function M.reset(window_id)
  local width = config.get().width
  local keep = adopted_for[window_id] == width and adopted[window_id] or nil
  M.forget_window(window_id)
  adopted[window_id], adopted_for[window_id] = keep, keep and width or nil
  settling[window_id] = util.now_ms()
end

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

---Width the split can hold: `adjust_node_at_cursor` clamps `first.cols` to `[1, width-2]`.
local function fits(cols, tab_cols, floor)
  floor = floor or MIN_WIDTH
  local out = math.max(cols, floor)
  if not tab_cols then
    return out
  end
  return math.min(out, math.max(floor, tab_cols - MIN_CONTENT))
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
  local collapsed = state.is_collapsed(wid)
  local seen = observed[wid]
  observed[wid] =
    { tab_id = tab_id, cols = cols, px = px, dpi = dpi, cell = cell, tab_cols = tab_cols, collapsed = collapsed }
  if seen and seen.tab_cols ~= tab_cols then
    settling[wid] = now
  end
  -- A divider drag moves the sidebar within a tab whose own width, pixels and cells all stay put.
  -- A rail width is ours, not a drag, and the width either side of a toggle is not comparable.
  -- A width seen while our own adjust is still in flight is that adjust landing, never a drag; so
  -- is one that matches what we last asked for. Anything else, in a tab whose own width, pixels and
  -- cells all stayed put, is the user on the divider — and it is adopted at once, because waiting
  -- out a settle window means the correction below pulls the divider back while they still hold it.
  local asked = attempted[wid] and attempted[wid].target or nil
  local steady = seen
    and not collapsed
    and seen.collapsed == false
    and seen.tab_id == tab_id
    and seen.px == px
    and seen.dpi == dpi
    and seen.cell == cell
    and seen.tab_cols == tab_cols
    and in_flight[wid] == nil
    and cols ~= asked
  if steady and seen.cols ~= cols then
    adopted[wid] = fits(cols, tab_cols)
    adopted_for[wid] = cfg.width
    unreachable[wid] = nil
    return false
  end

  -- A rail is deliberately narrower than any sidebar, so the floor is the rail's own width.
  local want = M.desired(wid)
  local target = fits(want, tab_cols, collapsed and want or MIN_WIDTH)
  local attempt = { tab_id = tab_id, tab_cols = tab_cols, target = target, cols = cols }
  if target == cols then
    attempted[wid] = nil
    in_flight[wid] = nil
    return false
  end
  if same_attempt(unreachable[wid], attempt) then
    attempted[wid] = nil
    return false
  end
  -- One copy of an adjust outstanding at a time: a mux applies it a poll late, and the duplicate
  -- lands too, overshooting into a width the drag heuristic would then adopt. A *different* target
  -- means the situation moved on, so that one goes out at once.
  local pending = in_flight[wid]
  if pending and now - pending.at < ADJUST_WAIT_MS and same_attempt(pending.attempt, attempt) then
    return false
  end
  local last = attempted[wid]
  -- A mux applies the adjust a poll late, so a width counts as unreachable only after it sits still
  -- across attempts we actually made; a wait for one still in flight is not evidence of anything.
  attempt.stuck = (last and same_attempt(last, attempt) and last.cols == cols) and last.stuck + 1 or 0
  if attempt.stuck >= STUCK_TRIES then
    unreachable[wid] = attempt
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
  in_flight[wid] = { at = now, attempt = attempt }
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

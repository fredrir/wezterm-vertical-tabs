local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local mux = require "vtabs.mux"
local util = require "vtabs.util"

local M = {}

local MIN_WIDTH = 8
local MIN_CONTENT = 20
local OBSERVE_MS = 400
local STUCK_TRIES = 3
-- A mux applies an adjust a poll late. Re-issuing it meanwhile stacks adjusts that all land, and
-- the overshoot then reads as a divider drag and is adopted, so one is outstanding at a time.
local ADJUST_WAIT_MS = 1000
-- No width is read as a drag until it has sat still this long, and never this soon after an adjust
-- of ours: a mux can apply one in pieces, and each piece looks exactly like a hand on the divider.
local ADOPT_FLOOR_MS = 250
-- `window-resized` fires once per frame of a drag; correcting per frame costs an adjust and a
-- repaint each. The leading edge is corrected, the rest waits for the poll after the drag stops.
local RESIZE_QUIET_MS = 150

local session = state.session
local adopted = {}
local adopted_for = {}
local observed = {}
local checked = {}
local unreachable = {}
local attempted = {}
local in_flight = {}
local driven = {}
local resized_at = {}
local rail_reserve = {}
local last_target = {}

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
  attempted[window_id] = nil
  in_flight[window_id] = nil
  driven[window_id] = nil
  resized_at[window_id] = nil
  rail_reserve[window_id] = nil
  last_target[window_id] = nil
end

---The sidebar reporting its own size means our adjust moved something, so the next one need not
---wait its turn. It does not mean the whole delta arrived, so the width stays ours for the floor.
function M.landed(window_id)
  in_flight[window_id] = nil
end

---True on the first resize event of a burst. A drag is one burst, so it corrects once at the start
---and once from the poll after it stops, rather than once per frame in between.
function M.on_resize(window_id)
  local now = util.now_ms()
  local last = resized_at[window_id]
  resized_at[window_id] = now
  return last == nil or now - last >= RESIZE_QUIET_MS
end

---A config reload only invalidates a dragged width when `width` itself changed: every edit to
---`wezterm.lua` reloads, and the plugin watches its own files too.
function M.reset(window_id)
  local width = config.get().width
  local keep = adopted_for[window_id] == width and adopted[window_id] or nil
  M.forget_window(window_id)
  adopted[window_id], adopted_for[window_id] = keep, keep and width or nil
end

table.insert(state.forget_hooks, M.forget_window)

---Columns, dpi and cell width of a pane; the last two tell a divider drag from a font or DPI change.
local function pane_metrics(pane)
  local d = mux.dims(pane)
  if type(d) ~= "table" or not d.cols or d.cols < 1 then
    return nil
  end
  return d.cols, d.dpi, d.pixel_width and d.pixel_width // d.cols or nil
end

local function window_px(gui_window)
  local d = mux.dims(gui_window)
  return d and d.pixel_width or nil
end

---Tab width in cells and whether any pane is zoomed; `panes_with_info` reports the unzoomed layout.
local function tab_metrics(tab, sb_id)
  local infos = mux.panes_with_info(tab)
  if type(infos) ~= "table" or #infos == 0 then
    return nil, false, 1
  end
  local cols, zoomed = 0, false
  -- Content panes sharing a `left` are stacked and share one column band; distinct lefts are bands
  -- side by side, and each needs its own minimum out of the width the sidebar leaves.
  local bands, n = {}, 0
  for _, info in ipairs(infos) do
    cols = math.max(cols, (info.left or 0) + (info.width or 0))
    zoomed = zoomed or info.is_zoomed == true
    local id = info.pane and info.pane:pane_id() or nil
    local left = info.left or 0
    if id ~= sb_id and not bands[left] then
      bands[left], n = true, n + 1
    end
  end
  return cols, zoomed, math.max(n, 1)
end

---Width the split can hold: `adjust_node_at_cursor` clamps `first.cols` to `[1, width-2]`, and every
---content band keeps `MIN_CONTENT` of its own -- charging it once leaves two shells splitting one
---band's worth between them.
local function fits(cols, tab_cols, floor, bands)
  floor = floor or MIN_WIDTH
  local out = math.max(cols, floor)
  if not tab_cols then
    return out
  end
  return math.min(out, math.max(floor, tab_cols - MIN_CONTENT * math.max(bands or 1, 1)))
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
  local tab_cols, zoomed, bands = tab_metrics(tab, sb:pane_id())
  if zoomed then
    return false
  end
  local tab_id = tab:tab_id()
  local px = window_px(gui_window)
  local now = util.now_ms()
  local collapsed = state.is_collapsed(wid)
  -- Comparable = the same sidebar in the same tab, with the window, font and tab width all
  -- unchanged, so the only thing that can have moved this width is the divider or us.
  local seen = observed[wid]
  local comparable = seen ~= nil
    and not collapsed
    and seen.collapsed == false
    and seen.tab_id == tab_id
    and seen.px == px
    and seen.dpi == dpi
    and seen.cell == cell
    and seen.tab_cols == tab_cols
  local since = (comparable and seen.cols == cols) and (seen.cols_at or now) or now
  observed[wid] = {
    tab_id = tab_id,
    cols = cols,
    cols_at = since,
    px = px,
    dpi = dpi,
    cell = cell,
    tab_cols = tab_cols,
    collapsed = collapsed,
  }

  -- A rail is deliberately narrower than any sidebar, so the floor is the rail's own width.
  local want = M.desired(wid)
  local target = fits(want, tab_cols, collapsed and want or MIN_WIDTH, bands)
  local attempt = { tab_id = tab_id, tab_cols = tab_cols, target = target, cols = cols }
  if target == cols then
    -- Remembered, not just forgotten: a width we asked for stays ours after we stop asking, or the
    -- adoption branch below reads our own clamp back as a hand on the divider.
    last_target[wid] = target
    attempted[wid] = nil
    in_flight[wid] = nil
    return false
  end

  -- Nothing our own adjust produced is a drag, and a mux can apply one in pieces: the width stays
  -- ours while a target of ours is unmet, while an adjust is in flight, and for a floor after it.
  -- Nor is wezterm's own doing: it deals the sidebar half of every column a window drag adds, and
  -- the last deal lands after the pixels and the tab width have already stopped moving.
  local outstanding = attempted[wid] ~= nil or in_flight[wid] ~= nil
  local quiet = math.max(cfg.poll_ms, ADOPT_FLOOR_MS)
  local settled = now - (driven[wid] or 0) >= ADOPT_FLOOR_MS and now - (resized_at[wid] or 0) >= quiet
  if comparable and not outstanding and settled and cols ~= (last_target[wid] or -1) then
    if seen.cols ~= cols or now - since < ADOPT_FLOOR_MS then
      -- Moving, and not by us: the user is on the divider. Correcting now fights their hand.
      return false
    end
    -- One band, deliberately: what they dragged to is their preference, and a second shell open at
    -- that moment must not rewrite it. The bands clamp the target below, every time it is computed.
    adopted[wid] = fits(cols, tab_cols, nil, 1)
    adopted_for[wid] = cfg.width
    unreachable[wid] = nil
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
  -- `AdjustPaneSize` ignores its pane argument and moves whichever pane is active, so the sidebar
  -- has to be the active one when it runs -- in a single-content tab as much as any other.
  local active = mux.active_pane(tab)
  local restore = active and active:pane_id() ~= sb:pane_id() and active or nil
  if restore then
    sb:activate()
  end
  local adjust = act.AdjustPaneSize { direction_for(cfg.position, target - cols), math.abs(target - cols) }
  mux.call(gui_window, "perform_action", adjust, sb)
  if restore then
    restore:activate()
  end
  attempted[wid] = attempt
  in_flight[wid] = { at = now, attempt = attempt }
  driven[wid] = now
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

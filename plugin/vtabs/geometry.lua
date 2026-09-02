local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local gate = require "vtabs.gate"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local mux = require "vtabs.mux"
local util = require "vtabs.util"

---Holds the sidebar at its width against everything WezTerm does to a split. A window resize deals
---the sidebar half of every column it adds or removes (mux/src/tab.rs `adjust_x_size`), a divider
---drag moves it, a split or a close reshapes the tab around it. The tab's split tree, read through
---`panes_with_info`, is the one source of truth: WezTerm updates it synchronously for each of those,
---in every domain, so a correction reads it, acts, and reads it again. Nothing here waits on a timer
---to guess whether a mux has caught up.
local M = {}

local MIN_WIDTH = 8
local MIN_CONTENT = 20
-- `window-resized` fires once per frame of a drag or an animated fill. A frame is corrected at once,
-- and no width that moves this soon after one is read as a divider drag.
local RESIZE_QUIET_MS = 100
M.RESIZE_QUIET_MS = RESIZE_QUIET_MS
M.SETTLE_MS = RESIZE_QUIET_MS + 20
-- An adjust the host declined (a WezTerm overlay owns the tab) is asked again this much later.
local BLOCKED_MS = 2000
-- A remote `perform_action` returns before its mux applies the adjustment. Suppress duplicate
-- deltas until the first one is visible; otherwise several identical polls can overshoot.
local REMOTE_APPLY_MS = 250
M.REMOTE_APPLY_MS = REMOTE_APPLY_MS

---Declared through `store`, so forgetting a window clears them without a list to keep in step.
local scope = store.scope "geometry"
local adopted = scope.window()
local adopted_for = scope.window()
local rail_reserve = scope.window()
local resized_at = scope.window()
local resize_gen = scope.window()
-- The layout a correction last left behind or found in order. The divider moving away from it with
-- the tab otherwise unchanged is the user's hand on it.
local settled = scope.window()
-- A target this tab cannot hold, or an adjust the host is refusing: not asked for again until the
-- tab changes shape, or the refusal has had time to clear.
local unreachable = scope.window()
-- Tabs whose adjust once walked into a content split: from then on it is issued from the sidebar.
local via_sidebar = scope.window()
local pending_adjust = scope.window()

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

---Stores Rust's computed macOS light reserve and applies it when a collapsed rail is widened.
function M.apply_rail_reserve(gui_window, cols)
  local wid = gui_window:window_id()
  local next_reserve = type(cols) == "number" and cols > 0 and math.floor(cols) or nil
  if rail_reserve[wid] == next_reserve then
    return false
  end
  rail_reserve[wid] = next_reserve
  local cfg = config.get()
  if cfg.rail_titlebar == "widen" and cfg.collapsed == "rail" and state.is_collapsed(wid) then
    return M.correct(gui_window)
  end
  return false
end

function M.rail_reserve(window_id)
  return rail_reserve[window_id]
end

M.forget_window = scope.forget_window

---Records one frame of a window resize and returns the burst's generation, so the settle timer
---armed by the last frame is the one that publishes.
function M.on_resize(window_id)
  resized_at[window_id] = util.now_ms()
  resize_gen[window_id] = (resize_gen[window_id] or 0) + 1
  return resize_gen[window_id]
end

function M.resize_gen(window_id)
  return resize_gen[window_id] or 0
end

function M.has_pending_adjust(window_id)
  return pending_adjust[window_id] ~= nil
end

---A config reload only invalidates a dragged width when `width` itself changed: every edit to
---`wezterm.lua` reloads, and the plugin watches its own files too.
function M.reset(window_id)
  local width = config.get().width
  local keep = adopted_for[window_id] == width and adopted[window_id] or nil
  M.forget_window(window_id)
  adopted[window_id], adopted_for[window_id] = keep, keep and width or nil
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
local function first_child_direction(delta)
  return delta > 0 and "Right" or "Left"
end

---A left sidebar is the root split's first child and grows with `Right`; a right one is its second.
local function direction_for(position, delta)
  return first_child_direction(position == "left" and delta or -delta)
end

---`panes_with_info` hands its numbers over as floats; `AdjustPaneSize` and `ActivatePaneByIndex`
---take only integers back, so every reading is whole from the start.
local function int(value)
  return math.floor(tonumber(value) or 0)
end

---One reading of a tab's split tree. Every number comes from the same `panes_with_info` call, so
---the sidebar's rect, the tab's size and the active leaf cannot disagree with each other.
---`panes_with_info` reports the unzoomed layout (mux/src/tab.rs `iter_panes_ignoring_zoom`).
local function read_layout(tab, sb_id, position)
  local infos = mux.panes_with_info(tab)
  if type(infos) ~= "table" or #infos == 0 then
    return nil
  end
  local layout = { cols = 0, zoomed = false, bands = 0, content = {} }
  local lefts = {}
  for _, info in ipairs(infos) do
    local left, width = int(info.left), int(info.width)
    local id = mux.call(info.pane, "pane_id")
    layout.cols = math.max(layout.cols, left + width)
    layout.zoomed = layout.zoomed or info.is_zoomed == true
    local rect = { index = int(info.index), id = id, left = left, width = width }
    if id == sb_id then
      rect.pixel_width = int(info.pixel_width)
      layout.sidebar = rect
    else
      layout.content[#layout.content + 1] = rect
      -- Content sharing a `left` is stacked in one column band; distinct lefts are bands side by side.
      if not lefts[left] then
        lefts[left] = true
        layout.bands = layout.bands + 1
      end
    end
    if info.is_active then
      layout.active = rect
    end
  end
  if not layout.sidebar then
    return nil
  end
  local sb = layout.sidebar
  layout.cell = sb.pixel_width and sb.width > 0 and sb.pixel_width // sb.width or nil
  layout.content_width = layout.cols - sb.width - 1
  layout.bands = math.max(layout.bands, 1)
  -- The content's shape, as the divider cannot change it: moving it shifts every content pane by
  -- the same amount (or, beside a right sidebar, none of them), while a split, a close, a move or a
  -- rotation renames or relocates one. Preorder is stable for one tree, so no sort is needed.
  local shape = {}
  for i, rect in ipairs(layout.content) do
    shape[i] = tostring(rect.id) .. "@" .. (position == "right" and rect.left or rect.left - sb.width)
  end
  layout.shape = table.concat(shape, " ")
  return layout
end

---The sidebar sits at its configured edge. A layout the user rotated or swapped it out of is not
---this module's to police; a pane split into its column is, and `sidebar_rescue` walks that one
---back out while the adjust here can at worst squeeze it until then.
local function at_edge(layout, position)
  local sb = layout.sidebar
  if position == "right" then
    return sb.left + sb.width == layout.cols
  end
  return sb.left == 0
end

local function record(tab_id, layout)
  return {
    tab_id = tab_id,
    tab_cols = layout.cols,
    cols = layout.sidebar.width,
    shape = layout.shape,
    cell = layout.cell,
  }
end

---The same tab in the same shape: nothing but the divider (or an adjust of ours) can have moved.
local function same_shape(seen, tab_id, layout)
  return seen ~= nil
    and seen.tab_id == tab_id
    and seen.tab_cols == layout.cols
    and seen.shape == layout.shape
    and seen.cell == layout.cell
end

---A content pane whose width changed between two readings of the same tab, and by how much.
local function content_shift(before, after)
  local widths = {}
  for _, rect in ipairs(before.content) do
    widths[rect.id] = rect.width
  end
  for _, rect in ipairs(after.content) do
    local was = widths[rect.id]
    if was and was ~= rect.width then
      return rect.width - was
    end
  end
  return nil
end

---The adjust walks up from the tab's active leaf to the nearest horizontal split (mux/src/tab.rs
---`adjust_pane_size`). From the sidebar that is the root. From a content pane it is the root only
---when no horizontal split lies between them, which is exactly when the pane spans the whole
---content width: every horizontal split narrows its children, a vertical one never does.
local function needs_dance(layout, sb_id, tab_id, wid)
  local active = layout.active
  if active == nil or (via_sidebar[wid] or {})[tab_id] then
    return true
  end
  return active.id ~= sb_id and active.width ~= layout.content_width
end

---Re-asserts the sidebar width on the active tab; background tabs are corrected once they activate.
local function correct(gui_window, snapshot)
  local cfg = snapshot and snapshot.cfg or config.get()
  local wid = snapshot and snapshot.window_id or gui_window:window_id()
  if store.drag[wid] then
    return false
  end
  local observed_tab = snapshot and snapshot.active or nil
  local tab = observed_tab and observed_tab.tab or util.active_tab(gui_window)
  if not tab then
    return false
  end
  local sb, ready
  if observed_tab then
    sb, ready = observed_tab.sidebar, observed_tab.sidebar_ready
  else
    sb = sidebar.find(tab)
    ready = sb and sidebar.is_ready(sb)
  end
  if not sb or not ready then
    return false
  end
  local sb_id = sb:pane_id()
  local tab_id = mux.tab_id(tab)
  if tab_id == nil then
    return false
  end
  -- Read afresh even under a snapshot: one taken before this poll's last frame would hand the
  -- adjust a stale delta, and the tree is one cheap call away.
  local layout = read_layout(tab, sb_id, cfg.position)
  if not layout or layout.zoomed or not at_edge(layout, cfg.position) then
    return false
  end
  local now = util.now_ms()
  local quiet = now - (resized_at[wid] or 0) >= RESIZE_QUIET_MS
  local collapsed = state.is_collapsed(wid)
  local cols = layout.sidebar.width
  local seen = settled[wid]
  local pending = pending_adjust[wid]

  if pending then
    if pending.tab_id ~= tab_id then
      pending_adjust[wid] = nil
    elseif cols == pending.target then
      pending.reached_at = pending.reached_at or now
      if now - pending.reached_at < REMOTE_APPLY_MS then
        return false
      end
      pending_adjust[wid] = nil
    elseif now - pending.at < REMOTE_APPLY_MS then
      pending.reached_at = nil
      return false
    else
      pending_adjust[wid] = nil
    end
  end

  -- The divider moved, the tab did not, no frame of a window resize is landing and we asked for
  -- nothing: that is the user's hand, and it is theirs at once. Nothing fights a drag in progress,
  -- and wherever the hand comes off is the width from then on. A rail is never a preference.
  if same_shape(seen, tab_id, layout) and quiet and not collapsed and seen.cols ~= cols then
    adopted[wid] = fits(cols, layout.cols, nil, 1)
    adopted_for[wid] = cfg.width
    unreachable[wid] = nil
  end

  -- A rail is deliberately narrower than any sidebar, so the floor is the rail's own width.
  local want = int(M.desired(wid))
  local target = fits(want, layout.cols, collapsed and want or MIN_WIDTH, layout.bands)
  if target == cols then
    settled[wid] = record(tab_id, layout)
    unreachable[wid] = nil
    return false
  end

  local blocked = unreachable[wid]
  if
    blocked
    and blocked.tab_id == tab_id
    and blocked.tab_cols == layout.cols
    and blocked.target == target
    and blocked.cols == cols
    and (blocked.moved or now - blocked.at < BLOCKED_MS)
  then
    return false
  end

  -- Activating a pane sends focus in and out of the user's shells. Once per frame of a window
  -- resize that would flood an editor with focus events, so a tab that needs the dance is
  -- corrected when the frames have stopped; one that does not is corrected on every frame.
  local dance = needs_dance(layout, sb_id, tab_id, wid)
  if dance and not quiet then
    return false
  end
  local active = layout.active
  local dir, n = direction_for(cfg.position, target - cols), int(math.abs(target - cols))
  local adjust = act.AdjustPaneSize { dir, n }
  local action = adjust
  if dance then
    -- One assignment: WezTerm runs the three steps back to back on its own thread, so no pointer
    -- motion under `hover = "follow"` can move focus between them and send the adjust elsewhere.
    local steps = { act.ActivatePaneByIndex(layout.sidebar.index), adjust }
    if active then
      steps[#steps + 1] = act.ActivatePaneByIndex(active.index)
    end
    action = act.Multiple(steps)
  end
  if cfg.debug then
    util.log(
      "geometry: tab %d sidebar %d -> %d of %d cols%s",
      tab_id,
      cols,
      target,
      layout.cols,
      dance and " (dance)" or ""
    )
  end
  mux.call(gui_window, "perform_action", action, sb)

  if mux.domain(sb) ~= "local" then
    pending_adjust[wid] = { tab_id = tab_id, target = target, at = now }
    settled[wid] = nil
    return true
  end

  -- `perform_action` awaited the GUI, so the tree already shows what the adjust did.
  local after = read_layout(tab, sb_id, cfg.position)
  if not after or after.cols ~= layout.cols or after.shape ~= layout.shape then
    -- the tab changed shape under the adjust (another frame landed); the next call reads it afresh
    settled[wid] = nil
    return true
  end
  local landed = after.sidebar.width
  settled[wid] = record(tab_id, after)
  if landed == target then
    unreachable[wid] = nil
  elseif landed ~= cols then
    -- moved but not all the way: as far as this tab width lets the split go
    unreachable[wid] =
      { tab_id = tab_id, tab_cols = after.cols, target = target, cols = landed, moved = true, at = now }
  else
    local shift = content_shift(layout, after)
    if shift and active then
      -- The walk found an inner horizontal split before the root and moved the user's panes
      -- instead. Put them back the same way, and route this tab's adjusts through the sidebar.
      util.warn("geometry: adjust reached a content split in tab %d; undone", tab_id)
      -- preorder lists that split's first child before its second, so `shift` is the first child's
      local undo = act.AdjustPaneSize { first_child_direction(-shift), int(math.abs(shift)) }
      mux.call(gui_window, "perform_action", act.Multiple { act.ActivatePaneByIndex(active.index), undo }, sb)
      via_sidebar[wid] = via_sidebar[wid] or {}
      via_sidebar[wid][tab_id] = true
      settled[wid] = nil
      return true
    end
    -- did not move at all: the host declined, which a WezTerm overlay over the tab does
    unreachable[wid] =
      { tab_id = tab_id, tab_cols = after.cols, target = target, cols = landed, moved = false, at = now }
  end
  return true
end

---`wait` takes the window's gate; without it a correction that meets a mutation in flight is
---skipped, and the next poll or frame corrects anyway.
function M.correct(gui_window, wait, snapshot)
  local wid = gui_window:window_id()
  if wait then
    return gate.run(wid, "correct", correct, gui_window, snapshot)
  end
  return gate.try(wid, "correct", correct, gui_window, snapshot) == true
end

---Per-poll entry point: one reading of the active tab's tree, and an adjust only when it is off.
function M.sync(gui_window, snapshot)
  return M.correct(gui_window, nil, snapshot)
end

---What a correction would aim for right now, without acting; for probes and tests.
function M.plan(gui_window)
  local wid = gui_window:window_id()
  local tab = util.active_tab(gui_window)
  local sb = tab and sidebar.find(tab)
  local position = config.get().position
  local layout = sb and read_layout(tab, sb:pane_id(), position)
  if not layout then
    return nil
  end
  local collapsed = state.is_collapsed(wid)
  local want = int(M.desired(wid))
  return {
    cols = layout.sidebar.width,
    tab_cols = layout.cols,
    bands = layout.bands,
    want = want,
    target = fits(want, layout.cols, collapsed and want or MIN_WIDTH, layout.bands),
    at_edge = at_edge(layout, position),
    dance = needs_dance(layout, sb:pane_id(), mux.tab_id(tab), wid),
  }
end

---This window's bookkeeping, for probes and tests.
function M.inspect(window_id)
  return {
    desired = M.desired(window_id),
    adopted = adopted[window_id],
    settled = settled[window_id],
    unreachable = unreachable[window_id],
    resized_at = resized_at[window_id],
  }
end

return M

local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local gate = require "vtabs.gate"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local mux = require "vtabs.mux"
local link = require "vtabs.link"
local transport = require "vtabs.transport"
local util = require "vtabs.util"

---Holds the sidebar at its width against everything WezTerm does to a split. A window resize deals
---the sidebar half of every column it adds or removes (mux/src/tab.rs `adjust_x_size`), a divider
---drag moves it, a split or a close reshapes the tab around it. The tab's split tree, read through
---`panes_with_info`, is the source of truth: WezTerm updates it synchronously for each of those.
---
---On a local domain that is the whole story: a correction reads the tree, acts, and reads it again.
---On a mux domain the tree is a mirror. Every pane resize round-trips to the server, whose
---`TabResized` makes the client fetch the pane list and rebuild the mirror, sometimes to an
---intermediate state (wezterm-client/src/domain.rs `process_pane_list`). So on a mux domain the
---sidebar's own backend resizes the split on the server, once the frames have stopped -- the GUI's
---and the server's, which the sidebar reports as they land on it; the backend is told the width to
---land at and works the delta out against the server's own pane list, since a mirror can lag the
---server by any amount; the sidebar's own report of its width, not the mirror's reading, is the
---width corrected from; one adjust is in flight at a time, released by that report; and a divider
---is read as the user's hand only once it has sat still across a round trip, well clear of any
---window resize.
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
-- A mux mirror that reads the target is not proof the server has it until the sidebar says so;
-- without that report the width has to hold this long. A divider seen moving on a mux domain is
-- adopted only after sitting still this long, since a mirror rebuilt from a stale pane list moves it
-- the same way a hand does.
local REMOTE_APPLY_MS = 250
M.REMOTE_APPLY_MS = REMOTE_APPLY_MS
-- An unreported adjust stops blocking after this: a server that lost it is not waited on forever.
local REMOTE_WAIT_MS = 600
-- Two tab switches this close together are a held key: nothing is split or adjusted into a tab
-- that is only passing by, and the tab the hand stops on is served by the next poll.
local SWITCH_DWELL_MS = 150
M.SWITCH_DWELL_MS = SWITCH_DWELL_MS
-- Columns a window resize dealt are corrected, never adopted. On a mux domain the mirror can lag
-- the server past the quiet window, so a divider seen moving this soon after a frame is not a hand.
local HAND_AFTER_RESIZE_MS = 1000
M.HAND_AFTER_RESIZE_MS = HAND_AFTER_RESIZE_MS

---Declared through `store`, so forgetting a window clears them without a list to keep in step.
local scope = store.scope "geometry"
local adopted = scope.window()
local adopted_for = scope.window()
local rail_reserve = scope.window()
local resized_at = scope.window()
local resize_tick = scope.window()
-- The last size a mux pane reported: the server dealing a frame to it, which the GUI's frames do
-- not account for one by one, and which its own adjusts and a divider drag cause too.
local server_resized_at = scope.window()
-- The sidebar's own last word on its size: on a mux domain the one width that is no mirror's guess.
local reported = scope.window()
-- The layout a correction last left behind or found in order. The divider moving away from it with
-- the tab otherwise unchanged is the user's hand on it.
local settled = scope.window()
-- A target this tab cannot hold, or an adjust the host is refusing: not asked for again until the
-- tab changes shape, or the refusal has had time to clear.
local unreachable = scope.window()
-- Tabs whose adjust once walked into a content split: from then on it is issued from the sidebar.
local via_sidebar = scope.window()
-- The one adjust a mux domain has in flight, until the sidebar reports the width or time runs out.
local pending_adjust = scope.window()
-- The content pane whose focus a resize burst parked on the sidebar, to be handed back at settle.
local parked_focus = scope.window()
-- A divider seen moving on a mux domain, and since when it has sat still.
local moving = scope.window()
local switched_at = scope.window()
local rapid_until = scope.window()
-- Follow-up corrections armed per window and reason, one of each at a time.
local armed = scope.window()

---`panes_with_info` hands its numbers over as floats; `AdjustPaneSize` and `ActivatePaneByIndex`
---take only integers back, so every reading is whole from the start.
local function int(value)
  return math.floor(tonumber(value) or 0)
end

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

---Records one frame of a window resize and returns its tick, so only the settle timer armed by the
---last frame publishes.
function M.on_resize(window_id)
  resized_at[window_id] = util.now_ms()
  resize_tick[window_id] = (resize_tick[window_id] or 0) + 1
  return resize_tick[window_id]
end

function M.resize_tick(window_id)
  return resize_tick[window_id] or 0
end

---A size report from a pane on a mux domain: the server is still dealing columns to the tab,
---whatever the GUI's own frames say, so the burst goes on from here.
function M.on_server_resize(window_id)
  server_resized_at[window_id] = util.now_ms()
end

local function last_frame(window_id)
  return math.max(resized_at[window_id] or 0, server_resized_at[window_id] or 0)
end

---True while frames of a window resize are still arriving, from the GUI or from the server.
function M.in_burst(window_id)
  return util.now_ms() - last_frame(window_id) < RESIZE_QUIET_MS
end

function M.has_pending_adjust(window_id)
  return pending_adjust[window_id] ~= nil
end

---One tab switch. Two inside the dwell are a held key, and the window is left alone until it stops.
function M.on_switch(window_id)
  local now = util.now_ms()
  local last = switched_at[window_id]
  if last and now - last < SWITCH_DWELL_MS then
    rapid_until[window_id] = now + SWITCH_DWELL_MS
  end
  switched_at[window_id] = now
end

---True while tabs are being switched through faster than the dwell.
function M.switching(window_id)
  return (rapid_until[window_id] or 0) > util.now_ms()
end

---The sidebar reporting its own size is the one word from the server side of a mux link. It is
---the width the next correction reads, and for the adjust in flight it is the answer: landed, so
---the next one need not wait for the mirror's timer, or elsewhere. Elsewhere in the adjust's own
---answer (`answered`) is as far as the server let the split go, not asked for again until
---something changes; elsewhere in a plain size report is something else moving the split, and
---the next correction reads it afresh.
function M.landed(window_id, pane_id, cols, rows, answered)
  cols = tonumber(cols)
  if cols == nil then
    return
  end
  local now = util.now_ms()
  reported[window_id] = { pane_id = pane_id, cols = int(cols), rows = tonumber(rows) and int(rows) or nil, at = now }
  local pending = pending_adjust[window_id]
  if not pending or pending.pane_id ~= pane_id then
    return
  end
  if cols == pending.target then
    pending.reported = true
    return
  end
  if answered then
    unreachable[window_id] =
      { tab_id = pending.tab_id, target = pending.target, cols = int(cols), remote = true, at = now }
  end
  pending_adjust[window_id] = nil
end

---The sidebar's last report of its own size, for probes and tests.
function M.reported(window_id)
  return reported[window_id]
end

---The server refused the adjust in flight (its `cli` answer said so): nothing is coming, so the
---next reading may ask again at once.
function M.abandon(window_id)
  pending_adjust[window_id] = nil
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

---The width a new sidebar is split off at: the window's own width, so the pane needs no adjust the
---moment it appears, clamped to what the tab can hold.
function M.attach_width(window_id, tab)
  local size = mux.call(tab, "get_size")
  local tab_cols = type(size) == "table" and tonumber(size.cols) or nil
  local want = int(M.desired(window_id))
  local collapsed = state.is_collapsed(window_id)
  return fits(want, tab_cols, collapsed and want or MIN_WIDTH, 1)
end

---`AdjustPaneSize` shifts the split node's FIRST child: `Right` by `+n`, `Left` by `-n`.
local function first_child_direction(delta)
  return delta > 0 and "Right" or "Left"
end

---A left sidebar is the root split's first child and grows with `Right`; a right one is its second.
local function direction_for(position, delta)
  return first_child_direction(position == "left" and delta or -delta)
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

local function content_rect(layout, id)
  for _, rect in ipairs(layout.content) do
    if rect.id == id then
      return rect
    end
  end
  return nil
end

---`cols` and `rows` are the width and height the correction settled at, whichever source read them.
local function record(tab_id, layout, cols, rows)
  return {
    tab_id = tab_id,
    tab_cols = layout.cols,
    cols = cols,
    rows = rows,
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

---Arms one follow-up correction per window and reason. The waits on a mux domain must not depend
---on a poll arriving: WezTerm re-arms `update-status` only after a title update, so a window with
---nothing printing in it can go without one for as long as the hand is still.
local function follow_up(gui_window, wid, key, ms)
  local keys = armed[wid]
  if not keys then
    keys = {}
    armed[wid] = keys
  end
  if keys[key] then
    return
  end
  keys[key] = true
  wezterm.time.call_after(ms / 1000, function()
    local live = armed[wid]
    if live then
      live[key] = nil
    end
    local ok, err = pcall(M.correct, gui_window)
    if not ok and not util.window_gone(err) then
      util.warn("geometry: %s", tostring(err))
    end
  end)
end

---Whether the one adjust a mux domain has in flight still blocks the next; clears it when the
---sidebar has reported the width and the mirror agrees, or when the mirror has held it long enough.
local function still_pending(gui_window, wid, tab_id, cols, now)
  local pending = pending_adjust[wid]
  if not pending then
    return false
  end
  if pending.tab_id ~= tab_id then
    pending_adjust[wid] = nil
    return false
  end
  if cols == pending.target then
    if not pending.reported then
      pending.reached_at = pending.reached_at or now
      if now - pending.reached_at < REMOTE_APPLY_MS then
        follow_up(gui_window, wid, "pending", REMOTE_APPLY_MS + 20)
        return true
      end
    end
    pending_adjust[wid] = nil
    return false
  end
  if now - pending.at < REMOTE_WAIT_MS then
    pending.reached_at = nil
    follow_up(gui_window, wid, "pending", REMOTE_WAIT_MS - (now - pending.at) + 20)
    return true
  end
  pending_adjust[wid] = nil
  return false
end

---On a mux domain a divider that moved is adopted once it has sat still across a round trip; on a
---local domain at once. True when the width is the user's to keep from here on.
local function hand_has_settled(gui_window, wid, tab_id, cols, remote, now)
  if not remote then
    return true
  end
  local hand = moving[wid]
  if hand and now - hand.at >= REMOTE_APPLY_MS then
    return true
  end
  if not hand then
    moving[wid] = { tab_id = tab_id, cols = cols, at = now }
  end
  follow_up(gui_window, wid, "hand", REMOTE_APPLY_MS + 20)
  return false
end

---Re-asserts the sidebar width on the active tab; background tabs are corrected once they activate.
local function correct(gui_window, snapshot)
  local cfg = snapshot and snapshot.cfg or config.get()
  local wid = snapshot and snapshot.window_id or gui_window:window_id()
  if store.drag[wid] or M.switching(wid) then
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
  local now = util.now_ms()
  local quiet = now - last_frame(wid) >= RESIZE_QUIET_MS
  local remote = mux.domain(sb) ~= "local"
  if remote and not quiet then
    -- The server is still dealing columns to the tab: nothing the mirror says meanwhile is worth an
    -- adjust, and the follow-up reads it afresh once the reports have stopped.
    follow_up(gui_window, wid, "quiet", RESIZE_QUIET_MS + 20)
    return false
  end
  -- Read afresh even under a snapshot: one taken before this poll's last frame would hand the
  -- adjust a stale delta, and the tree is one cheap call away.
  local layout = read_layout(tab, sb_id, cfg.position)
  if not layout or layout.zoomed or not at_edge(layout, cfg.position) then
    return false
  end
  local collapsed = state.is_collapsed(wid)
  local cols = layout.sidebar.width
  local report = reported[wid]
  if not (remote and report and report.pane_id == sb_id) then
    report = nil
  end
  if report then
    -- The pane's own word over the mirror's: a mirror rebuilt from a stale pane list, or frozen
    -- on one, reads any width at all, and an adjust computed from it lands anywhere.
    cols = report.cols
  end
  if still_pending(gui_window, wid, tab_id, cols, now) then
    return false
  end

  -- A burst parked the content pane's focus on the sidebar; once the frames stop it goes back,
  -- unless the user has already moved it elsewhere or the pane has gone.
  local parked = parked_focus[wid]
  if parked and parked.tab_id ~= tab_id then
    parked_focus[wid] = nil
    parked = nil
  end
  local restore = nil
  if parked and quiet then
    parked_focus[wid] = nil
    local rect = layout.active and layout.active.id == sb_id and content_rect(layout, parked.id) or nil
    restore = rect and rect.index or nil
  end

  -- The divider moved, the tab did not, no frame of a window resize is landing and we asked for
  -- nothing: that is the user's hand. Nothing fights a drag in progress, and wherever the hand
  -- comes off is the width from then on. A rail is never a preference.
  local seen = settled[wid]
  local hand = moving[wid]
  if hand and (hand.tab_id ~= tab_id or hand.cols ~= cols) then
    -- moved on, or another tab: the wait starts over from whatever this reading says
    moving[wid] = nil
    hand = nil
  end
  -- A mux mirror rebuilt mid-way through a server-side adjust reads as a tab of another width for
  -- one poll; a hand already seen holding this width is not made to start over by it.
  local moved = hand ~= nil or (same_shape(seen, tab_id, layout) and seen.cols ~= cols)
  if moved and remote and not hand then
    -- Dealt by a window resize the mirror has not caught up with: a frame within the last second,
    -- or a height the report says changed, is no hand on the divider.
    local resized = now - (resized_at[wid] or 0) < HAND_AFTER_RESIZE_MS
    local taller = report and seen.rows and report.rows and report.rows ~= seen.rows
    if resized or taller then
      moved = false
    end
  end
  if moved and quiet and not collapsed then
    if not hand_has_settled(gui_window, wid, tab_id, cols, remote, now) then
      return false
    end
    adopted[wid] = fits(cols, layout.cols, nil, 1)
    adopted_for[wid] = cfg.width
    unreachable[wid] = nil
    moving[wid] = nil
  end

  -- A rail is deliberately narrower than any sidebar, so the floor is the rail's own width.
  local want = int(M.desired(wid))
  local target = fits(want, layout.cols, collapsed and want or MIN_WIDTH, layout.bands)
  if target == cols then
    settled[wid] = record(tab_id, layout, cols, report and report.rows or nil)
    unreachable[wid] = nil
    if restore then
      mux.call(gui_window, "perform_action", act.ActivatePaneByIndex(restore), sb)
    end
    return false
  end

  local blocked = unreachable[wid]
  if blocked and blocked.tab_id == tab_id and blocked.target == target and blocked.cols == cols then
    if blocked.remote then
      -- the server's own answer to this very ask; the tab may give more once it has changed
      if now - blocked.at < BLOCKED_MS then
        return false
      end
    elseif blocked.tab_cols == layout.cols and (blocked.moved or now - blocked.at < BLOCKED_MS) then
      return false
    end
  end

  local dir, n = direction_for(cfg.position, target - cols), int(math.abs(target - cols))
  if remote then
    -- The server's tree is the truth a mux GUI mirrors, and every pane resize the GUI sends makes
    -- the client fetch the pane list and rebuild the whole mirror; a correction issued here would
    -- cost one such rebuild per pane and echo back stale widths. So the sidebar's own backend
    -- resizes the split on the server: one change there, one rebuild, no echo. Once per settle,
    -- not per frame -- during a burst the frames alone already cost a rebuild per pane. The
    -- backend is told the width to land at and reads its own from the server's list: the delta
    -- here comes from a mirror, and a mirror can lag the server by any amount.
    if link.busy(mux.domain(sb)) and transport.state(sb) ~= "active" then
      -- the mirror rebuilds are still crossing the link the adjust would take; asked from a fresh
      -- reading once it is quiet. A sidebar on its inbox is asked at once: nothing crosses.
      follow_up(gui_window, wid, "link", link.QUIET_MS + 20)
      return false
    end
    if cfg.debug then
      util.log("geometry: tab %d sidebar %d -> %d of %d cols (server)", tab_id, cols, target, layout.cols)
    end
    local message = { t = "adjust", target = target, min_content = MIN_CONTENT }
    if not sidebar.send(sb, message) then
      return false
    end
    pending_adjust[wid] = { tab_id = tab_id, pane_id = sb_id, target = target, at = now }
    settled[wid] = nil
    follow_up(gui_window, wid, "pending", REMOTE_WAIT_MS + 20)
    return true
  end

  local active = layout.active
  local dance = needs_dance(layout, sb_id, tab_id, wid)
  local adjust = act.AdjustPaneSize { dir, n }
  local action = adjust
  local steps = nil
  if dance then
    -- One assignment: WezTerm runs the steps back to back on its own thread, so no pointer motion
    -- under `hover = "follow"` can move focus between them and send the adjust elsewhere.
    steps = { act.ActivatePaneByIndex(layout.sidebar.index), adjust }
    if quiet then
      if active then
        steps[#steps + 1] = act.ActivatePaneByIndex(active.index)
      end
    elseif active then
      -- Every activation sends focus events to the shells involved. Once per frame of a window
      -- resize that would flood an editor, so the focus stays parked on the sidebar for the burst
      -- and every further frame is corrected from there; the settle hands it back.
      parked_focus[wid] = { tab_id = tab_id, id = active.id }
    end
  elseif restore then
    steps = { adjust, act.ActivatePaneByIndex(restore) }
  end
  if steps then
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

  -- `perform_action` awaited the GUI, so the tree already shows what the adjust did.
  local after = read_layout(tab, sb_id, cfg.position)
  if not after or after.cols ~= layout.cols or after.shape ~= layout.shape then
    -- the tab changed shape under the adjust (another frame landed); the next call reads it afresh
    settled[wid] = nil
    return true
  end
  local landed = after.sidebar.width
  settled[wid] = record(tab_id, after, landed, nil)
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
    pending = pending_adjust[window_id],
    parked = parked_focus[window_id],
    moving = moving[window_id],
    resized_at = resized_at[window_id],
    server_resized_at = server_resized_at[window_id],
    reported = reported[window_id],
    switching = M.switching(window_id),
  }
end

return M

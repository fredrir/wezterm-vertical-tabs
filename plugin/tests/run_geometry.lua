local H = require "support.helpers"
local wezterm = require "wezterm"
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local geometry = require "vtabs.geometry"
local fake = require "fake_mux"

local test, eq, later, attach_all = H.test, H.eq, H.later, H.attach_all
local mark_ready, window = H.mark_ready, H.window

local BACKEND = { path = "/bin/wez-vtabs" }

local function last_action(win)
  return win.actions[#win.actions].action
end

---The one `AdjustPaneSize` inside an assignment, whether it was issued bare or as a dance step.
local function adjust_of(action)
  if action.action == "AdjustPaneSize" then
    return action
  end
  for _, step in ipairs(action.action == "Multiple" and action.arg or {}) do
    if step.action == "AdjustPaneSize" then
      return step
    end
  end
  return nil
end

local function adjusts(win, from)
  local n = 0
  for i = (from or 0) + 1, #win.actions do
    if adjust_of(win.actions[i].action) then
      n = n + 1
    end
  end
  return n
end

local function spy_activate(pane)
  local calls = 0
  pane.activate = function(self)
    calls = calls + 1
    return getmetatable(self).activate(self)
  end
  return function()
    pane.activate = nil
    return calls
  end
end

---One tab, its sidebar attached and authenticated, with the width verified once so a later move of
---the divider is measured against a known layout.
local function settled_tab()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  return win, gui, tab, sb
end

test("AdjustPaneSize Right adds delta to split first.cols, Left subtracts (tab.rs:1294; headless 28-33-28)", function()
  local win = fake.window(80)
  local tab = win:add_tab { title = "g" }
  tab.pane_list[1]:split { direction = "Left", top_level = true, size = 28 }
  eq(tab.pane_list[1].cols, 28)
  eq(tab.pane_list[2].cols, 51)
  win.gui:perform_action(wezterm.action.AdjustPaneSize { "Right", 5 }, tab.pane_list[1])
  eq(tab.pane_list[1].cols, 33)
  eq(tab.pane_list[2].cols, 46)
  win.gui:perform_action(wezterm.action.AdjustPaneSize { "Left", 5 }, tab.pane_list[1])
  eq(tab.pane_list[1].cols, 28)
end)

test("window growth deals the sidebar half the columns; correct claws them back in one AdjustPaneSize", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  eq(sb.cols, 28)
  win:resize(40)
  eq(sb.cols, 48, "adjust_x_size gave the sidebar half of the delta")
  local before = #win.actions
  assert(geometry.correct(gui), "correction ran")
  eq(#win.actions - before, 1, "exactly one action")
  eq(last_action(win).action, "AdjustPaneSize")
  eq(last_action(win).arg[1], "Left")
  eq(last_action(win).arg[2], 20)
  eq(math.type(last_action(win).arg[2]), "integer", "WezTerm refuses a float amount: the tree's floats never reach it")
  eq(sb.cols, 28)
  eq(geometry.correct(gui), false, "second pass is a no-op")
end)

test("split Left puts the sidebar in first, split Right in second, so a right sidebar grows with Left", function()
  config.setup { position = "right", backend = BACKEND }
  local win = fake.window(80)
  win:add_tab { title = "r" }
  local gui = win.gui
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(sb.split_args.direction, "Right")
  eq(sb.cols, 28)
  win:resize(-20)
  eq(sb.cols, 18)
  assert(geometry.correct(gui), "correction ran")
  eq(last_action(win).arg[1], "Left")
  eq(last_action(win).arg[2], 10)
  eq(sb.cols, 28)
  config.setup { backend = BACKEND }
end)

test("a divider drag is adopted the moment it moves, and followed wherever the hand goes", function()
  local win, gui, tab, sb = settled_tab()
  local wid = gui:window_id()
  local before = #win.actions
  tab:set_split(34)
  eq(geometry.correct(gui), false, "the hand on the divider is not fought")
  eq(geometry.desired(wid), 34, "and where it is now is the width")
  tab:set_split(40)
  eq(geometry.correct(gui), false)
  eq(geometry.desired(wid), 40, "every move of the drag is followed")
  eq(#win.actions, before, "no AdjustPaneSize at any point")
  eq(sb.cols, 40)
end)

test("a divider drag survives a config reload, unless the reload changed width itself", function()
  local win, gui, tab, sb = settled_tab()
  local wid = gui:window_id()
  tab:set_split(34)
  eq(geometry.correct(gui), false)
  eq(geometry.desired(wid), 34)
  -- Every edit to wezterm.lua reloads, and the plugin watches its own files too, so a reload that
  -- says nothing about the width must not throw the drag away.
  geometry.reset(wid)
  eq(geometry.desired(wid), 34, "an unrelated reload keeps it")
  eq(geometry.correct(gui), false, "and nothing is re-asserted")
  config.setup { width = 30, backend = BACKEND }
  geometry.reset(wid)
  eq(geometry.desired(wid), 30, "changing width itself drops it")
  assert(geometry.correct(gui))
  eq(sb.cols, 30)
  eq(#win.actions > 0, true)
  config.setup { backend = BACKEND }
end)

test("every frame of a window resize is corrected at once; the settle timer has nothing left to do", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  local view = require "vtabs.view"
  local clock = H.clock()
  wezterm.timers = {}
  local before = #win.actions
  for i = 1, 8 do
    win:resize(2)
    eq(sb.cols, 29, "the frame dealt the sidebar a column")
    view.on_resize(gui)
    eq(sb.cols, 28, "frame " .. i .. " is corrected before the next one arrives")
  end
  eq(adjusts(win, before), 8, "one adjust per frame")
  eq(#wezterm.timers, 8, "one settle timer per frame")
  clock.advance(geometry.SETTLE_MS)
  wezterm.fire_timers()
  eq(adjusts(win, before), 8, "the last frame's timer found the width in order")
  eq(sb.cols, 28)
  clock.restore()
end)

test("a column dealt during a window drag is corrected on that frame, never adopted", function()
  local win, gui, _, sb = settled_tab()
  local wid = gui:window_id()
  geometry.on_resize(wid)
  win:resize(30)
  assert(geometry.correct(gui), "the frame is corrected")
  eq(sb.cols, 28)
  eq(geometry.desired(wid), 28, "the deal was wezterm's, not the user's")
  later(800, function()
    eq(geometry.correct(gui), false, "nothing to do once the frames stop")
  end)
  eq(geometry.desired(wid), 28)
end)

test("a frame that lands while an adjust is in flight is corrected by the next call", function()
  local win, gui, _, sb = settled_tab()
  local real = getmetatable(gui).perform_action
  gui.perform_action = function(self, action, pane)
    -- the GUI takes the assignment after another frame dealt the sidebar one more column
    win:resize(2)
    return real(self, action, pane)
  end
  win:resize(10)
  assert(geometry.correct(gui), "corrected from the columns it read")
  eq(sb.cols, 29, "and landed one off, the frame having moved the tab under it")
  gui.perform_action = nil
  assert(geometry.correct(gui), "the next call reads the tree afresh")
  eq(sb.cols, 28)
end)

---Frames the sidebar's backend was asked to resize the split with, server-side.
local function adjusts_sent(sb, from)
  local n = 0
  for i = (from or 0) + 1, #sb.sent do
    if sb.sent[i]:find('"t":"adjust"', 1, true) then
      n = n + 1
    end
  end
  return n
end

test("a remote correction stays singular until the mux applies it", function()
  local win, gui, tab, sb = settled_tab()
  local clock = H.clock()
  local content = sidebar.content_pane(tab)
  sb.domain, content.domain = "e2emux", "e2emux"
  fake.remote_lag = true
  local before_actions, before_sent = #win.actions, #sb.sent

  win:resize(40)
  assert(geometry.correct(gui), "the first remote correction is sent")
  eq(adjusts_sent(sb, before_sent), 1, "to the sidebar's own backend, which resizes the split on the server")
  eq(#win.actions, before_actions, "nothing through the GUI's mirror")
  eq(sb.cols, 48, "the fake remote mux has not applied it yet")
  eq(geometry.correct(gui), false, "a poll cannot enqueue the same delta again")
  eq(adjusts_sent(sb, before_sent), 1, "only one remote adjustment is in flight")

  assert(fake.apply_remote(sb), "the remote mux eventually applies the queued correction")
  eq(sb.cols, 28)
  eq(geometry.correct(gui), false, "the observed target starts a stability window")
  clock.advance(geometry.REMOTE_APPLY_MS)
  eq(geometry.correct(gui), false, "the stable target clears the in-flight record")
  fake.remote_lag = false
  clock.restore()
end)

test("stacked content is one band and spans the content column: corrected with no dance at all", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  local extra = fake.pane(tab, { cols = content.cols })
  content.height, extra.top, extra.height = 12, 13, 11
  tab.pane_list[#tab.pane_list + 1] = extra
  extra:activate()
  local activations = spy_activate(sb)
  win:resize(10)
  assert(geometry.correct(gui), "correction ran")
  eq(last_action(win).action, "AdjustPaneSize", "issued bare, from the content leaf")
  eq(activations(), 0)
  eq(tab.active, extra, "focus never moved")
  eq(sb.cols, 28)
end)

test("side-by-side content corrects in one atomic dance once the frames have stopped", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  local extra = fake.pane(tab, { cols = 28 })
  tab.pane_list[#tab.pane_list + 1] = extra
  extra:activate()
  win:resize(10)
  eq(sb.cols, 33)
  content.left, content.width = 34, 27
  extra.left, extra.width = 62, 28
  local activations = spy_activate(sb)
  assert(geometry.correct(gui), "corrected")
  local action = last_action(win)
  eq(action.action, "Multiple", "one assignment: nothing can land between its steps")
  eq(action.arg[1].action, "ActivatePaneByIndex")
  eq(action.arg[1].arg, 0, "the sidebar, whose nearest horizontal split is the root")
  eq(math.type(action.arg[1].arg), "integer", "an index WezTerm accepts")
  eq(action.arg[2].action, "AdjustPaneSize")
  eq(action.arg[2].arg[1], "Left")
  eq(action.arg[2].arg[2], 5)
  eq(action.arg[3].action, "ActivatePaneByIndex")
  eq(action.arg[3].arg, 2, "and the pane that had focus gets it back")
  eq(activations(), 0, "no pane:activate() of its own, which the pointer could undo")
  eq(tab.active, extra, "focus restored")
  eq(sb.cols, 28)
end)

test(
  "through a resize burst, side-by-side content parks its focus on the sidebar and gets it back at settle",
  function()
    local win, gui = window(1)
    sidebar.ensure(gui)
    local tab = win.tab_list[1]
    local sb = mark_ready(tab)
    local wid = gui:window_id()
    local content = sidebar.content_pane(tab)
    local extra = fake.pane(tab, { cols = 28 })
    tab.pane_list[#tab.pane_list + 1] = extra
    extra:activate()
    win:resize(10)
    eq(sb.cols, 33)
    content.left, content.width = 34, 27
    extra.left, extra.width = 62, 28
    local activations = spy_activate(sb)
    geometry.on_resize(wid)
    assert(geometry.correct(gui), "the frame is corrected at once")
    local action = last_action(win)
    eq(action.action, "Multiple")
    eq(action.arg[1].action, "ActivatePaneByIndex")
    eq(action.arg[1].arg, 0)
    eq(action.arg[2].action, "AdjustPaneSize")
    eq(#action.arg, 2, "focus stays parked on the sidebar while the frames keep coming")
    eq(tab.active, sb)
    eq(sb.cols, 28)
    -- the next frame is corrected from the sidebar, which already holds focus: no activation at all
    win:resize(2)
    geometry.on_resize(wid)
    assert(geometry.correct(gui))
    eq(last_action(win).action, "AdjustPaneSize", "bare: one focus event pair per burst, not per frame")
    eq(sb.cols, 28)
    later(200, function()
      eq(geometry.correct(gui), false, "nothing left to correct once the frames stop")
    end)
    eq(last_action(win).action, "ActivatePaneByIndex", "and focus goes back")
    eq(last_action(win).arg, 2)
    eq(tab.active, extra, "to the pane that had it")
    eq(activations(), 0, "no pane:activate() of its own, which the pointer could undo")
  end
)

test("a parked focus is not handed back to a pane the user has meanwhile left or closed", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  local content = sidebar.content_pane(tab)
  local extra = fake.pane(tab, { cols = 28 })
  tab.pane_list[#tab.pane_list + 1] = extra
  extra:activate()
  win:resize(10)
  content.left, content.width = 34, 27
  extra.left, extra.width = 62, 28
  geometry.on_resize(wid)
  assert(geometry.correct(gui))
  eq(tab.active, sb, "parked")
  -- the user clicks the other content pane before the frames stop
  content:activate()
  local before = #win.actions
  later(200, function()
    eq(geometry.correct(gui), false)
  end)
  eq(#win.actions, before, "the hand-back is not forced over the user's own choice")
  eq(tab.active, content)
end)

test("a content pane as wide as the content column reaches the root split, beside narrower neighbours", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  local a = sidebar.content_pane(tab)
  local b = fake.pane(tab, { cols = 23 })
  local c = fake.pane(tab, { cols = 46 })
  tab.pane_list[#tab.pane_list + 1] = b
  tab.pane_list[#tab.pane_list + 1] = c
  -- VSplit(HSplit(a, b), c): a and b share the top, c has the whole content width beneath them
  tab:set_split(33)
  a.left, a.width, a.height = 34, 22, 12
  b.left, b.width, b.height = 57, 23, 12
  c.left, c.width, c.top, c.height = 34, 46, 13, 11
  c:activate()
  geometry.reset(wid)
  local plan = geometry.plan(gui)
  eq(plan.bands, 2)
  eq(plan.dance, false, "c's path to the root has no horizontal split")
  assert(geometry.correct(gui))
  eq(last_action(win).action, "AdjustPaneSize", "issued bare, from c")
  eq(tab.active, c)
  eq(sb.cols, 28)
end)

test("an adjust that walks into a content split is undone, and that tab is corrected from the sidebar", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  local a = sidebar.content_pane(tab)
  local b = fake.pane(tab, { cols = 46 })
  tab.pane_list[#tab.pane_list + 1] = b
  tab:set_split(33)
  -- a spans the content column, so the adjust is issued from it; this host walks it somewhere else
  a.left, a.width, a.height = 34, 46, 12
  b.left, b.width, b.top, b.height = 34, 46, 13, 11
  a:activate()
  local misrouted = 0
  gui.perform_action = function(_, action, pane)
    win.actions[#win.actions + 1] = { action = action, pane = pane }
    local steps = action.action == "Multiple" and action.arg or { action }
    for _, step in ipairs(steps) do
      if step.action == "ActivatePaneByIndex" then
        tab.active = tab.pane_list[step.arg + 1]
      elseif tab.active == a then
        misrouted = misrouted + 1
        a.width = a.width + (step.arg[1] == "Right" and step.arg[2] or -step.arg[2])
      else
        tab:adjust_from_active(step.arg[1], step.arg[2])
      end
    end
  end
  geometry.reset(wid)
  assert(geometry.correct(gui), "the adjust was issued")
  eq(a.width, 46, "and undone the moment the sidebar was seen not to move")
  eq(misrouted, 2, "one wrong walk, one exact reversal")
  eq(sb.cols, 33, "the sidebar untouched so far")
  assert(geometry.correct(gui), "the retry goes through the sidebar")
  eq(last_action(win).action, "Multiple")
  eq(last_action(win).arg[1].arg, 0)
  eq(sb.cols, 28)
  eq(tab.active, a, "focus handed back")
  eq(misrouted, 2, "and never through a content leaf again in this tab")
  gui.perform_action = nil
end)

test("the adjust stays in-process even where the cli is usable", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  content:activate()
  win:resize(40)
  eq(sb.cols, 48)
  local spawned = #wezterm.spawned
  H.with_cli(function()
    assert(geometry.correct(gui), "correction ran")
  end)
  eq(#wezterm.spawned, spawned, "no wezterm cli process for a width")
  eq(last_action(win).action, "AdjustPaneSize")
  eq(tab.active, content)
  eq(sb.cols, 28)
end)

test("a sidebar that only carries the title marker is never resized", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  win:resize(20)
  local before = #win.actions
  eq(geometry.correct(gui), false, "an unauthenticated pane is not corrected")
  eq(#win.actions, before, "no AdjustPaneSize")
  eq(sb.cols, 38)
  mark_ready(tab)
  assert(geometry.correct(gui), "the same pane is corrected once it echoes its token")
  eq(sb.cols, 28)
end)

test("a zoomed pane suspends adoption and correction until it is unzoomed", function()
  local win, gui, tab, sb = settled_tab()
  sb.zoomed = true
  sb.cols = tab:width()
  local before = #win.actions
  eq(geometry.correct(gui), false, "zoom is not a divider drag")
  eq(#win.actions, before, "no AdjustPaneSize while zoomed")
  eq(geometry.desired(gui:window_id()), 28, "full-window zoom width not latched")
  sb.zoomed = false
  sb.cols = 28
  eq(geometry.correct(gui), false, "unzoom restores the width it had")
  eq(geometry.desired(gui:window_id()), 28)
end)

test("an adopted width is clamped to a plausible sidebar, and the clamp is applied at once", function()
  local win, gui, tab, sb = settled_tab()
  local wid = gui:window_id()
  tab:set_split(78)
  assert(geometry.correct(gui), "the drag is adopted, and its clamp driven")
  eq(geometry.desired(wid), 60, "clamped to tab width minus the content margin")
  eq(sb.cols, 60)
  eq(geometry.correct(gui), false)
  tab:set_split(1)
  assert(geometry.correct(gui))
  eq(geometry.desired(wid), 8, "clamped to the minimum width")
  eq(sb.cols, 8)
  eq(#win.actions > 0, true)
end)

test("an unreachable width is driven as far as the split allows, then left until the tab changes shape", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  win:resize(-71)
  eq(tab:width(), 9, "a window too narrow to hold the sidebar")
  assert(geometry.correct(gui), "one attempt moves it as far as the split allows")
  eq(sb.cols, 7, "clamped to width - 2")
  local settled = #win.actions
  for _, at in ipairs { 0, 1000, 5000 } do
    later(at, function()
      eq(geometry.correct(gui), false, "left alone at " .. at .. " ms")
    end)
  end
  eq(#win.actions, settled, "no AdjustPaneSize and no activate once it is known unreachable")
  win:resize(40)
  assert(geometry.correct(gui), "a window resize unblocks the retry")
  eq(sb.cols, 28)
end)

test("a width we drove to is never adopted, however long it sits there", function()
  local win, gui, _, sb = settled_tab()
  local wid = gui:window_id()
  win:resize(-71)
  assert(geometry.correct(gui))
  eq(sb.cols, 7, "as far as the split allows, not the target")
  for _, at in ipairs { 300, 3000, 30000 } do
    later(at, function()
      geometry.correct(gui)
    end)
    eq(geometry.desired(wid), 28, "still cfg.width after " .. at .. " ms at the clamp")
  end
  win:resize(40)
  assert(geometry.correct(gui))
  eq(sb.cols, 28, "and cfg.width is what the room is used for once it comes back")
end)

test("two content panes side by side each keep MIN_CONTENT, not 20 columns between them", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  config.setup { meta = "auto", width = 40, backend = BACKEND }
  win:resize(73 - win.cols)
  geometry.reset(wid)
  assert(geometry.correct(gui))
  eq(sb.cols, 40, "one shell can give the sidebar most of a 73-column tab")

  -- A second shell beside the first: with the guard charged once, the sidebar would have kept its
  -- 40 and the two shells been dealt 20 and 11.
  local content = sidebar.content_pane(tab)
  content.left, content.width = 41, 20
  local beside = fake.pane(tab, { cols = 11 })
  beside.left, beside.width = 62, 11
  tab.pane_list[#tab.pane_list + 1] = beside
  geometry.reset(wid)
  assert(geometry.correct(gui))
  eq(sb.cols, 33, "the second band costs the sidebar width, and leaves 20 columns for each of them")

  -- Stacked panes share one band, so they cost nothing extra.
  content.width = 32
  beside.left, beside.width, beside.top = 41, 32, 12
  geometry.reset(wid)
  assert(geometry.correct(gui))
  eq(sb.cols, 40, "a pane stacked under another shares its column band")
  config.setup { backend = BACKEND }
end)

test("a single-content tab adjusts from the content leaf, with nothing activated", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  eq(#sidebar.classify(tab), 1, "one content pane: the root split is its nearest horizontal node")
  content:activate()
  geometry.reset(gui:window_id())
  tab:set_split(18)
  local seen = nil
  local real = getmetatable(gui).perform_action
  gui.perform_action = function(self, action, pane)
    if action.action == "AdjustPaneSize" then
      seen = tab.active
    end
    return real(self, action, pane)
  end
  assert(geometry.correct(gui), "the width is corrected")
  eq(seen, content, "from the content leaf, which the divider sits right above")
  eq(tab.active, content, "and focus never moved")
  eq(sb.cols, 28)
  gui.perform_action = nil
end)

test("a width the content bands clamp is still the width the user asked for, not a new one", function()
  local win, gui, tab, sb = settled_tab()
  local wid = gui:window_id()
  local content = sidebar.content_pane(tab)
  -- The user drags the divider out to 40 and it is adopted, with one content band.
  tab:set_split(40)
  eq(geometry.correct(gui), false, "adopted as it moves")
  eq(geometry.desired(wid), 40, "the drag is theirs")

  -- More bands beside it: 40 no longer leaves each of them MIN_CONTENT, so the width is clamped.
  -- That is a clamp, not an adoption -- what they asked for is remembered, not rewritten.
  content.left, content.width = 41, 21
  for i = 1, 2 do
    local beside = fake.pane(tab, { cols = 8 })
    tab.pane_list[#tab.pane_list + 1] = beside
    beside.left, beside.width = 63 + (i - 1) * 9, 8
  end
  assert(geometry.correct(gui), "the clamp is driven the moment the bands appear")
  eq(sb.cols, 20, "the sidebar is where the bands leave room for it")
  eq(geometry.desired(wid), 40, "the clamp does not overwrite the width they asked for")
  -- The width we clamped it to sits still with nothing outstanding: never a hand on the divider.
  for _, at in ipairs { 2000, 3000, 8000 } do
    later(at, function()
      eq(geometry.correct(gui), false, "nothing to do at " .. at .. " ms")
    end)
    eq(geometry.desired(wid), 40, "still theirs after " .. at .. " ms at the clamp")
  end

  -- Close them and the width they asked for is what the plugin goes back to.
  table.remove(tab.pane_list, #tab.pane_list)
  table.remove(tab.pane_list, #tab.pane_list)
  content.left, content.width = nil, nil
  assert(geometry.correct(gui), "the room came back, so the width is pursued")
  eq(sb.cols, 40, "driven back to what they dragged to")
  eq(geometry.desired(wid), 40, "20 was our clamp, never their preference")
  eq(#win.actions > 0, true)
end)

test("a font or dpi change is corrected, not adopted as a divider drag", function()
  local win, gui, tab, sb = settled_tab()
  for _, p in ipairs(tab:panes()) do
    p.cell_width = 14
  end
  tab:set_split(20)
  assert(geometry.correct(gui), "a wider cell is not the user dragging the divider")
  eq(geometry.desired(gui:window_id()), 28, "cfg.width still wins")
  eq(sb.cols, 28)
  eq(#win.actions > 0, true)
end)

test("a pane split into the sidebar's column changes the tab's shape, so its width is not adopted", function()
  local _, gui, tab, sb = settled_tab()
  local wid = gui:window_id()
  -- SplitVertical with the sidebar active: the sidebar keeps its columns but loses half its rows
  local intruder = fake.pane(tab, { cols = 28 })
  intruder.left, intruder.top, intruder.height, intruder.width = 0, 13, 11, 28
  sb.height = 12
  table.insert(tab.pane_list, 2, intruder)
  tab:set_split(20)
  assert(geometry.correct(gui), "the new shape is corrected, not read as a drag")
  eq(geometry.desired(wid), 28)
  eq(sb.cols, 28)
end)

test("a pane moved elsewhere in the tab widens the sidebar's column without a hand on the divider", function()
  local win, gui, tab, sb = settled_tab()
  local wid = gui:window_id()
  local content = sidebar.content_pane(tab)
  -- A split into the sidebar's column left it 25 of the column's 27 beside the intruder, and the
  -- inner split refuses to give up more: the width settles there, known unreachable.
  local moved = fake.pane(tab, { cols = 1 })
  table.insert(tab.pane_list, 2, moved)
  tab:set_split(25)
  moved.left, moved.width = 26, 1
  content.left = 28
  gui.perform_action = function(_, action, pane)
    win.actions[#win.actions + 1] = { action = action, pane = pane }
  end
  assert(geometry.correct(gui), "asked for the column")
  eq(sb.cols, 25, "and refused by the inner split")
  eq(geometry.correct(gui), false, "left there")
  gui.perform_action = nil
  -- The rescue moves the intruder under the content, and the whole column falls to the sidebar:
  -- the same panes in the same tab, one of them somewhere else.
  table.remove(tab.pane_list, 2)
  tab.pane_list[#tab.pane_list + 1] = moved
  tab:set_split(27)
  moved.left, moved.width, moved.top = 28, content.cols, 13
  assert(geometry.correct(gui), "27 is the column the move handed over, not a drag: corrected")
  eq(sb.cols, 28)
  eq(geometry.desired(wid), 28, "nothing was adopted")
end)

test("an adjust the host declines is asked again only after a pause, then lands when the host is back", function()
  local win, gui, _, sb = settled_tab()
  local real = getmetatable(gui).perform_action
  local declined = 0
  gui.perform_action = function(_, action, pane)
    -- a WezTerm overlay over the tab: the assignment is recorded and ignored
    declined = declined + 1
    win.actions[#win.actions + 1] = { action = action, pane = pane }
  end
  win:resize(10)
  assert(geometry.correct(gui), "asked once")
  eq(sb.cols, 33, "and ignored")
  eq(geometry.correct(gui), false, "not asked again at once")
  later(500, function()
    eq(geometry.correct(gui), false, "nor half a second later")
  end)
  eq(declined, 1)
  later(2500, function()
    assert(geometry.correct(gui), "asked again once the overlay has had time to close")
  end)
  eq(declined, 2)
  gui.perform_action = real
  later(5000, function()
    assert(geometry.correct(gui), "and lands when the host takes it")
  end)
  eq(sb.cols, 28)
  gui.perform_action = nil
end)

test("geometry.sync corrects whichever tab is active on the very next poll", function()
  local win, gui = window(2)
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  local first, second = win.tab_list[1], win.tab_list[2]
  win.active_tab_ref = first
  eq(geometry.sync(gui), false, "nothing to correct")
  win:resize(20)
  assert(geometry.sync(gui), "the active tab is corrected on the next poll, no rate gate")
  eq(sidebar.find(first).cols, 28)
  eq(sidebar.find(second).cols, 38, "the background tab waits for its activation")
  win.active_tab_ref = second
  assert(geometry.sync(gui), "and is corrected as it comes to the front")
  eq(sidebar.find(second).cols, 28)
end)

test("correction is skipped while a tab drag is in flight", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  win:resize(20)
  state.session.drag[gui:window_id()] = { tab_id = win.tab_list[1].id }
  eq(geometry.correct(gui), false)
  eq(sidebar.find(win.tab_list[1]).cols, 38)
  state.session.drag[gui:window_id()] = nil
  assert(geometry.correct(gui))
  eq(sidebar.find(win.tab_list[1]).cols, 28)
end)

test("the rail's narrow width is never adopted as the user's desired width", function()
  local win, gui, _, sb = settled_tab()
  local wid = gui:window_id()
  state.set_collapsed(wid, true)
  assert(geometry.correct(gui))
  eq(sb.cols, config.get().rail_width, "collapsed to the rail")
  eq(geometry.correct(gui), false)
  state.set_collapsed(wid, false)
  assert(geometry.correct(gui), "expanding corrects back")
  eq(sb.cols, 28)
  eq(geometry.desired(wid), 28, "the rail width was not adopted as a divider drag")
  eq(#win.actions > 0, true)
end)

test("typed Rust rail reserve effects are stored and applied only by geometry", function()
  config.setup {
    collapsed = "rail",
    rail_width = 5,
    rail_titlebar = "widen",
    backend = BACKEND,
  }
  local win, gui = H.ready_window(2)
  local first = sidebar.find(win.tab_list[1])
  local second = sidebar.find(win.tab_list[2])
  local wid = gui:window_id()
  state.set_collapsed(wid, true)
  local input = require "vtabs.input"
  input.handle(gui, second, "vtabs", '{"t":"intent","a":"set_rail_reserve","cols":12}')
  eq(geometry.rail_reserve(wid), nil, "a delayed background effect is ignored")
  input.handle(gui, first, "vtabs", '{"t":"intent","a":"set_rail_reserve","cols":9}')
  eq(geometry.rail_reserve(wid), 9, "Rust's computed reserve is retained")
  eq(geometry.desired(wid), 9, "widen uses the returned reserve")
  win.active_tab_ref = win.tab_list[2]
  input.handle(gui, first, "vtabs", '{"t":"intent","a":"set_rail_reserve","cols":0}')
  eq(geometry.rail_reserve(wid), 9, "the former active pane cannot clear the new tab's reserve")
  input.handle(gui, second, "vtabs", '{"t":"do","a":"set_rail_reserve","args":{"cols":7}}')
  eq(geometry.rail_reserve(wid), 7, "the legacy envelope reaches the same guarded effect")
  eq(geometry.desired(wid), 7)
  config.setup { backend = BACKEND }
end)

test("the split tree, not the pane's own size report, is the width corrected from", function()
  local win, gui, _, sb = settled_tab()
  win:resize(4)
  eq(sb.cols, 30)
  -- A mux mirror that has not caught up: the pane still reports 28 while the tree says 30.
  sb.get_dimensions = function()
    return { cols = 28, viewport_rows = 24, pixel_width = 280, pixel_height = 480, dpi = 96 }
  end
  assert(geometry.correct(gui))
  eq(last_action(win).arg[1], "Left")
  eq(last_action(win).arg[2], 2, "two columns, from the tree's 30, not none from the report's 28")
  eq(sb.cols, 28)
  sb.get_dimensions = nil
end)

test("the backend's resize report corrects from the tree and publishes nothing, with no width of its own", function()
  local win, gui, _, sb = settled_tab()
  local input = require "vtabs.input"
  local store = require "vtabs.store"
  local protocol = require "vtabs.gen.protocol"
  store.proto[sb:pane_id()] = protocol.VERSION
  win:resize(6)
  eq(sb.cols, 31)
  local sent = #sb.sent
  input.handle(gui, sb, "vtabs", '{"t":"resize","cols":31,"rows":24,"n":2}')
  eq(sb.cols, 28, "the report triggered a correction")
  eq(last_action(win).arg[2], 3)
  eq(#sb.sent, sent, "and no publish: the pane repainted itself, and a report per frame is not a generation per frame")
end)

---A tab whose panes live on a mux domain, the backend's server-side adjust queued the way a mux
---server answers: acknowledged at once, applied later.
local function remote_tab()
  local win, gui, tab, sb = settled_tab()
  local content = sidebar.content_pane(tab)
  sb.domain, content.domain = "e2emux", "e2emux"
  fake.remote_lag = true
  local function apply()
    return fake.apply_remote(sb)
  end
  return win, gui, tab, sb, apply
end

test("on a mux domain the sidebar's own size report releases the adjust in flight at once", function()
  local win, gui, _, sb, apply = remote_tab()
  local wid = gui:window_id()
  win:resize(10)
  assert(geometry.correct(gui), "sent")
  eq(geometry.correct(gui), false, "and nothing more while it is in flight")
  assert(apply(), "the mux applied it")
  eq(sb.cols, 28)
  geometry.landed(wid, sb:pane_id(), 28)
  eq(geometry.correct(gui), false, "in order")
  eq(geometry.inspect(wid).pending, nil, "released by the report; no stability window to wait out")
  win:resize(4)
  assert(geometry.correct(gui), "so the next frame's correction goes out at once")
  fake.remote_lag = false
end)

test("a mux mirror rebuilt to a stale width around an adjust in flight is neither chased nor adopted", function()
  local win, gui, tab, sb, apply = remote_tab()
  local wid = gui:window_id()
  local clock = H.clock()
  win:resize(2)
  eq(sb.cols, 29)
  assert(geometry.correct(gui), "the frame is corrected")
  -- the server applies it, but the mirror is rebuilt from a pane list answered just before that
  apply()
  eq(sb.cols, 28)
  local sent = #sb.sent
  clock.advance(50)
  tab:set_split(29)
  eq(geometry.correct(gui), false, "an echo of the old width is not chased")
  eq(#sb.sent, sent, "no second adjust on top of the first")
  -- the report arrives while the mirror still shows the echo
  geometry.landed(wid, sb:pane_id(), 28)
  clock.advance(50)
  eq(geometry.correct(gui), false, "still not adopted: the report and the mirror disagree")
  eq(geometry.desired(wid), 28)
  -- the mirror catches up with the server
  tab:set_split(28)
  clock.advance(50)
  eq(geometry.correct(gui), false)
  eq(geometry.inspect(wid).pending, nil, "cleared once report and mirror agree")
  eq(geometry.desired(wid), 28, "the width is still cfg.width")
  fake.remote_lag = false
  clock.restore()
end)

test("an unreported mux adjust stops blocking after its wait, and the width is asked for again", function()
  local win, gui, _, sb, _ = remote_tab()
  local clock = H.clock()
  win:resize(10)
  assert(geometry.correct(gui))
  local sent = #sb.sent
  clock.advance(300)
  eq(geometry.correct(gui), false, "the mux may still be applying it")
  eq(#sb.sent, sent)
  clock.advance(400)
  assert(geometry.correct(gui), "a server that lost it is not waited on forever")
  eq(adjusts_sent(sb, sent), 1, "asked once more")
  eq(sb.cols, 33, "the fake never applied either")
  fake.remote_lag = false
  clock.restore()
end)

test("a server that refuses the adjust releases the wait at once", function()
  local win, gui, _, sb, _ = remote_tab()
  local wid = gui:window_id()
  local input = require "vtabs.input"
  local store = require "vtabs.store"
  store.proto[sb:pane_id()] = require("vtabs.gen.protocol").VERSION
  win:resize(10)
  assert(geometry.correct(gui))
  local sent = #sb.sent
  eq(geometry.correct(gui), false, "in flight")
  input.handle(gui, sb, "vtabs", '{"t":"cli","op":"adjust","ok":false,"detail":"no cli here"}')
  eq(geometry.inspect(wid).pending, nil, "nothing is coming: the record is dropped")
  assert(geometry.correct(gui), "and the width is asked for again without waiting out the round trip")
  eq(adjusts_sent(sb, sent), 1)
  fake.remote_lag = false
end)

test("on a mux domain the frames of a resize are left to the server, and one adjust follows the settle", function()
  local win, gui, tab, sb = settled_tab()
  local content = sidebar.content_pane(tab)
  sb.domain, content.domain = "e2emux", "e2emux"
  local wid = gui:window_id()
  local clock = H.clock()
  local before_actions, sent = #win.actions, #sb.sent
  for _, d in ipairs { 5, 3, 2 } do
    geometry.on_resize(wid)
    win:resize(d)
    eq(
      geometry.correct(gui),
      false,
      "a frame on a mux domain is not corrected: each adjust costs a mirror rebuild per pane"
    )
    clock.advance(30)
  end
  assert(sb.cols > 28, "the frames dealt the sidebar columns")
  eq(adjusts_sent(sb, sent), 0)
  clock.advance(geometry.RESIZE_QUIET_MS)
  assert(geometry.correct(gui), "the settle corrects once")
  eq(adjusts_sent(sb, sent), 1, "server-side, through the sidebar's backend")
  eq(#win.actions, before_actions, "never through the GUI's mirror")
  eq(sb.cols, 28, "the backend resized the split")
  eq(tab.active, content, "and handed focus back")
  clock.restore()
end)

test("on a mux domain a moved divider is the user's only once it has sat still across a round trip", function()
  local win, gui, tab, sb = settled_tab()
  local content = sidebar.content_pane(tab)
  sb.domain, content.domain = "e2emux", "e2emux"
  local wid = gui:window_id()
  local before = #win.actions
  local clock = H.clock()
  wezterm.timers = {}
  tab:set_split(34)
  eq(geometry.correct(gui), false, "seen moving: neither fought nor adopted yet")
  eq(geometry.desired(wid), 28, "a mirror rebuilt from a stale pane list moves it the same way")
  eq(#wezterm.timers, 1, "and a follow-up is armed: the wait must not depend on a poll arriving")
  clock.advance(100)
  eq(geometry.correct(gui), false)
  eq(#wezterm.timers, 1, "one follow-up at a time")
  eq(geometry.desired(wid), 28)
  clock.advance(200)
  wezterm.fire_timers()
  eq(geometry.desired(wid), 34, "still there a round trip later: the hand's, by the follow-up alone")
  eq(#win.actions, before, "and never fought")
  eq(sb.cols, 34)
  clock.restore()
end)

test("a held tab-switch key is waited out: nothing is adjusted into a tab the key is passing through", function()
  local win, gui, _, sb = settled_tab()
  local wid = gui:window_id()
  local clock = H.clock()
  geometry.on_switch(wid)
  eq(geometry.switching(wid), false, "one switch is a switch")
  clock.advance(50)
  geometry.on_switch(wid)
  win:resize(10)
  clock.advance(10)
  eq(geometry.switching(wid), true, "two inside the dwell are a held key")
  eq(geometry.correct(gui), false, "the tab is left alone")
  eq(sb.cols, 33)
  clock.advance(240)
  eq(geometry.switching(wid), false)
  assert(geometry.correct(gui), "the tab the hand stopped on is served")
  eq(sb.cols, 28)
  clock.restore()
end)

test("a new sidebar is split at the width the window wants, so it never jumps a poll later", function()
  local win, gui = window(3)
  local wid = gui:window_id()
  win.active_tab_ref = win.tab_list[1]
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  eq(geometry.correct(gui), false)
  win.tab_list[1]:set_split(34)
  eq(geometry.correct(gui), false, "adopted")
  eq(geometry.desired(wid), 34)
  win.active_tab_ref = win.tab_list[2]
  sidebar.ensure(gui)
  local second = sidebar.find(win.tab_list[2])
  eq(second.split_args.size, 34, "split at the adopted width, not cfg.width")
  eq(second.cols, 34)
  state.set_collapsed(wid, true)
  win.active_tab_ref = win.tab_list[3]
  sidebar.ensure(gui)
  local third = sidebar.find(win.tab_list[3])
  eq(third.split_args.size, config.get().rail_width, "a collapsed window splits a rail")
  eq(math.type(third.split_args.size), "integer")
  state.set_collapsed(wid, false)
end)

test("a mux mirror rebuilt mid-adjust to another tab width does not make a held divider start over", function()
  local win, gui, tab, sb = settled_tab()
  local content = sidebar.content_pane(tab)
  sb.domain, content.domain = "e2emux", "e2emux"
  local wid = gui:window_id()
  local before = #win.actions
  local clock = H.clock()
  tab:set_split(34)
  eq(geometry.correct(gui), false, "seen holding 34")
  -- the server has resized the sidebar but not yet the content: the mirror reads 106 columns wide
  clock.advance(100)
  content.width = content.cols + 6
  eq(geometry.correct(gui), false, "a one-poll glitch: neither fought nor started over")
  content.width = nil
  clock.advance(200)
  eq(geometry.correct(gui), false)
  eq(geometry.desired(wid), 34, "adopted on the round trip the hand had already waited")
  eq(#win.actions, before, "and never fought")
  clock.restore()
end)

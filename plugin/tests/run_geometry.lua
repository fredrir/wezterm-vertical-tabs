local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local theme = require "vtabs.theme"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local geometry = require "vtabs.geometry"
local fake = require "fake_mux"

local test, eq, rgb, later, attach_all = H.test, H.eq, H.rgb, H.later, H.attach_all
local mark_ready, window = H.mark_ready, H.window

test("the page is tinted by default and elevation = 0 makes it seamless", function()
  local t = theme.resolve({}, fake.palette)
  local seamless = theme.resolve({ elevation = 0 }, fake.palette)
  eq(rgb(seamless.bg), "30,30,46", "0 is exactly the terminal background")
  assert(t.bg[1] > seamless.bg[1] and t.bg[3] > seamless.bg[3], "the default tint lifts it toward fg")
  eq(rgb(t.bg), rgb(theme.resolve({ elevation = 0.06 }, fake.palette).bg), "and it is 0.06")
end)

test("the accent chain is cursor_bg, then tab_bar active, then ansi[5], each behind both gates", function()
  local base = fake.palette
  -- Mocha's rosewater cursor clears 3.0 against the page but is 1.06 from the foreground.
  assert(theme.contrast({ 245, 224, 220 }, { 205, 214, 244 }) < 1.2, "fixture cursor is fg-coloured")
  eq(rgb(theme.resolve({}, base).accent), "137,180,250", "falls through to ansi[5]")
  local usable = util.merge(base, { cursor_bg = "#f38ba8" })
  eq(rgb(theme.resolve({}, usable).accent), "243,139,168", "a cursor that clears both gates wins")
  local no_cursor = util.merge(base, { cursor_bg = "#242438" })
  no_cursor.tab_bar = { active_tab = { bg_color = "#74c7ec" } }
  eq(rgb(theme.resolve({}, no_cursor).accent), "116,199,236", "then the scheme's active tab colour")
  local flat = util.merge(base, { cursor_bg = "#242438" })
  flat.tab_bar = { active_tab = { bg_color = "#94e2d5" } }
  eq(rgb(theme.resolve({}, flat).accent), "137,180,250", "a tab colour too close to fg is skipped too")
  eq(rgb(theme.resolve({ accent = "#ff0000" }, base).accent), "255,0,0", "a user accent still wins")
end)

local function last_action(win)
  return win.actions[#win.actions].action
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

test("window growth drifts the sidebar 50/50; correct claws it back in one AdjustPaneSize", function()
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
  eq(sb.cols, 28)
  eq(geometry.correct(gui), false, "second pass is a no-op")
end)

test("split Left puts the sidebar in first, split Right in second, so a right sidebar grows with Left", function()
  config.setup { position = "right", backend = { path = "/bin/wez-vtabs" } }
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
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a divider drag survives a config reload, unless the reload changed width itself", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  tab:set_split(34)
  eq(geometry.correct(gui), false, "a divider still moving is neither adopted nor fought")
  eq(geometry.desired(gui:window_id()), 28, "nothing is taken from a width that is still changing")
  later(400, function()
    eq(geometry.correct(gui), false, "and once the hand comes off, it is adopted")
  end)
  eq(geometry.desired(gui:window_id()), 34)
  eq(geometry.correct(gui), false)
  eq(sb.cols, 34)
  -- Every edit to wezterm.lua reloads, and the plugin watches its own files too, so a reload that
  -- says nothing about the width must not throw the drag away.
  geometry.reset(gui:window_id())
  eq(geometry.desired(gui:window_id()), 34, "an unrelated reload keeps it")
  eq(geometry.correct(gui), false, "and nothing is re-asserted")
  config.setup { width = 30, backend = { path = "/bin/wez-vtabs" } }
  geometry.reset(gui:window_id())
  eq(geometry.desired(gui:window_id()), 30, "changing width itself drops it")
  assert(geometry.correct(gui))
  eq(sb.cols, 30)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("an adjust the mux has not applied yet is issued once, and its landing is never adopted", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local wid = gui:window_id()
  mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  local issued = 0
  -- A remote mux acknowledges the adjust and applies it a poll or more later.
  gui.perform_action = function(_, action)
    if action.action == "AdjustPaneSize" then
      issued = issued + 1
    end
  end
  win:resize(30)
  assert(geometry.correct(gui), "the window resize is corrected")
  for _ = 1, 4 do
    eq(geometry.correct(gui), false, "and not re-issued while the mux has not applied it")
  end
  eq(issued, 1, "one AdjustPaneSize, not one per poll; the duplicates all land and overshoot")

  -- The width it eventually lands on is ours, so it must never read as a divider drag.

  tab:set_split(24)
  eq(geometry.correct(gui), false)
  eq(geometry.desired(wid), 28, "the landing is not adopted")

  -- The sidebar reporting its own size is proof it landed, so the next target goes out at once.
  geometry.landed(wid)
  gui.perform_action = nil
  assert(geometry.correct(gui), "and the retry is not blocked once it has")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a window drag corrects on the first frame of the burst and leaves the rest to the poll", function()
  local win, gui = window(1)
  local wid = gui:window_id()
  geometry.forget_window(wid)
  assert(geometry.on_resize(wid), "the first frame is corrected")
  for _ = 1, 10 do
    eq(geometry.on_resize(wid), false, "every frame after it costs nothing")
  end
  eq(#win.actions, 0, "so a drag issues no adjust per frame at all")
end)

test("correction with several content panes activates the sidebar and restores focus", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  mark_ready(tab)
  local extra = fake.pane(tab, { cols = sidebar.content_pane(tab).cols })
  tab.pane_list[#tab.pane_list + 1] = extra
  extra:activate()
  win:resize(10)
  assert(geometry.correct(gui), "correction ran")
  eq(tab.active, extra, "focus restored")
  eq(sidebar.find(tab).cols, 28)
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
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
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

test("an adopted width is clamped to a plausible sidebar", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  tab:set_split(78)
  eq(geometry.correct(gui), false, "the divider is still moving")
  later(400, function()
    eq(geometry.correct(gui), false, "drag adopted once it stops")
  end)
  eq(geometry.desired(gui:window_id()), 60, "clamped to tab width minus the content margin")
  geometry.reset(gui:window_id())
  assert(geometry.correct(gui), "a reset re-asserts cfg.width")

  -- Past the floor that follows the reset's own adjust, where a width is still ours.
  later(400, function()
    eq(geometry.correct(gui), false, "baseline recorded")
  end)
  tab:set_split(1)
  later(800, function()
    eq(geometry.correct(gui), false, "the divider is still moving")
  end)
  later(1200, function()
    eq(geometry.correct(gui), false, "drag adopted once it stops")
  end)
  eq(geometry.desired(gui:window_id()), 8, "clamped to the minimum width")
end)

test("an unreachable width is attempted until it stops moving, then left alone", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  win:resize(-71)
  eq(tab:width(), 9, "a window too narrow to hold the sidebar")
  assert(geometry.correct(gui), "first attempt moves it as far as the split allows")
  eq(sb.cols, 7, "clamped to width - 2")
  local issued = 0
  for _ = 1, 10 do
    if geometry.correct(gui) then
      issued = issued + 1
    end
  end
  assert(issued <= 4, "the retry is bounded; a mux gets a few polls to catch up, got " .. issued)
  local settled = #win.actions
  eq(geometry.correct(gui), false)
  eq(geometry.correct(gui), false)
  eq(#win.actions, settled, "no AdjustPaneSize and no activate once it is known unreachable")
  win:resize(40)
  assert(geometry.correct(gui), "a window resize unblocks the retry")
end)

test("two content panes side by side each keep MIN_CONTENT, not 20 columns between them", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  config.setup { meta = "auto", width = 40, backend = { path = "/bin/wez-vtabs" } }
  win:resize(73 - win.cols)
  sb.left, sb.width = 0, sb.cols
  content.left, content.width = sb.cols + 1, content.cols
  geometry.reset(gui:window_id())
  geometry.correct(gui)
  local one_band = sb.cols
  assert(one_band > 20, "one shell can give the sidebar most of a 73-column tab, got " .. one_band)

  -- A second shell beside the first: the guard was satisfied by their combined width, so the
  -- sidebar kept its 40 and the two shells were dealt 20 and 11.
  local beside = fake.pane(tab, { cols = 12 })
  tab.pane_list[#tab.pane_list + 1] = beside
  beside.left, beside.width = content.left + 20, 12
  geometry.reset(gui:window_id())
  geometry.correct(gui)
  assert(sb.cols < one_band, "the second band costs the sidebar width, got " .. sb.cols)
  assert(sb.cols <= 73 - 40, "and leaves 20 columns for each of them, got " .. sb.cols)

  -- Stacked panes share one band, so they cost nothing extra.
  beside.left = content.left
  geometry.reset(gui:window_id())
  geometry.correct(gui)
  eq(sb.cols, one_band, "a pane stacked under another shares its column band")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("an adjust the mux applies in pieces is never adopted, however long the pieces take", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  eq(geometry.correct(gui), false, "baseline recorded")

  -- A mux that moves the divider part of the way and finishes later. Each piece looks exactly like
  -- a hand on the divider: same tab width, same pixels, same cells, sitting still between polls.
  gui.perform_action = function(_, action)
    if action.action == "AdjustPaneSize" then
      local half = math.max(1, math.floor(action.arg[2] / 2))
      local delta = action.arg[1] == "Right" and half or -half
      tab:set_split(sb.cols + delta)
    end
  end
  -- The reported bug in miniature: collapse to the rail, expand again, and the restore lands in
  -- pieces because the mux is slow.
  sidebar.set_collapsed(gui, true)
  later(400, function()
    assert(geometry.correct(gui), "collapsing asks for the rail")
  end)
  sidebar.set_collapsed(gui, false)
  later(800, function()
    assert(geometry.correct(gui), "expanding asks for the sidebar back")
  end)
  assert(sb.cols ~= 28, "and the mux applied only part of it, at " .. sb.cols)

  -- The backend reports its own new size, so the next adjust need not wait its turn. That must not
  -- make a width we are still driving towards look like the user's.
  geometry.landed(wid)
  for _, at in ipairs { 1200, 1600, 2000 } do
    later(at, function()
      geometry.correct(gui)
    end)
    eq(geometry.desired(wid), 28, "the width is still ours at " .. at .. " ms, never adopted")
  end
  gui.perform_action = nil
  later(2400, function()
    geometry.correct(gui)
  end)
  eq(sb.cols, 28, "and it restores once the mux keeps up")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the sidebar is activated before an adjust even in a tab with one content pane", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  eq(#sidebar.classify(tab), 1, "one content pane, which used to skip the activation")
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
  eq(seen, sb, "with the sidebar active, so AdjustPaneSize moves the right child")
  eq(tab.active, content, "and focus handed straight back")
  gui.perform_action = nil
end)

test("a column wezterm dealt during a window drag is not adopted once the metrics settle", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  later(400, function()
    eq(geometry.correct(gui), false, "baseline recorded")
  end)

  -- A window drag is one burst: only its leading edge corrects, and wezterm deals the sidebar half
  -- of every new column. The last deal lands after the pixels and the tab width have stopped.
  geometry.on_resize(wid)
  win:resize(30)
  later(800, function()
    geometry.correct(gui)
  end)
  -- Now nothing is moving but the deal itself, which every metric test reads as a divider drag.
  later(900, function()
    geometry.correct(gui)
  end)
  eq(geometry.desired(wid), 28, "the deal is wezterm's, not the user's")
  assert(sb.cols == 28, "and the width is put back, at " .. sb.cols)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a width the content bands clamp is still the width the user asked for, not a new one", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  local content = sidebar.content_pane(tab)
  sb.left, sb.width = 0, sb.cols
  content.left, content.width = sb.cols + 1, content.cols
  later(400, function()
    geometry.correct(gui)
  end)
  -- The user drags the divider out to 40 and it is adopted, with one content band.
  tab:set_split(40)
  later(800, function()
    eq(geometry.correct(gui), false, "still moving")
  end)
  later(1200, function()
    eq(geometry.correct(gui), false, "adopted once it stops")
  end)
  eq(geometry.desired(wid), 40, "the drag is theirs")

  -- More bands beside it: 40 no longer leaves each of them MIN_CONTENT, so the width is clamped.
  -- That is a clamp, not an adoption -- what they asked for is remembered, not rewritten.
  for i = 1, 2 do
    local beside = fake.pane(tab, { cols = 8 })
    tab.pane_list[#tab.pane_list + 1] = beside
    beside.left, beside.width = content.left + i * 9, 8
  end
  later(1600, function()
    geometry.correct(gui)
  end)
  later(2000, function()
    geometry.correct(gui)
  end)
  eq(geometry.desired(wid), 40, "the clamp does not overwrite the width they asked for")

  -- Close them and the width they asked for is what the plugin goes back to.
  table.remove(tab.pane_list, #tab.pane_list)
  table.remove(tab.pane_list, #tab.pane_list)
  later(2000, function()
    geometry.correct(gui)
  end)
  eq(geometry.desired(wid), 40, "still theirs, once the room comes back")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a width the bands clamped us to is never adopted, however long it sits there", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  local content = sidebar.content_pane(tab)
  sb.left, sb.width = 0, sb.cols
  content.left, content.width = sb.cols + 1, content.cols
  later(400, function()
    geometry.correct(gui)
  end)
  tab:set_split(40)
  later(800, function()
    geometry.correct(gui)
  end)
  later(1200, function()
    eq(geometry.correct(gui), false, "the drag is adopted once the hand comes off")
  end)
  eq(geometry.desired(wid), 40, "40 is theirs")

  -- Two more bands: 40 no longer leaves each of them MIN_CONTENT, so we drive it down to 20.
  for i = 1, 2 do
    local beside = fake.pane(tab, { cols = 8 })
    tab.pane_list[#tab.pane_list + 1] = beside
    beside.left, beside.width = content.left + i * 9, 8
  end
  -- A width that has sat still with nothing outstanding is re-adopted before the adjust is reached,
  -- so the new target is only pursued once something perturbs it -- here the resize that opened the
  -- band. That deferral is a separate wart; what this test pins is what happens to the clamp after.
  later(1600, function()
    geometry.on_resize(wid)
    assert(geometry.correct(gui), "the clamp is driven, not merely computed")
  end)
  eq(sb.cols, 20, "the sidebar is where the bands leave room for it")
  -- The width we clamped it to now sits still, with nothing outstanding: this is the state the
  -- adoption branch used to mistake for a hand on the divider.
  for _, at in ipairs { 2000, 3000, 8000 } do
    later(at, function()
      eq(geometry.correct(gui), false, "nothing to do at " .. at .. " ms")
    end)
    eq(geometry.desired(wid), 40, "still theirs after " .. at .. " ms at the clamp")
  end

  -- The band closes: the room comes back and so must the width, rather than 20 being kept.
  table.remove(tab.pane_list, #tab.pane_list)
  table.remove(tab.pane_list, #tab.pane_list)
  later(9000, function()
    assert(geometry.correct(gui), "the room came back, so the width is pursued")
  end)
  eq(sb.cols, 40, "driven back to what they dragged to")
  eq(geometry.desired(wid), 40, "20 was our clamp, never their preference")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a font or dpi change is corrected, not adopted as a divider drag", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  for _, p in ipairs(tab:panes()) do
    p.cell_width = 14
  end
  tab:set_split(20)
  assert(geometry.correct(gui), "a wider cell is not the user dragging the divider")
  eq(geometry.desired(gui:window_id()), 28, "cfg.width still wins")
  eq(sb.cols, 28)
end)

test("geometry.sync corrects on a tab change and rate-gates otherwise", function()
  local win, gui = window(2)
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  local first, second = win.tab_list[1], win.tab_list[2]
  win.active_tab_ref = first
  assert(geometry.sync(gui, first.id) == false, "nothing to correct")
  win:resize(20)
  eq(geometry.sync(gui, first.id), false, "same tab inside the observe window is skipped")
  eq(sidebar.find(first).cols, 38)
  win.active_tab_ref = second
  assert(geometry.sync(gui, second.id), "a tab change corrects at once")
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
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  eq(geometry.correct(gui), false, "baseline recorded")
  state.set_collapsed(wid, true)
  geometry.correct(gui)
  eq(sb.cols, config.get().rail_width, "collapsed to the rail")
  geometry.correct(gui)
  state.set_collapsed(wid, false)
  assert(geometry.correct(gui), "expanding corrects back")
  eq(sb.cols, 28)
  eq(geometry.desired(wid), 28, "the rail width was not adopted as a divider drag")
end)

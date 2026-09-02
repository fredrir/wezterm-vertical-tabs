---The pane lifecycle under interleaving: handlers as tasks, a mux that answers late, two windows.
local H = require "support.helpers"
local async = require "support.async"
local fake = require "fake_mux"
local wezterm = require "wezterm"
local actions = require "vtabs.actions"
local gate = require "vtabs.gate"
local rescue = require "vtabs.sidebar_rescue"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local store = require "vtabs.store"
local util = require "vtabs.util"
local test, eq = H.test, H.eq

---Runs `fn` with a mux that answers late, and never leaves the fake that way for the next test.
local function deferred(fn)
  fake.deferred = true
  local ok, err = pcall(fn)
  fake.deferred = false
  if not ok then
    error(err, 0)
  end
end

local function warnings(from, needle)
  local n = 0
  for i = from + 1, #wezterm.log do
    if wezterm.log[i]:find(needle, 1, true) then
      n = n + 1
    end
  end
  return n
end

test("a deferred split leaves the tree alone until the task resumes", function()
  local win = H.window(1)
  local tab = win.tab_list[1]
  fake.deferred = true
  local task = async.spawn(function()
    return tab.pane_list[1]:split { direction = "Left", size = 28, args = { "wez-vtabs" } }
  end)
  eq(#tab:panes(), 1)
  eq(task.tag, "split")
  async.run()
  fake.deferred = false
  eq(#tab:panes(), 2)
  eq(task.results[1], tab.pane_list[1])
end)

test("the fake cli answers only on the GUI's own socket", function()
  local win = H.window(1, { attach = true, ready = true })
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  eq(rescue.cli_kill(sb:pane_id()), false)
  eq(#tab:panes(), 2)
  H.with_cli(function()
    eq(rescue.cli_kill(sb:pane_id()), true)
  end)
  eq(#tab:panes(), 1)
  local last = wezterm.spawned[#wezterm.spawned]
  eq(last[4], "kill-pane")
  eq(last[6], tostring(sb:pane_id()))
end)

test("the clock only moves by hand", function()
  local clock = H.clock()
  local t0 = util.now_ms()
  eq(clock.advance(1500), t0 + 1500)
  eq(util.now_ms(), t0 + 1500)
  clock.restore()
end)

test("a moved tab lives in the other window and both windows are listed", function()
  local win = H.window(2)
  local dest = fake.window()
  local tab = win.tab_list[2]
  fake.move_tab(win, tab, dest)
  eq(#win.tab_list, 1)
  eq(#dest.tab_list, 1)
  eq(tab:window(), dest)
  local listed = 0
  for _, w in ipairs(wezterm.mux.all_windows()) do
    if w == dest or w == win then
      listed = listed + 1
    end
  end
  eq(listed, 2)
  fake.close_window(dest)
end)

test("the gate: a verb waits for a split another handler is inside", function()
  local win, gui = H.window(1)
  local wid = gui:window_id()
  local tab = win.tab_list[1]
  deferred(function()
    local first = async.spawn(function()
      sidebar.ensure(gui)
    end)
    eq(first.tag, "split")
    eq(gate.held(wid), true)
    local seen
    local second = async.spawn(function()
      gate.run(wid, "probe", function()
        seen = #tab:panes()
      end)
    end)
    eq(second.tag, "sleep")
    eq(seen, nil)
    async.run()
    eq(seen, 2)
  end)
  eq(gate.held(wid), false)
end)

test("the gate: ensure and activate_tab racing across a deferred split make one sidebar", function()
  local win, gui = H.window(1)
  local tab = win.tab_list[1]
  local activate
  deferred(function()
    async.spawn(function()
      sidebar.ensure(gui)
    end)
    activate = async.spawn(function()
      return actions.activate_tab(gui, tab:tab_id())
    end)
    async.run()
  end)
  eq(#tab:panes(), 2)
  eq(H.sidebars_in(tab), 1)
  eq(state.sidebar_pane_id(tab:tab_id()), sidebar.find(tab):pane_id())
  eq(activate.results[1], sidebar.content_pane(tab))

  -- the other order: activate_tab is inside the split, and ensure answers busy at once
  local win2, gui2 = H.window(1)
  local tab2 = win2.tab_list[1]
  deferred(function()
    local first = async.spawn(function()
      return actions.activate_tab(gui2, tab2:tab_id())
    end)
    eq(first.tag, "split")
    local _, why = sidebar.ensure(gui2)
    eq(why, "busy")
    async.run()
  end)
  eq(#tab2:panes(), 2)
  eq(H.sidebars_in(tab2), 1)
end)

test("the gate: a stale holder is evicted once, with one warning", function()
  local _, gui = H.window(1)
  local wid = gui:window_id()
  local clock = H.clock()
  local stuck = async.spawn(function()
    gate.run(wid, "stuck", function()
      async.yield "stuck"
    end)
  end)
  stuck.parked = true
  eq(gate.held(wid), true)
  clock.advance(gate.STALE_MS + 1000)
  local ran = false
  local before = #wezterm.log
  gate.run(wid, "late", function()
    ran = true
  end)
  eq(ran, true)
  eq(gate.held(wid), false)
  eq(warnings(before, "gate: stuck held window"), 1)

  -- a new holder, then the evicted one finishing: it must not clear its successor
  local successor = async.spawn(function()
    gate.run(wid, "next", function()
      async.yield "hold"
    end)
  end)
  successor.parked = true
  eq(gate.held(wid), true)
  stuck.parked = false
  async.run()
  eq(gate.held(wid), true)
  successor.parked = false
  async.run()
  eq(gate.held(wid), false)
  clock.restore()
end)

test("ensure while another coroutine holds the gate returns without running", function()
  local win, gui = H.window(1)
  local wid = gui:window_id()
  local tab = win.tab_list[1]
  local holder = async.spawn(function()
    gate.run(wid, "hold", function()
      async.yield "hold"
    end)
  end)
  holder.parked = true
  local _, why = sidebar.ensure(gui)
  eq(why, "busy")
  eq(#tab:panes(), 1)
  holder.parked = false
  async.run()
  sidebar.ensure(gui)
  eq(#tab:panes(), 2)
end)

test("a tab moved to another window keeps its sidebar and is not recorded as closed", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  local tab = win.tab_list[2]
  local sb = sidebar.find(tab)
  local token = state.token_for(sb:pane_id())
  local dest = fake.window()
  fake.move_tab(win, tab, dest)
  while state.pop_closed() do
  end
  sidebar.ensure(gui)
  eq(state.sidebar_pane_id(tab:tab_id()), sb:pane_id(), "mapping kept")
  eq(state.token_for(sb:pane_id()), token, "token kept")
  eq(state.pop_closed(), nil, "nothing recorded as closed")
  dest.active_tab_ref = tab
  sidebar.ensure(dest.gui)
  eq(#tab:panes(), 2, "no second split")
  eq(sidebar.find(tab), sb)
  eq(sidebar.is_ready(sb), true)
  fake.close_window(dest)
end)

test("tear_off gives the new window its sidebar at once and closes the orphan behind it", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local tab = win.tab_list[1]
  local windows = #wezterm.windows
  eq(actions.tear_off(gui, tab:tab_id()), true)
  eq(#wezterm.windows, windows + 1)
  local new_win = wezterm.windows[#wezterm.windows]
  eq(#new_win.tab_list, 1)
  eq(#new_win.tab_list[1]:panes(), 2)
  eq(H.sidebars_in(new_win.tab_list[1]), 1)
  eq(#win.tab_list, 0, "the source tab held only a sidebar and is gone")
  fake.close_window(new_win)
end)

test("a duplicate sidebar this process split is closed on the next pass; a stranger's marker is not", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  local dup = fake.pane(tab, { title = "wez-vtabs:beef" })
  tab.pane_list[#tab.pane_list + 1] = dup
  store.spawned[dup:pane_id()] = true
  local before, actions_before = #wezterm.log, #win.actions
  H.with_cli(function()
    sidebar.ensure(gui)
  end)
  eq(#tab:panes(), 2, "the duplicate is gone")
  eq(sidebar.find(tab), sb)
  eq(warnings(before, "duplicate sidebar"), 1)
  eq(#win.actions, actions_before, "closed through the cli, not by activation")
  H.with_cli(function()
    sidebar.ensure(gui)
  end)
  eq(warnings(before, "duplicate sidebar"), 1, "said once")

  local stranger = fake.pane(tab, { title = "wez-vtabs:cafe" })
  tab.pane_list[#tab.pane_list + 1] = stranger
  local kills = 0
  H.with_cli(function()
    sidebar.ensure(gui)
  end)
  for _, argv in ipairs(wezterm.spawned) do
    if argv[4] == "kill-pane" and argv[6] == tostring(stranger:pane_id()) then
      kills = kills + 1
    end
  end
  eq(kills, 0)
  eq(#tab:panes(), 3, "a marker nobody here spawned stays")
end)

test("a resize burst leaves one sidebar per tab at the desired width", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  local wid = gui:window_id()
  local clock = H.clock()
  local geometry = require "vtabs.geometry"
  local function adjusts()
    local n = 0
    for _, argv in ipairs(wezterm.spawned) do
      if argv[4] == "adjust-pane-size" then
        n = n + 1
      end
    end
    return n
  end
  local before = adjusts()
  H.with_cli(function()
    for _, d in ipairs { 5, -6, 7, -5, 6, -7, 5, 6 } do
      win:resize_mux(d)
      async.spawn(function()
        if geometry.on_resize(wid) then
          geometry.correct(gui)
        end
      end)
    end
    win:settle_mux()
    async.run()
    for _ = 1, 3 do
      clock.advance(500)
      sidebar.ensure(gui)
      geometry.landed(wid)
      geometry.correct(gui)
    end
  end)
  for _, tab in ipairs(win.tab_list) do
    eq(#tab:panes(), 2)
    eq(H.sidebars_in(tab), 1)
  end
  eq(sidebar.find(win.tab_list[1]).cols, 28)
  assert(adjusts() - before <= 2, "at most two adjusts for one burst, got " .. (adjusts() - before))
  H.with_cli(function()
    actions.activate_tab(gui, win.tab_list[2]:tab_id())
  end)
  eq(sidebar.find(win.tab_list[2]).cols, 28, "the other tab is corrected as it activates")
  clock.restore()
end)

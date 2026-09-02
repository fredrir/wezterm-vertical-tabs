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

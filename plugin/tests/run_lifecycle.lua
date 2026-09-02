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
  eq(rescue.cli_kill(sb), false)
  eq(#tab:panes(), 2)
  H.with_cli(function()
    eq(rescue.cli_kill(sb), true)
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
  local view = require "vtabs.view"
  local function adjusts()
    local n = 0
    for _, entry in ipairs(win.actions) do
      if entry.action.action == "AdjustPaneSize" then
        n = n + 1
      end
    end
    return n
  end
  local before = adjusts()
  wezterm.timers = {}
  for _, d in ipairs { 5, -6, 7, -5, 6, -7, 5, 6 } do
    win:resize_mux(d)
    async.spawn(function()
      view.on_resize(gui)
    end)
  end
  eq(adjusts(), before, "nothing adjusted while frames arrived")
  win:settle_mux()
  async.run()
  clock.advance(geometry.SETTLE_MS)
  wezterm.fire_timers()
  for _ = 1, 3 do
    clock.advance(500)
    sidebar.ensure(gui)
    geometry.landed(wid)
    geometry.correct(gui)
  end
  for _, tab in ipairs(win.tab_list) do
    eq(#tab:panes(), 2)
    eq(H.sidebars_in(tab), 1)
  end
  eq(sidebar.find(win.tab_list[1]).cols, 28)
  assert(adjusts() - before <= 2, "at most two adjusts for one burst, got " .. (adjusts() - before))
  actions.activate_tab(gui, win.tab_list[2]:tab_id())
  eq(sidebar.find(win.tab_list[2]).cols, 28, "the other tab is corrected as it activates")
  clock.restore()
end)

test("a tab switch still waiting is superseded by the next one", function()
  local win, gui = H.window(3, { attach = true, ready = true })
  local wid = gui:window_id()
  local holder = async.spawn(function()
    gate.run(wid, "hold", function()
      async.yield "hold"
    end)
  end)
  holder.parked = true
  local first = async.spawn(function()
    return actions.activate_tab(gui, win.tab_list[2]:tab_id())
  end)
  local second = async.spawn(function()
    return actions.activate_tab(gui, win.tab_list[3]:tab_id())
  end)
  eq(first.tag, "sleep")
  eq(second.tag, "sleep")
  holder.parked = false
  async.run()
  eq(first.results[2], "superseded")
  eq(second.results[1], sidebar.content_pane(win.tab_list[3]))
  eq(win.active_tab_ref, win.tab_list[3])
end)

test("a pane on a mux domain is never driven through the cli", function()
  local win = H.window(1, { attach = true, ready = true })
  local sb = sidebar.find(win.tab_list[1])
  sb.domain = "localmux"
  local spawned = #wezterm.spawned
  H.with_cli(function()
    eq(rescue.cli_kill(sb), false)
  end)
  eq(#wezterm.spawned, spawned, "no cli process for a mux-domain pane")
  eq(#win.tab_list[1]:panes(), 2)
end)

test("a backend is restarted after three unanswered pings, never for idle time alone", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  local pid = sb:pane_id()
  local clock = H.clock()
  local function pings()
    local n = 0
    for _, line in ipairs(sb.sent) do
      n = n + (line:find('"ping"', 1, true) and 1 or 0)
    end
    return n
  end
  local logged = #wezterm.log
  store.seen[pid] = clock.advance(0)
  -- the GUI polled nothing for a minute: the first poll back only asks
  eq(rescue.check_liveness(gui, tab, sb, clock.advance(60000)), true)
  eq(pings(), 1, "one ping, no restart")
  eq(#tab:panes(), 2)
  -- the pong lands: a fresh cycle, quiet until the idle threshold again
  store.seen[pid] = clock.advance(10)
  eq(rescue.check_liveness(gui, tab, sb, clock.advance(5000)), true)
  eq(pings(), 1)
  rescue.check_liveness(gui, tab, sb, clock.advance(4000))
  eq(pings(), 2, "idle again: pinged")
  -- silence: one more ping per poll two seconds apart; the gap after the third unanswered restarts
  rescue.check_liveness(gui, tab, sb, clock.advance(500))
  eq(pings(), 2, "never re-pinged inside the gap")
  eq(rescue.check_liveness(gui, tab, sb, clock.advance(2000)), true)
  eq(pings(), 3)
  eq(rescue.check_liveness(gui, tab, sb, clock.advance(2000)), true)
  eq(pings(), 4)
  eq(rescue.check_liveness(gui, tab, sb, clock.advance(2000)), false)
  eq(#tab:panes(), 1, "the sidebar is closed")
  eq(warnings(logged, "unresponsive"), 1)
  clock.restore()
end)

test("the model goes to the shown sidebar; a background one catches up when its tab comes forward", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  local view = require "vtabs.view"
  local shown, hidden = sidebar.find(win.tab_list[1]), sidebar.find(win.tab_list[2])
  store.proto[shown:pane_id()], store.proto[hidden:pane_id()] = 2, 2
  local function models(sb)
    local n = 0
    for _, line in ipairs(sb.sent) do
      n = n + (line:find('"t":"model"', 1, true) and 1 or 0)
    end
    return n
  end
  view.sync(gui)
  eq(models(shown), 1)
  eq(models(hidden), 1, "a sidebar never written to is dressed once")
  win.tab_list[2]:set_title "renamed"
  view.sync(gui)
  eq(models(shown), 2, "the shown sidebar gets the change")
  eq(models(hidden), 1, "the background one is left as it is")
  win.active_tab_ref = win.tab_list[2]
  view.sync(gui)
  eq(models(hidden), 2, "and catches up once shown")
  eq(models(shown), 2)
end)

test("a wheel tick the backend already applied waits for the poll instead of a sync of its own", function()
  local _, gui, _, sb = H.key_window(1)
  local view = require "vtabs.view"
  local input = require "vtabs.input"
  store.proto[sb:pane_id()] = 2
  view.sync(gui)
  local function models()
    local n = 0
    for _, line in ipairs(sb.sent) do
      n = n + (line:find('"t":"model"', 1, true) and 1 or 0)
    end
    return n
  end
  local before = models()
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"set_scroll","args":{"top":3,"user":true}}')
  eq(store.scroll[gui:window_id()], 3, "the scroll is recorded")
  eq(models(), before, "and nothing is sent until the poll")
  view.sync(gui)
  eq(models(), before + 1, "which carries it")
end)

test("a sidebar that ignores quit is killed by another backend on its server, by title", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  local tab, helper_tab = win.tab_list[1], win.tab_list[2]
  local sb, helper = sidebar.find(tab), sidebar.find(helper_tab)
  sb:send_text "\27]2;wez-vtabs:abcd\7"
  sb.hung = true
  local clock = H.clock()
  local acted = #win.actions
  sidebar.detach(gui, tab)
  eq(#tab:panes(), 2, "quit went unheard")
  clock.advance(2100)
  sidebar.ensure(gui)
  assert(helper.sent[#helper.sent]:find('"kill"', 1, true), "the helper was asked")
  assert(helper.sent[#helper.sent]:find("wez-vtabs:abcd", 1, true), "by title")
  eq(helper.killed, 1)
  eq(#tab:panes(), 1, "and the server killed it")
  eq(#win.actions, acted, "nothing closed by activation")
  eq(H.sidebars_in(helper_tab), 1, "the helper is untouched")
  clock.restore()
end)

test("a split that landed in the sidebar's column on a mux domain is moved by the tab's own backend", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local tab = win.tab_list[1]
  local sb, shell = sidebar.find(tab), tab.pane_list[2]
  for _, p in ipairs(tab.pane_list) do
    p.domain = "localmux"
  end
  store.proto[sb:pane_id()] = 2
  -- SplitHorizontal on the sidebar: a shell inside the 28 columns the sidebar is meant to have
  local stray = fake.pane(tab, { title = "zsh", cols = 13, domain = "localmux" })
  stray.left = 15
  table.insert(tab.pane_list, 2, stray)
  local spawned = #wezterm.spawned
  sidebar.ensure(gui)
  assert(sb.sent[#sb.sent]:find('"rescue"', 1, true), "the backend was asked: " .. sb.sent[#sb.sent])
  eq(sb.moved, 1, "and moved the stray under the shell")
  eq(tab.pane_list[2], shell)
  eq(tab.pane_list[3], stray)
  eq(#wezterm.spawned, spawned, "no cli from the GUI")
  local before = #sb.sent
  sidebar.ensure(gui)
  eq(#sb.sent, before, "not asked again while the move lands")
end)

test("split takes a spawn command for the new pane, or a function of the content pane", function()
  local _, gui, _, sb, content = H.key_window(1)
  local seen = nil
  local below = actions.split(gui, "Down", function(base)
    seen = base
    return { domain = "CurrentPaneDomain" }
  end)
  eq(seen, content, "the content pane, never the sidebar")
  eq(below.split_args.direction, "Bottom")
  eq(below.split_args.domain, "CurrentPaneDomain")
  assert(sb ~= below)
  local pane = actions.split(gui, "Right", { args = { "zsh", "-l" }, set_environment_variables = { A = "1" } })
  eq(pane.split_args.direction, "Right")
  eq(pane.split_args.args[2], "-l")
  eq(pane.split_args.set_environment_variables.A, "1")
end)

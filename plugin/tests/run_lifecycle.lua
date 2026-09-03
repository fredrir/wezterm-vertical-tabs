---The pane lifecycle under interleaving: handlers as tasks, a mux that answers late, two windows.
local H = require "support.helpers"
local async = require "support.async"
local fake = require "fake_mux"
local wezterm = require "wezterm"
local actions = require "vtabs.actions"
local config = require "vtabs.config"
local gate = require "vtabs.gate"
local rescue = require "vtabs.sidebar_rescue"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local store = require "vtabs.store"
local protocol = require "vtabs.gen.protocol"
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

local function batch_lines(text)
  local out = {}
  for line in tostring(text):gmatch "[^\n]+" do
    out[#out + 1] = line
  end
  return out
end

local function batch_generation(text)
  return tonumber(tostring(text):match '"t":"begin","generation":(%d+)')
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
  state.set_token(dup:pane_id(), "duplicate-test")
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

test("a resize burst is corrected frame by frame and leaves one sidebar per tab at the desired width", function()
  local win, gui = H.window(2, { attach = true, ready = true })
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
  local frames = { 5, -6, 7, -5, 6, -7, 5, 6 }
  for _, d in ipairs(frames) do
    win:resize(d)
    async.spawn(function()
      view.on_resize(gui)
    end)
    async.run()
    eq(sidebar.find(win.tab_list[1]).cols, 28, "the frame is corrected before the next one lands")
  end
  clock.advance(geometry.SETTLE_MS)
  wezterm.fire_timers()
  for _ = 1, 3 do
    clock.advance(500)
    sidebar.ensure(gui)
    geometry.correct(gui)
  end
  for _, tab in ipairs(win.tab_list) do
    eq(#tab:panes(), 2)
    eq(H.sidebars_in(tab), 1)
  end
  eq(sidebar.find(win.tab_list[1]).cols, 28)
  eq(adjusts() - before, #frames, "one adjust per frame, none from the settle timer or the polls")
  eq(sidebar.find(win.tab_list[2]).cols, 28 + 6, "the background tab keeps what the frames dealt it")
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
  store.proto[shown:pane_id()], store.proto[hidden:pane_id()] = protocol.VERSION, protocol.VERSION
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

test("one publish enumerates the window tabs once", function()
  local win, gui = H.window(3, { attach = true, ready = true })
  local view = require "vtabs.view"
  config.get().debug = true
  local logs_before = #wezterm.log
  for _, tab in ipairs(win.tab_list) do
    local sb = sidebar.find(tab)
    store.proto[sb:pane_id()] = protocol.VERSION
  end
  win.tab_enumerations = 0
  eq(view.sync(gui), true)
  eq(win.tab_enumerations, 1, "snapshot is the publish's only mux-tree observation")
  local sb = sidebar.find(win.active_tab_ref)
  assert(sb.sent[#sb.sent]:find('"t":"menu"', 1, true), "the coherent snapshot was published")
  local debug = table.concat(wezterm.log, "\n", logs_before + 1)
  assert(debug:find("mux_collections=4", 1, true), "debug counters expose one tree plus three pane collections")
  assert(debug:find("changed=", 1, true) and debug:find("bytes=", 1, true), "wire counters expose deltas and bytes")
end)

test("the strip wire never borrows cached or background metrics for the active pane", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  local view = require "vtabs.view"
  local first, second = sidebar.find(win.tab_list[1]), sidebar.find(win.tab_list[2])
  store.proto[first:pane_id()] = protocol.VERSION
  store.proto[second:pane_id()] = protocol.VERSION
  view.sync(gui)
  win.active_tab_ref = win.tab_list[2]
  second.get_dimensions = function()
    return nil
  end
  local before = #second.sent
  view.sync(gui)
  local model_line = nil
  for i = before + 1, #second.sent do
    model_line = second.sent[i]:find('"t":"model"', 1, true) and second.sent[i] or model_line
  end
  assert(model_line, "active pane receives its changed raw-facts model")
  assert(model_line:find('"chrome"', 1, true), "raw host chrome facts cross the model wire")
  assert(not model_line:find('"metrics"', 1, true), "no cached or background pane metrics are borrowed")
  assert(not model_line:find('"toggle_row"', 1, true), "Lua sends no derived toggle geometry")
  assert(not model_line:find('"cell_w"', 1, true), "Lua sends no derived cell width")

  gui.full_screen = true
  before = #second.sent
  view.sync(gui)
  model_line = nil
  for i = before + 1, #second.sent do
    model_line = second.sent[i]:find('"t":"model"', 1, true) and second.sent[i] or model_line
  end
  assert(model_line and model_line:find('"is_full_screen":true', 1, true), "fullscreen chrome refreshes")
  assert(not model_line:find('"metrics"', 1, true), "fullscreen needs no invented metrics")
end)

test("a geometry adjustment discards the snapshot it invalidated", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local view = require "vtabs.view"
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  store.proto[sb:pane_id()] = protocol.VERSION
  tab:set_split(38)
  local sent = #sb.sent
  eq(view.sync(gui), false, "the correcting pass is not publishable")
  eq(#sb.sent, sent, "no section from the stale geometry crossed the wire")
  assert(#win.actions > 0, "geometry issued its correction")
end)

test("ready atomic_sync capability selects one ordered publish batch", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local sb = sidebar.find(win.tab_list[1])
  require("vtabs.input").handle(
    gui,
    sb,
    "vtabs",
    '{"t":"ready","v":3,"cols":28,"rows":24,"paints":true,"caps":["atomic_sync",'
      .. '"typed_intents","theme_hooks","settings_document"],"n":1}'
  )
  assert(sidebar.supports(sb, "atomic_sync"), "ready capability was recorded")
  assert(sidebar.supports(sb, "typed_intents"), "typed intent capability was recorded")
  assert(sidebar.supports(sb, "theme_hooks"), "theme hook capability was recorded")
  assert(sidebar.supports(sb, "settings_document"), "settings ownership capability was recorded")
  local lines = batch_lines(sb.sent[#sb.sent])
  eq(#lines, 6, "begin, four sidebar sections, commit share one send_text")
  local frame_prefix = protocol.CONTROL_PREFIX .. state.token_for(sb:pane_id()) .. " "
  for _, line in ipairs(lines) do
    eq(line:sub(1, #frame_prefix), frame_prefix, "every record carries the pane session proof")
  end
  assert(lines[1]:find('"t":"begin"', 1, true), "begin first")
  assert(lines[2]:find('"t":"config"', 1, true), "config second")
  assert(lines[3]:find('"t":"theme"', 1, true), "theme third")
  assert(lines[4]:find('"t":"model"', 1, true), "model fourth")
  assert(lines[5]:find('"t":"menu"', 1, true), "menu fifth")
  assert(lines[6]:find('"t":"commit"', 1, true), "commit last")
  eq(batch_generation(lines[1]), tonumber(lines[6]:match '"generation":(%d+)'), "transaction generations match")
end)

test("ready requires the exact framed protocol version before auth", function()
  local _, gui, _, sb = H.key_window(1)
  store.proto[sb:pane_id()] = nil
  local before = #sb.sent
  require("vtabs.input").handle(
    gui,
    sb,
    "vtabs",
    '{"t":"ready","v":2,"cols":28,"rows":24,"paints":true,"caps":["atomic_sync"],"n":1}'
  )
  eq(store.proto[sb:pane_id()], nil, "an older unframed transport is not negotiated")
  eq(store.given_up[sb:pane_id()], true, "the incompatible backend is retired")
  eq(#sb.sent, before, "no auth token is disclosed to the incompatible backend")
  store.given_up[sb:pane_id()] = nil
  for domain in pairs(store.failed_domains) do
    store.failed_domains[domain] = nil
  end
end)

test("only the current settings pane may apply delayed effects or run hooks", function()
  local win, gui = H.window(1)
  local _, current = win:spawn_tab { args = { "/bin/wez-vtabs", "--role", "settings" } }
  local _, stale = win:spawn_tab { args = { "/bin/wez-vtabs", "--role", "settings" } }
  state.set_token(current:pane_id(), "current-settings")
  current.vars.vtabs_token = "current-settings"
  state.set_token(stale:pane_id(), "stale-settings")
  stale.vars.vtabs_token = "stale-settings"
  store.proto[current:pane_id()] = protocol.VERSION
  sidebar.set_capabilities(current, { "atomic_sync", "settings_document" })

  local hooks = 0
  config.setup {
    width = 28,
    hooks = {
      theme = function(theme)
        hooks = hooks + 1
        return theme
      end,
    },
    settings = false,
    backend = { path = "/bin/wez-vtabs" },
  }
  require("vtabs.view").sync(gui)
  local wire = require "vtabs.wire"
  local source_rev = assert(wire.revision(gui:window_id(), "settings"))
  require("vtabs.input").handle(
    gui,
    current,
    "vtabs",
    string.format(
      '{"t":"settings_commit","settings_rev":%d,"path":["width"],'
        .. '"change":{"op":"set","value":30},"mode":"instant",'
        .. '"persistence_json":"{\\"version\\":1,\\"options\\":{}}"}',
      source_rev
    )
  )
  eq(config.get().width, 30, "the current pane's current revision is accepted")
  for _, revision in ipairs { source_rev, source_rev + 99 } do
    require("vtabs.input").handle(
      gui,
      current,
      "vtabs",
      string.format(
        '{"t":"settings_commit","settings_rev":%d,"path":["width"],'
          .. '"change":{"op":"set","value":98},"mode":"instant",'
          .. '"persistence_json":"{\\"version\\":1,\\"options\\":{}}"}',
        revision
      )
    )
  end
  require("vtabs.input").handle(
    gui,
    current,
    "vtabs",
    '{"t":"settings_commit","path":["width"],"change":{"op":"set","value":97},'
      .. '"mode":"instant","persistence_json":"{\\"version\\":1,\\"options\\":{}}"}'
  )
  eq(config.get().width, 30, "stale, future and missing revisions are inert")
  require("vtabs.input").handle(
    gui,
    stale,
    "vtabs",
    '{"t":"settings_commit","path":["width"],"change":{"op":"set","value":99},'
      .. '"mode":"instant","persistence_json":"{\\"version\\":1,\\"options\\":{}}"}'
  )
  eq(config.get().width, 30, "a duplicate settings pane cannot mutate config")
  require("vtabs.input").handle(gui, stale, "vtabs", '{"t":"theme_hook_request","generation":7,"theme":{}}')
  eq(hooks, 0, "a duplicate settings pane cannot invoke the theme hook")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("raw settings ownership is capability-gated without rebuilding a Lua model", function()
  local win, gui = H.window(1)
  local _, pane = win:spawn_tab { args = { "/bin/wez-vtabs", "--role", "settings" } }
  state.set_token(pane:pane_id(), "settings-cap")
  pane.vars.vtabs_token = "settings-cap"
  store.proto[pane:pane_id()] = protocol.VERSION

  require("vtabs.view").sync(gui)
  eq(#pane.sent, 0, "an older backend is not sent an unknown raw settings command")

  sidebar.set_capabilities(pane, { "atomic_sync", "settings_document" })
  require("vtabs.view").sync(gui)
  assert(pane.sent[#pane.sent]:find('"t":"settings"', 1, true), "a capable backend receives the raw document")
end)

test("an oversized settings section never begins a partial atomic generation", function()
  local win, gui = H.window(1)
  local _, pane = win:spawn_tab { args = { "/bin/wez-vtabs", "--role", "settings" } }
  state.set_token(pane:pane_id(), "settings-large")
  pane.vars.vtabs_token = "settings-large"
  store.proto[pane:pane_id()] = protocol.VERSION
  sidebar.set_capabilities(pane, { "atomic_sync", "settings_document" })
  config.setup {
    backend = { path = "/bin/wez-vtabs", env = { LARGE = string.rep("x", protocol.LINE_MAX) } },
  }
  require("vtabs.view").sync(gui)
  eq(#pane.sent, 0, "no begin or section is written past the line cap")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
  require("vtabs.view").sync(gui)
  assert(pane.sent[#pane.sent]:find('"t":"begin"', 1, true), "a later bounded document can publish")
end)

test("a new background atomic pane catches up at the current generation", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  local view = require "vtabs.view"
  local shown, background = sidebar.find(win.tab_list[1]), sidebar.find(win.tab_list[2])
  store.proto[shown:pane_id()] = protocol.VERSION
  sidebar.set_capabilities(shown, { "atomic_sync" })
  local background_before = #background.sent
  view.sync(gui)
  local first_generation = batch_generation(shown.sent[#shown.sent])
  win.tab_list[2]:set_title "new title"
  view.sync(gui)
  local current = batch_generation(shown.sent[#shown.sent])
  assert(current > first_generation, "a semantic model change advances the window generation")

  store.proto[background:pane_id()] = protocol.VERSION
  sidebar.set_capabilities(background, { "atomic_sync" })
  view.sync(gui)
  eq(#background.sent, background_before + 1, "bare background pane receives one catch-up write")
  local batch = background.sent[#background.sent]
  eq(batch_generation(batch), current, "catch-up does not invent another generation")
  local lines = batch_lines(batch)
  eq(#lines, 6, "catch-up carries every sidebar section")
end)

test("a failed atomic write advances no seen section and retries the same generation", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local view = require "vtabs.view"
  local sb = sidebar.find(win.tab_list[1])
  store.proto[sb:pane_id()] = protocol.VERSION
  sidebar.set_capabilities(sb, { "atomic_sync" })
  view.sync(gui)
  local sent = #sb.sent
  win.tab_list[1]:set_title "retry me"
  sb.fail_send = true
  view.sync(gui)
  sb.fail_send = false
  eq(#sb.sent, sent, "the failing send records no delivery")
  local failed_generation = batch_generation(sb.last_send_attempt)
  assert(failed_generation, "the failed batch was attempted")
  view.sync(gui)
  eq(#sb.sent, sent + 1, "the unchanged semantic state is retried")
  eq(batch_generation(sb.sent[#sb.sent]), failed_generation, "retry keeps the same window generation")
end)

test("a backend without atomic_sync retains immediate section sends", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local view = require "vtabs.view"
  local sb = sidebar.find(win.tab_list[1])
  store.proto[sb:pane_id()] = protocol.VERSION
  sidebar.set_capabilities(sb, {})
  local before = #sb.sent
  view.sync(gui)
  eq(#sb.sent, before + 4, "legacy config, theme, model, menu stay separate")
  for i = before + 1, #sb.sent do
    assert(not sb.sent[i]:find('"t":"begin"', 1, true), "legacy has no transaction envelope")
    assert(not sb.sent[i]:find('"t":"commit"', 1, true), "legacy has no transaction envelope")
  end
end)

test("typed and legacy scroll intents share one quiet execution path", function()
  local _, gui, _, sb = H.key_window(1)
  local view = require "vtabs.view"
  local input = require "vtabs.input"
  store.proto[sb:pane_id()] = protocol.VERSION
  view.sync(gui)
  local function models()
    local n = 0
    for _, line in ipairs(sb.sent) do
      n = n + (line:find('"t":"model"', 1, true) and 1 or 0)
    end
    return n
  end
  local before = models()
  input.handle(gui, sb, "vtabs", '{"t":"intent","a":"set_scroll","top":4,"user":true}')
  eq(store.scroll[gui:window_id()], 4, "the typed top-level field is recorded")
  eq(models(), before, "and nothing is sent until the poll")
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"set_scroll","args":{"top":3,"user":true}}')
  eq(store.scroll[gui:window_id()], 3, "legacy do is adapted at the boundary")
  eq(models(), before, "the legacy adapter keeps the typed intent quiet")
  view.sync(gui)
  eq(models(), before + 1, "which carries it")
end)

test("every Lua intent handler is covered by Rust's generated contract", function()
  for name in pairs(require("vtabs.input").INTENT) do
    assert(protocol.INTENT_NAMES[name], "handler missing from Rust intent inventory: " .. name)
  end
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
  store.proto[sb:pane_id()] = protocol.VERSION
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

test("closing the settings page closes its tab whole and records nothing to reopen", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  local settings = require "vtabs.settings"
  assert(settings.open(gui), "opened")
  local page_tab, page = settings.find(gui:mux_window())
  page.vars.vtabs_token = state.token_for(page:pane_id())
  eq(#win.tab_list, 2)
  eq(H.sidebars_in(page_tab), 1, "the page has its own sidebar")
  local pushes = 0
  local real_push = state.push_closed
  state.push_closed = function(...)
    pushes = pushes + 1
    return real_push(...)
  end
  assert(settings.close(gui), "closed")
  state.push_closed = real_push
  eq(#win.tab_list, 1, "the whole tab went, sidebar included: nothing is left holding it alone")
  local closing = win.actions[#win.actions].action
  eq(closing.action, "CloseCurrentTab", "one close for the tab, not a quit for the page first")
  sidebar.ensure(gui)
  eq(pushes, 0, "and the page is not in the closed-tab history")
end)

test("nothing is published while a resize burst is in flight; the settle publishes once it stops", function()
  local win, gui = H.window(1, { attach = true, ready = true, sync = true })
  local view = require "vtabs.view"
  local geometry = require "vtabs.geometry"
  local sb = sidebar.find(win.tab_list[1])
  store.proto[sb:pane_id()] = protocol.VERSION
  win:resize(2)
  view.on_resize(gui)
  eq(sb.cols, 28, "the frame was corrected")
  local sent = #sb.sent
  eq(view.sync(gui), false, "a frame publishes nothing: the sidebar repaints from its own size")
  eq(#sb.sent, sent, "no section crossed the wire mid-burst")
  H.later(geometry.SETTLE_MS, function()
    eq(view.sync(gui), true, "publishable again once the frames have stopped")
  end)
end)

test("a tab a held key is passing through gets no sidebar split into it until the key stops", function()
  local win, gui = H.window(2, { attach = false })
  local geometry = require "vtabs.geometry"
  local wid = gui:window_id()
  local clock = H.clock()
  win.active_tab_ref = win.tab_list[1]
  sidebar.ensure(gui)
  assert(sidebar.find(win.tab_list[1]), "the first tab is served at once")
  geometry.on_switch(wid)
  clock.advance(50)
  geometry.on_switch(wid)
  win.active_tab_ref = win.tab_list[2]
  sidebar.ensure(gui)
  eq(sidebar.find(win.tab_list[2]), nil, "nothing split into a tab the key is passing through")
  clock.advance(300)
  sidebar.ensure(gui)
  assert(sidebar.find(win.tab_list[2]), "served once the switching has stopped")
  clock.restore()
end)

test("frames for a mux domain are held while its link is busy, and flushed in order once quiet", function()
  local link = require "vtabs.link"
  local win, gui = H.window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = H.mark_ready(tab)
  sb.domain, sidebar.content_pane(tab).domain = "e2emux", "e2emux"
  local clock = H.clock()
  wezterm.timers = {}
  local before = #sb.sent
  link.activity(sb)
  eq(link.busy "e2emux", true, "a server-side resize marks the domain busy")
  eq(link.busy "local", false, "the local domain never is")
  assert(sidebar.send(sb, { t = "ping", n = 1 }), "accepted")
  assert(sidebar.send(sb, { t = "ping", n = 2 }))
  eq(#sb.sent, before, "nothing crossed the link")
  eq(#wezterm.timers, 1, "one flush armed")
  clock.advance(100)
  link.activity(sb)
  wezterm.fire_timers()
  eq(#sb.sent, before, "still busy: nothing sent")
  eq(#wezterm.timers, 1, "and the flush re-armed")
  clock.advance(link.QUIET_MS)
  wezterm.fire_timers()
  eq(#sb.sent, before + 1, "flushed in one write")
  local flushed = sb.sent[#sb.sent]
  assert(flushed:find('"n":1', 1, true) < flushed:find('"n":2', 1, true), "in order")
  eq(#wezterm.timers, 0)
  assert(sidebar.send(sb, { t = "ping", n = 3 }))
  eq(#sb.sent, before + 2, "quiet: sent at once")
  link.reset()
  clock.restore()
end)

test("a resize report from a mux pane marks its link busy; one from a local pane marks nothing", function()
  local link = require "vtabs.link"
  local input = require "vtabs.input"
  local win, gui = H.window(2)
  sidebar.ensure(gui)
  local local_sb = H.mark_ready(win.tab_list[1])
  store.proto[local_sb:pane_id()] = protocol.VERSION
  input.handle(gui, local_sb, "vtabs", '{"t":"resize","cols":28,"rows":24,"n":2}')
  eq(link.busy_any(), false)
  actions.activate_tab(gui, win.tab_list[2]:tab_id())
  local remote_sb = H.mark_ready(win.tab_list[2])
  remote_sb.domain, sidebar.content_pane(win.tab_list[2]).domain = "e2emux", "e2emux"
  store.proto[remote_sb:pane_id()] = protocol.VERSION
  input.handle(gui, remote_sb, "vtabs", '{"t":"resize","cols":28,"rows":24,"n":2}')
  eq(link.busy "e2emux", true)
  eq(link.busy_any(), true)
  link.reset()
end)

test("no publish crosses a busy mux link; the next poll after it goes quiet publishes", function()
  local link = require "vtabs.link"
  local view = require "vtabs.view"
  local win, gui = H.window(1)
  sidebar.ensure(gui)
  local sb = H.mark_ready(win.tab_list[1])
  sb.domain, sidebar.content_pane(win.tab_list[1]).domain = "e2emux", "e2emux"
  local clock = H.clock()
  link.activity(sb)
  local before = #sb.sent
  eq(view.sync(gui), false, "skipped")
  eq(#sb.sent, before, "and nothing held either: a publish is remade from a fresh snapshot later")
  clock.advance(link.QUIET_MS)
  assert(view.sync(gui) ~= false, "published once quiet")
  link.reset()
  clock.restore()
end)

-- The inbox transport: frames to a mux pane of this machine as files in its directory, and the
-- closes, gates and key handovers that stop crossing the link with it (transport.lua).

local transport = require "vtabs.transport"
local view = require "vtabs.view"
local input = require "vtabs.input"
local wire = require "vtabs.wire"

local function count(list, needle)
  local n = 0
  for _, text in ipairs(list) do
    if text:find(needle, 1, true) then
      n = n + 1
    end
  end
  return n
end

local function last(list)
  return list[#list]
end

test("a mux sidebar moves onto its inbox: probe as message 1, barrier on stdin, then every batch is a file", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    local before = #sb.sent
    local id = fake.ready(gui, sb)
    assert(id:match "^inbox%-%d+%-%x+$", id)
    eq(#sb.sent, before + 2, "auth, then the barrier: nothing else typed")
    assert(sb.sent[before + 1]:find('"auth"', 1, true), "auth first")
    assert(sb.sent[before + 1]:find('"keys":"server"', 1, true), "keys are delivered on the server")
    assert(sb.sent[before + 2]:find('"transport_barrier"', 1, true), "the barrier last")
    eq(sb.probed, id, "the probe was in the directory when the barrier arrived")
    eq(transport.state(sb), "active")
    assert(last(sb.events):find('"transport_ready"', 1, true))
    eq(#sb.inbox_msgs, 1, "the publish the ready set off is one file in the inbox")
    assert(last(sb.inbox_msgs):find('"t":"begin"', 1, true) and last(sb.inbox_msgs):find('"t":"commit"', 1, true))
    eq(transport.inspect(sb).next_seq, 3, "the probe was message 1, the publish message 2")
    local typed = #sb.sent
    win.tab_list[1]:set_title "renamed"
    assert(view.sync(gui) ~= false, "published")
    eq(#sb.sent, typed, "nothing crossed the link")
    eq(#sb.inbox_msgs, 2, "one file for the batch")
    for n = 1, 3 do
      assert(sidebar.send(sb, { t = "ping", n = n }))
    end
    eq(#sb.inbox_msgs, 5, "one file per send")
    eq(#fake.listing(sb.inbox_dir), 0, "each read and deleted")
    for n = 1, 3 do
      assert(sb.inbox_msgs[2 + n]:find('"n":' .. n, 1, true), "in sequence")
    end
    eq(transport.inspect(sb).next_seq, 7)
  end)
end)

test("frames sent while the inbox is negotiated wait in order and go into it on ready", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    sb.hold_events = true
    fake.ready(gui, sb)
    eq(transport.state(sb), "negotiating")
    local typed = #sb.sent
    local queued = transport.inspect(sb).queued
    eq(queued, 1, "the publish the ready set off waits")
    assert(sidebar.send(sb, { t = "ping", n = 1 }), "accepted")
    assert(sidebar.send(sb, { t = "ping", n = 2 }))
    eq(#sb.sent, typed, "nothing typed")
    eq(#fake.listing(sb.inbox_dir), 0, "nothing written before the ack")
    eq(transport.inspect(sb).queued, 3)
    fake.deliver(sb)
    eq(transport.state(sb), "active")
    eq(#sb.inbox_msgs, 3, "the queue went into the inbox")
    assert(sb.inbox_msgs[1]:find('"t":"begin"', 1, true), "the publish first")
    assert(sb.inbox_msgs[2]:find('"n":1', 1, true) and sb.inbox_msgs[3]:find('"n":2', 1, true), "in order")
    eq(#sb.sent, typed, "and never crossed the link")
  end)
end)

test("a refused negotiation types its queue on stdin, in order, and stays there", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    fake.lose_probe = true
    sb.hold_events = true
    fake.ready(gui, sb)
    assert(last(sb.events):find('"transport_refused"', 1, true), "the barrier found no probe")
    assert(last(sb.events):find('"why":"probe"', 1, true))
    eq(transport.state(sb), "negotiating", "the answer has not reached the plugin yet")
    local typed = #sb.sent
    assert(sidebar.send(sb, { t = "ping", n = 1 }))
    assert(sidebar.send(sb, { t = "ping", n = 2 }))
    fake.deliver(sb)
    eq(transport.state(sb), "off")
    eq(#sb.sent, typed + 1, "one write for the queue")
    local flushed = last(sb.sent)
    assert(flushed:find('"n":1', 1, true) < flushed:find('"n":2', 1, true), "in order")
    eq(sb.stopped, nil, "a backend that refused is not told to stop")
    assert(sidebar.send(sb, { t = "ping", n = 3 }))
    eq(#sb.sent, typed + 2, "stdin from here")
    eq(#sb.inbox_msgs, 0)
  end)
end)

test("a negotiation nobody answers is given up after 2 s: queue on stdin, stop sent, late ack ignored", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    local clock = H.clock()
    wezterm.timers = {}
    sb.hold_events = true
    fake.ready(gui, sb)
    eq(#wezterm.timers, 1, "the clock is armed")
    local typed = #sb.sent
    assert(sidebar.send(sb, { t = "ping", n = 1 }))
    assert(sidebar.send(sb, { t = "ping", n = 2 }))
    clock.advance(1000)
    wezterm.fire_timers()
    eq(transport.state(sb), "negotiating", "still inside the bound")
    eq(#wezterm.timers, 1, "re-armed for the rest")
    clock.advance(transport.NEGOTIATE_MS)
    wezterm.fire_timers()
    eq(transport.state(sb), "off")
    eq(#sb.sent, typed + 2, "the queue in one write, then the stop")
    local flushed = sb.sent[typed + 1]
    assert(flushed:find('"n":1', 1, true) < flushed:find('"n":2', 1, true), "in order")
    assert(last(sb.sent):find('"transport_stop"', 1, true))
    eq(sb.stopped, 1, "the backend heard it")
    fake.deliver(sb)
    eq(transport.state(sb), "off", "the late ready changes nothing")
    clock.restore()
  end)
end)

test("a write that fails loses that batch alone: stop sent, no replay, stdin from there", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    fake.ready(gui, sb)
    view.sync(gui)
    assert(wire.dressed(sb:pane_id()))
    local typed, filed = #sb.sent, #sb.inbox_msgs
    fake.fail.close = true
    eq(sidebar.send(sb, { t = "ping", n = 1 }), false)
    fake.fail.close = nil
    eq(#fake.listing(sb.inbox_dir), 0, "no tmp left behind")
    eq(transport.state(sb), "off")
    assert(last(sb.sent):find('"transport_stop"', 1, true), "the backend drains and stops")
    eq(sb.stopped, 1)
    eq(wire.dressed(sb:pane_id()), false, "the wire forgets what it delivered")
    assert(sidebar.send(sb, { t = "ping", n = 2 }))
    eq(#sb.sent, typed + 2, "stop, then the next frame on stdin")
    eq(count(sb.sent, '"n":1') + count(sb.inbox_msgs, '"n":1'), 0, "the lost frame is never replayed")
    eq(#sb.inbox_msgs, filed)
  end)
end)

test("an inbox name that is not one bare directory name, or is too long, is never written to", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    for _, name in ipairs { "../evil", "inbox/1", "inbox 1", "inbox.1", string.rep("a", 65) } do
      fake.ready(gui, sb, { inbox = name })
      eq(transport.state(sb), "off", name)
      eq(next(fake.files), nil, "nothing written: " .. name)
    end
    eq(count(sb.sent, '"transport_barrier"'), 0, "no barrier for any of them")
    fake.ready(gui, sb, { inbox = string.rep("a", 64) })
    eq(transport.state(sb), "active", "64 bytes of word characters is the longest name allowed")
  end)
end)

test(
  "only a mux pane of this machine is offered the inbox: never a local one, a remote one, or with the knob off",
  function()
    H.with_inbox(function()
      local win, gui = H.window(1, { attach = true, ready = true })
      local tab = win.tab_list[1]
      local sb = sidebar.find(tab)
      local function offered(opts)
        local typed = #sb.sent
        fake.ready(gui, sb, opts)
        local barriers = count({ table.unpack(sb.sent, typed + 1) }, '"transport_barrier"')
        return transport.state(sb) ~= "off", barriers, sb.sent[typed + 1]
      end
      local on, barriers, auth = offered()
      eq(on, false, "local domain: the PTY is written directly")
      eq(barriers, 0, "no barrier")
      assert(auth:find('"auth"', 1, true) and not auth:find('"keys"', 1, true), "and keys stay with the plugin")
      sb.domain, sidebar.content_pane(tab).domain = "e2essh", "e2essh"
      eq(offered(), false, "a domain of another machine")
      sb.domain, sidebar.content_pane(tab).domain = "localmux", "localmux"
      store.pane_domain[sb:pane_id()] = "localmux@build.example"
      eq(offered(), false, "a unix domain proxied to another host")
      store.pane_domain[sb:pane_id()] = "localmux@"
      config.get().backend.inbox = false
      eq(offered(), false, "the knob")
      config.get().backend.inbox = nil
      local dir = util.runtime_dir
      util.runtime_dir = function()
        return nil
      end
      eq(offered { root = "/run/elsewhere" }, false, "no private directory of this user's to write in")
      util.runtime_dir = dir
      eq(offered(), true)
    end)
  end
)

test("a busy link holds back neither the publish nor the adjust of a sidebar on its inbox", function()
  H.with_inbox(function()
    local link = require "vtabs.link"
    local geometry = require "vtabs.geometry"
    local win, gui = H.mux_window(1)
    local tab = win.tab_list[1]
    local sb = sidebar.find(tab)
    fake.ready(gui, sb)
    view.sync(gui)
    local filed, typed = #sb.inbox_msgs, #sb.sent
    local clock = H.clock()
    wezterm.timers = {}
    link.activity(sb)
    eq(link.busy "localmux", true)
    tab:set_title "renamed"
    assert(view.sync(gui) ~= false, "published while the link is busy")
    eq(#sb.inbox_msgs, filed + 1, "into the inbox")
    eq(#sb.sent, typed, "nothing typed")
    win:resize(10)
    clock.advance(100)
    eq(link.busy "localmux", true)
    assert(geometry.correct(gui), "adjusted at once")
    eq(sb.adjusted, 1, "through the inbox")
    eq(#sb.sent, typed)
    eq(sb.cols, 28)
    clock.restore()
  end)
end)

test("link.lua holds the barrier while the domain is busy; the negotiation completes once it is quiet", function()
  H.with_inbox(function()
    local link = require "vtabs.link"
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    local clock = H.clock()
    wezterm.timers = {}
    link.activity(sb)
    local typed = #sb.sent
    local id = fake.ready(gui, sb)
    eq(#sb.sent, typed, "auth and barrier both held")
    eq(transport.state(sb), "negotiating")
    eq(fake.listing(sb.inbox_dir)[1], "00000001.msg", "the probe is already in place")
    clock.advance(link.QUIET_MS)
    wezterm.fire_timers()
    eq(#sb.sent, typed + 1, "flushed in one write")
    local flushed = last(sb.sent)
    assert(flushed:find('"auth"', 1, true) < flushed:find('"transport_barrier"', 1, true), "auth before barrier")
    eq(transport.state(sb), "active")
    eq(sb.probed, id)
    clock.restore()
  end)
end)

test("no sidebar is split into a tab while its mux link is busy; the next quiet poll splits it", function()
  H.with_inbox(function()
    local link = require "vtabs.link"
    local win, gui = H.window(1, { attach = true, ready = true })
    local tab = win:add_tab { title = "mux", domain = "localmux" }
    win.active_tab_ref = tab
    local clock = H.clock()
    link.activity(tab.pane_list[1])
    sidebar.ensure(gui)
    eq(#tab:panes(), 1, "nothing split while the link is busy")
    clock.advance(link.QUIET_MS)
    sidebar.ensure(gui)
    eq(#tab:panes(), 2, "split once quiet")
    eq(sidebar.find(tab).split_args.set_environment_variables.VTABS_INBOX_ROOT, "/run/vtabs-test")
    clock.restore()
  end)
end)

test(
  "closing the settings page on a mux domain quits its backends; the tab goes with them, nothing by activation",
  function()
    local win, gui = H.window(1, { attach = true, ready = true })
    local settings = require "vtabs.settings"
    assert(settings.open(gui), "opened")
    local page_tab, page = settings.find(gui:mux_window())
    page.vars.vtabs_token = state.token_for(page:pane_id())
    local page_sb = H.mark_ready(page_tab)
    for _, p in ipairs(page_tab.pane_list) do
      p.domain = "localmux"
    end
    local acted, tabs = #win.actions, #win.tab_list
    local pushes = 0
    local real_push = state.push_closed
    state.push_closed = function(...)
      pushes = pushes + 1
      return real_push(...)
    end
    assert(settings.close(gui), "closed")
    assert(last(page.sent):find('"quit"', 1, true), "the page was told to quit")
    assert(last(page_sb.sent):find('"quit"', 1, true), "and so was its sidebar")
    eq(#win.tab_list, tabs - 1, "the tab went with its last pane")
    eq(#win.actions, acted, "no CloseCurrentTab, no CloseCurrentPane")
    sidebar.ensure(gui)
    state.push_closed = real_push
    eq(pushes, 0, "and the page is not in the closed-tab history")
  end
)

test(
  "a mux sidebar that ignores quit is killed by its server id through another backend, never by activation while one is left",
  function()
    H.with_inbox(function()
      local win, gui = H.mux_window(2)
      local tab, helper_tab = win.tab_list[1], win.tab_list[2]
      local sb, helper = sidebar.find(tab), sidebar.find(helper_tab)
      fake.ready(gui, sb, { transport = false })
      eq(store.server_pane[sb:pane_id()], sb.server_id, "the backend's id on its server is kept")
      sb.hung = true
      local clock = H.clock()
      local acted = #win.actions
      sidebar.detach(gui, tab)
      eq(#tab:panes(), 2, "quit went unheard")
      helper.hung = true
      clock.advance(2100)
      sidebar.ensure(gui)
      assert(last(helper.sent):find('"kill"', 1, true), "the helper was asked")
      assert(last(helper.sent):find('"pane":' .. sb.server_id, 1, true), "by server pane id")
      eq(#tab:panes(), 2, "the helper did not hear either")
      clock.advance(2100)
      sidebar.ensure(gui)
      eq(count(helper.sent, '"kill"'), 2, "asked again")
      eq(#win.actions, acted, "and nothing closed by activation")
      helper.hung = nil
      clock.advance(2100)
      sidebar.ensure(gui)
      eq(helper.killed, 1)
      eq(#tab:panes(), 1, "the server killed it")
      eq(#win.actions, acted)
      clock.restore()
    end)
  end
)

test("on a mux domain a backend no other backend can reach is the one closed by activation", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local tab = win.tab_list[1]
    local sb = sidebar.find(tab)
    fake.ready(gui, sb, { transport = false })
    sb.hung = true
    local clock = H.clock()
    local acted = #win.actions
    win.active_tab_ref = tab
    sidebar.detach(gui, tab)
    clock.advance(2100)
    sidebar.ensure(gui)
    eq(#tab:panes(), 1, "closed by activation: the server has no backend left to ask")
    eq(#win.actions, acted + 1)
    clock.restore()
  end)
end)

test("a key the backend delivered on its server only moves the focus; the plugin types nothing", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local tab = win.tab_list[1]
    local sb, content = sidebar.find(tab), sidebar.content_pane(tab)
    fake.ready(gui, sb)
    sb:activate()
    state.set_focus(gui:window_id(), false)
    local typed = #content.sent
    fake.server_key(sb, "a")
    eq(content.typed[1], "a", "the server typed it")
    eq(#content.sent, typed, "the plugin did not")
    eq(tab.active, content, "focus handed over")
  end)
end)

test("a message the backend never received has the wire resend every section", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    fake.ready(gui, sb)
    view.sync(gui)
    local filed = #sb.inbox_msgs
    view.sync(gui)
    eq(#sb.inbox_msgs, filed, "nothing changed: nothing sent")
    input.handle(gui, sb, "vtabs", '{"t":"dropped","what":"message","seq":2,"n":9}')
    eq(#sb.inbox_msgs, filed + 1, "resent at once")
    local batch = last(sb.inbox_msgs)
    for _, kind in ipairs { "config", "theme", "spaces", "model", "menu" } do
      assert(batch:find('"t":"' .. kind .. '"', 1, true), kind .. " resent")
    end
    eq(transport.state(sb), "active", "the transport stays up")
  end)
end)

test("a fresh ready drops what waited for the old session and negotiates the announced one", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    sb.hold_events = true
    local old = fake.ready(gui, sb)
    local queued = transport.inspect(sb).queued
    assert(sidebar.send(sb, { t = "ping", n = 1 }))
    eq(transport.inspect(sb).queued, queued + 1)
    sb.hold_events = nil
    local new = fake.ready(gui, sb)
    assert(new ~= old)
    eq(transport.inspect(sb).session, new)
    eq(transport.state(sb), "active")
    eq(count(sb.inbox_msgs, '"n":1') + count(sb.sent, '"n":1'), 0, "the stale frame went nowhere")
    eq(#sb.inbox_msgs, 1, "the wire republished after the ready, into the new inbox")
    assert(last(sb.inbox_msgs):find('"t":"commit"', 1, true))
  end)
end)

test("the 65th frame waiting on a negotiation gives it up: everything typed in order, then stop", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    sb.hold_events = true
    fake.ready(gui, sb)
    local typed = #sb.sent
    local pings = transport.QUEUE_MAX - transport.inspect(sb).queued
    for n = 1, pings do
      assert(sidebar.send(sb, { t = "ping", n = n }))
    end
    eq(#sb.sent, typed, "64 wait")
    eq(transport.inspect(sb).queued, transport.QUEUE_MAX)
    assert(sidebar.send(sb, { t = "ping", n = pings + 1 }), "still accepted")
    eq(transport.state(sb), "off")
    eq(#sb.sent, typed + 2, "one write for the queue, one for the stop")
    local flushed = sb.sent[typed + 1]
    local at = flushed:find('"t":"commit"', 1, true)
    assert(at, "the publish first")
    for n = 1, pings + 1 do
      local here = flushed:find('"n":' .. n .. "}", 1, true) or flushed:find('"n":' .. n .. ",", 1, true)
      assert(here and here > at, "in order: " .. n)
      at = here
    end
    assert(last(sb.sent):find('"transport_stop"', 1, true))
  end)
end)

test("the fake backend reads its inbox in sequence and waits at a gap", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    fake.ready(gui, sb)
    local dir = sb.inbox_dir
    local function file(seq, body)
      local tmp, msg = string.format("%s/%08d.tmp", dir, seq), string.format("%s/%08d.msg", dir, seq)
      fake.files[tmp] = body
      fake.fs.rename(tmp, msg)
    end
    local read, seq = #sb.inbox_msgs, sb.last_seq + 1
    file(seq + 1, "later")
    eq(#sb.inbox_msgs, read, "the next one is missing: nothing applied")
    eq(#fake.listing(dir), 1, "and the file waits")
    file(seq, "sooner")
    eq(#sb.inbox_msgs, read + 2, "the gap closed: both, in order")
    eq(sb.inbox_msgs[read + 1], "sooner")
    eq(sb.inbox_msgs[read + 2], "later")
    eq(#fake.listing(dir), 0)
  end)
end)

test("keys go to the server only where the plugin would hand them over, and follow the hover mode", function()
  H.with_inbox(function()
    local win, gui = H.mux_window(1)
    local sb = sidebar.find(win.tab_list[1])
    fake.ready(gui, sb)
    assert(last(sb.sent):find('"transport_barrier"', 1, true))
    eq(store.keys_mode[sb:pane_id()], "server")
    config.get().hover = "press"
    local before = #sb.sent
    input.tick(gui)
    eq(#sb.sent, before + 1, "one same-token auth")
    local auth = last(sb.sent)
    assert(auth:find('"auth"', 1, true) and not auth:find('"keys"', 1, true), "keys back with the plugin")
    eq(store.keys_mode[sb:pane_id()], "host")
    input.tick(gui)
    eq(#sb.sent, before + 1, "nothing more while the mode holds")
    config.get().hover = "follow"
    input.tick(gui)
    assert(last(sb.sent):find('"keys":"server"', 1, true), "and out to the server again")
    eq(transport.state(sb), "active", "the same token never reset the transport")
  end)
end)

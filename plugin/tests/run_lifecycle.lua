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

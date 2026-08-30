local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local state = require "vtabs.state"
local model = require "vtabs.model"
local sidebar = require "vtabs.sidebar"
local input = require "vtabs.input"
local fake = require "fake_mux"
local backend = require "vtabs.backend"

local test, eq, sidebars_in, attach_all = H.test, H.eq, H.sidebars_in, H.attach_all
local mark_ready, window = H.mark_ready, H.window

local function write_state(body)
  local f = assert(io.open(state.file, "w"))
  f:write(body)
  f:close()
end

local function read_state()
  local f = assert(io.open(state.file, "r"))
  local body = f:read "a"
  f:close()
  return body
end

---A fresh Lua VM against the same mux: no GLOBAL, no session, the file is all that is left.
local function restart_vm(body)
  if body then
    write_state(body)
  else
    os.remove(state.file)
  end
  wezterm.GLOBAL.vtabs = nil
  for _, tbl in pairs(state.session) do
    for k in pairs(tbl) do
      tbl[k] = nil
    end
  end
  state.reload()
end

local function count_log(needle)
  local n = 0
  for _, line in ipairs(wezterm.log) do
    if line:find(needle, 1, true) then
      n = n + 1
    end
  end
  return n
end

test("the state file holds only a version, pins and closed tabs", function()
  restart_vm()
  state.set_pinned(31, true)
  state.push_closed { cwd = "/tmp", domain = "local" }
  local body = read_state()
  assert(body:find '"version"', "version written")
  assert(not body:find("token", 1, true), "no token on disk")
  assert(not body:find("sidebars", 1, true), "no pane ids on disk")
  assert(not body:find("private", 1, true), "no window ids on disk")
  state.set_pinned(31, false)
  eq(state.pop_closed().cwd, "/tmp")
end)

test("a corrupt state file warns once and starts empty", function()
  restart_vm "not json at all"
  local warned = count_log "state file unreadable"
  assert(warned >= 1, "warned")
  eq(state.pins_pending(), false)
  eq(state.pop_closed(), nil)
  restart_vm "still not json"
  eq(count_log "state file unreadable", warned, "warned once")
end)

test("pins are restored only when a backend pane survived the mux", function()
  local win, gui = window(1)
  local tab = win.tab_list[1]
  restart_vm(string.format('{"version":1,"pinned":{"%d":true},"closed":[]}', tab.id))
  assert(state.pins_pending(), "pins held back")
  eq(state.is_pinned(tab.id), false)
  local ghost = fake.pane(tab, { title = "wez-vtabs:abcd" })
  table.insert(tab.pane_list, 1, ghost)
  sidebar.ensure(gui)
  eq(state.is_pinned(tab.id), true, "adopted after proof the mux lived")
  eq(state.pins_pending(), false)
end)

test("pins are discarded when nothing survived", function()
  local win, gui = window(1)
  local tab = win.tab_list[1]
  restart_vm(string.format('{"version":1,"pinned":{"%d":true},"closed":[]}', tab.id))
  sidebar.ensure(gui)
  eq(state.is_pinned(tab.id), false)
  local real_now = util.now_ms
  util.now_ms = function()
    return real_now() + 5000
  end
  sidebar.ensure(gui)
  util.now_ms = real_now
  eq(state.pins_pending(), false, "dropped after the grace period")
  eq(state.is_pinned(tab.id), false)
end)

test("a backend pane that outlived the GUI is adopted, not duplicated", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  sb:send_text "\27]2;wez-vtabs:abcd\7"
  eq(sb:get_title(), "wez-vtabs:abcd", "OSC 2 sets the pane title")
  local panes = #tab:panes()
  win:reattach()
  restart_vm()
  sidebar.ensure(gui)
  eq(#tab:panes(), panes, "no second sidebar split")
  eq(sidebar.is_ready(sb), false, "not trusted before the echo")
  local token = state.token_for(sb:pane_id())
  assert(token, "fresh token minted")
  assert(sb.sent[#sb.sent]:find(token, 1, true), "re-authed with it")
  sb.vars.vtabs_token = token
  eq(sidebar.is_ready(sb), true)
  eq(sidebars_in(tab), 1)
end)

test("a pane faking the title marker is never trusted and closes nothing", function()
  local win, gui = window(1)
  local tab = win.tab_list[1]
  local liar = fake.pane(tab, { title = "wez-vtabs:dead" })
  table.insert(tab.pane_list, 1, liar)
  sidebar.ensure(gui)
  eq(sidebar.is_ready(liar), false, "adoption is not trust")
  local sent = #liar.sent
  require("vtabs.view").sync(gui, { force = true })
  eq(#liar.sent, sent, "no frames")
  table.remove(tab.pane_list, 2)
  local tabs = #win.tab_list
  sidebar.ensure(gui)
  eq(#win.tab_list, tabs, "tab kept: classification alone never closes")
  local acted = #win.actions
  input.handle(gui, liar, "vtabs", '{"t":"mouse","k":"down","b":"middle","x":3,"y":3}')
  eq(#win.actions, acted, "events ignored")
end)

test("an orphan tab is closed only once its sidebar echoed a token", function()
  local win, gui = window(2)
  attach_all(win, gui)
  local victim = win.tab_list[2]
  local sb = sidebar.find(victim)
  table.remove(victim.pane_list, 2)
  sidebar.ensure(gui)
  eq(#win.tab_list, 2, "unauthenticated sidebar keeps its tab")
  sb.vars.vtabs_token = state.token_for(sb:pane_id())
  sidebar.ensure(gui)
  eq(#win.tab_list, 1)
end)

test("collapsing to hidden leaves an unauthenticated sidebar alone", function()
  local win, gui = window(1)
  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden" }
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  sidebar.set_collapsed(gui, true)
  eq(#tab:panes(), 2, "not closed without a token")
  mark_ready(tab)
  sidebar.ensure(gui)
  eq(#tab:panes(), 1)
  sidebar.set_collapsed(gui, false)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a failed domain is retried after a minute", function()
  local win, gui = window(1)
  state.session.failed_domains["local@"] = util.now_ms() - 61000
  sidebar.ensure(gui)
  eq(sidebars_in(win.tab_list[1]), 1)
  eq(state.session.failed_domains["local@"], nil, "expired entry dropped")
end)

test("windows the mux forgot are dropped from state", function()
  state.set_collapsed(4242, true)
  state.session.hover[4242] = { x = 1, y = 1 }
  state.forget_windows_except { [1] = true }
  eq(state.is_collapsed(4242), false)
  eq(state.session.hover[4242], nil)
end)

test("tokens do not repeat and are 16 bytes of hex", function()
  local a, b = util.random_token(), util.random_token()
  assert(a ~= b, "distinct")
  eq(#a, 32)
  assert(a:match "^%x+$", "hex only")
end)

test("the backend is told the sidebar background", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  eq(backend.env(cfg, "local", nil, "#1e1e2e").VTABS_BG, "#1e1e2e")
  eq(backend.env(cfg, "local", nil, "red").VTABS_BG, nil)
  eq(backend.env(cfg, "local").VTABS_BG, nil)
  local win = window(1)
  sidebar.ensure(win.gui)
  local sb = sidebar.find(win.tab_list[1])
  assert(sb.split_args.set_environment_variables.VTABS_BG:match "^#%x%x%x%x%x%x$", "hex background passed")
end)

test("the marker pattern matches the backend title and nothing else", function()
  for _, title in ipairs { "wez-vtabs:c7a00ec8", "wez-vtabs:00", "wez-vtabs:abcd" } do
    eq(sidebar.marker(title), true, title)
  end
  for _, title in ipairs { "wez-vtabs", "wez-vtabs --version", "wez-vtabs:deploy prod", "my wez-vtabs:00" } do
    eq(sidebar.marker(title), false, title)
  end
end)

test("a marker title never reaches the rendered tab list", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  tab:set_title "wez-vtabs:abcd"
  local built = model.build(gui)
  eq(built[1].title:find "wez%-vtabs", nil)
end)

test("a content pane faking the marker cannot empty its own tab", function()
  local win, gui = window(2)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local shell = sidebar.content_pane(tab)
  shell.title = "wez-vtabs:00"
  sidebar.ensure(gui)
  eq(#win.tab_list, 2, "tab kept")
  eq(sidebar.find(tab):pane_id(), sb:pane_id(), "the ready pane stays the sidebar")
  eq(sidebar.content_pane(tab):pane_id(), shell:pane_id(), "the liar stays content")
  eq(state.token_for(sb:pane_id()) ~= nil, true, "token not revoked")
end)

test("a marker pane before the sidebar in pane order does not steal the role", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local token = state.token_for(sb:pane_id())
  local liar = fake.pane(tab, { title = "wez-vtabs:00" })
  table.insert(tab.pane_list, 1, liar)
  eq(sidebar.find(tab):pane_id(), sb:pane_id(), "ranked above a bare marker")
  sidebar.ensure(gui)
  eq(state.token_for(sb:pane_id()), token, "token kept")
  local sent = #sb.sent
  require("vtabs.view").sync(gui, { force = true })
  assert(#sb.sent > sent, "frames still go to the real sidebar")
  eq(#liar.sent, 0, "the liar is never written to")
end)

test("an adopted pane that never authenticates is handed back to content", function()
  local win, gui = window(1)
  local tab = win.tab_list[1]
  local liar = fake.pane(tab, { title = "wez-vtabs:00" })
  table.insert(tab.pane_list, 1, liar)
  local real_now = util.now_ms
  local clock = real_now()
  util.now_ms = function()
    return clock
  end
  for _ = 1, 10 do
    clock = clock + 3000
    sidebar.ensure(gui)
  end
  util.now_ms = real_now
  eq(state.sidebar_pane_id(tab.id) ~= liar:pane_id(), true, "unmapped after the retry budget")
  assert(#liar.sent <= 5, "auth attempts bounded, got " .. #liar.sent)
  eq(sidebars_in(tab), 1, "a real sidebar was attached instead")
end)

local function marker_tab(win, domain)
  local tab = win.tab_list[1]
  for _, p in ipairs(tab.pane_list) do
    p.domain = domain or p.domain
  end
  local liar = fake.pane(tab, { title = "wez-vtabs:00", domain = domain or "local" })
  table.insert(tab.pane_list, 1, liar)
  return tab, liar
end

test("a marker pane in another domain is never adopted", function()
  local win, gui = window(1)
  local tab, liar = marker_tab(win)
  liar.domain = "desktop"
  sidebar.ensure(gui)
  eq(#liar.sent, 0, "never auth'd")
  sidebar.ensure(gui)
  eq(sidebar.is_backend(liar), false, "treated as content")
  eq(sidebars_in(tab), 1, "the tab still gets a real sidebar")
end)

test("adopt=auto skips domains this process never spawned a backend in", function()
  local win, gui = window(1)
  config.setup { backend = { path = { ["local"] = "/bin/wez-vtabs" } } }
  local tab, liar = marker_tab(win, "desktop")
  sidebar.ensure(gui)
  eq(#liar.sent, 0, "never auth'd")
  sidebar.ensure(gui)
  eq(sidebar.is_backend(liar), false, "treated as content")
  eq(sidebars_in(tab), 1)
end)

test("adopt=true takes over the same pane", function()
  local win, gui = window(1)
  config.setup { adopt = true, backend = { path = { ["local"] = "/bin/wez-vtabs" } } }
  local _, liar = marker_tab(win, "desktop")
  sidebar.ensure(gui)
  assert(liar.sent[1] and liar.sent[1]:find '"auth"', "auth sent")
  eq(sidebar.is_ready(liar), false, "still needs the echo")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("adopt=false never auths a marker pane", function()
  local win, gui = window(1)
  config.setup { adopt = false, backend = { path = "/bin/wez-vtabs" } }
  local tab, liar = marker_tab(win)
  sidebar.ensure(gui)
  eq(#liar.sent, 0, "never auth'd")
  sidebar.ensure(gui)
  eq(sidebars_in(tab), 1, "falls back to a fresh sidebar")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("an adopted pane is abandoned once its window closes", function()
  local win, gui = window(1)
  local _, liar = marker_tab(win)
  sidebar.ensure(gui)
  assert(#liar.sent > 0, "adopted")
  local real_now = util.now_ms
  local clock = real_now()
  util.now_ms = function()
    return clock
  end
  clock = clock + 31000
  sidebar.ensure(gui)
  util.now_ms = real_now
  eq(state.sidebar_pane_id(win.tab_list[1].id) ~= liar:pane_id(), true, "unmapped after 30 s")
end)

test("a re-entrant ensure returns without running", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  mark_ready(tab)
  local content = sidebar.content_pane(tab)
  local depth, deepest, triggered = 0, 0, false
  content.get_user_vars = function(self)
    depth = depth + 1
    deepest = math.max(deepest, depth)
    if not triggered then
      triggered = true
      sidebar.ensure(gui)
    end
    depth = depth - 1
    return self.vars
  end
  sidebar.ensure(gui)
  content.get_user_vars = nil
  assert(triggered, "the inner ensure was attempted")
  eq(deepest, 1, "the inner ensure did not re-enter the body")
end)

test("ensure clears its guard when a pass throws", function()
  local win, gui = window(1)
  local tab = win.tab_list[1]
  local panes = tab.panes
  tab.panes = function()
    error "mux went away"
  end
  eq(pcall(sidebar.ensure, gui), false, "the error is raised, not swallowed")
  tab.panes = panes
  sidebar.ensure(gui)
  eq(sidebars_in(tab), 1, "the next pass runs")
end)

test("detach never closes a pane that is no longer the active one", function()
  local win, gui = window(2)
  sidebar.ensure(gui)
  local tab, other = win.tab_list[1], win.tab_list[2]
  local sb = mark_ready(tab)
  win.active_tab_ref = tab
  -- another handler activates a different tab while the sidebar activation is in flight
  sb.activate = function()
    other:activate()
  end
  local panes, tabs, acted = #tab:panes(), #win.tab_list, #win.actions
  sidebar.detach(gui, tab)
  eq(#tab:panes(), panes, "the sidebar is left open")
  eq(#win.tab_list, tabs, "no tab closed")
  eq(#win.actions, acted, "CloseCurrentPane never ran")
  sb.activate = nil
end)

test("close_orphan never closes a tab that lost focus", function()
  local win, gui = window(2)
  attach_all(win, gui)
  local victim, other = win.tab_list[2], win.tab_list[1]
  local sb = mark_ready(victim)
  table.remove(victim.pane_list, 2)
  win.active_tab_ref = other
  victim.activate = function()
    other:activate()
  end
  local acted = #win.actions
  sidebar.close_orphan(gui, victim, sb)
  eq(#win.tab_list, 2, "the tab survives")
  eq(#win.actions, acted, "CloseCurrentTab never ran")
  victim.activate = nil
end)

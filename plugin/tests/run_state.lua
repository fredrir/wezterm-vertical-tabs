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

test("the state file holds only restorable state", function()
  restart_vm()
  state.set_pinned(31, true)
  state.push_closed { cwd = "/tmp", domain = "local" }
  local body = read_state()
  local saved = wezterm.json_parse(body)
  eq(saved.pinned["31"], true)
  eq(saved.closed["1"].cwd, "/tmp")
  assert(not body:find("token", 1, true), "no token on disk")
  assert(not body:find("sidebars", 1, true), "no pane ids on disk")
  assert(not body:find("private", 1, true), "no window ids on disk")
  state.set_pinned(31, false)
  eq(state.pop_closed().cwd, "/tmp")
end)

test("state outside the current shape starts empty", function()
  local removed = table.concat { "format_", "revi", "sion" }
  restart_vm(string.format('{"%s":1,"pinned":{"31":true},"closed":[]}', removed))
  eq(state.pins_pending(), false)
  eq(state.is_pinned(31), false)
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
  restart_vm(string.format('{"pinned":{"%d":true},"closed":[]}', tab.id))
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
  restart_vm(string.format('{"pinned":{"%d":true},"closed":[]}', tab.id))
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
  local old = state.token_for(sb:pane_id())
  win:reattach()
  restart_vm()
  local sent = #sb.sent
  sidebar.ensure(gui)
  eq(#tab:panes(), panes, "no second sidebar split")
  eq(sidebar.is_ready(sb), false, "not trusted before the echo")
  local token = state.token_for(sb:pane_id())
  assert(token, "fresh token minted")
  assert(token ~= old)
  -- The first auth is framed blind: a fresh GUI has no user vars for the pane. The backend answers
  -- it by publishing the session it holds, and the next auth is framed with that.
  eq(#sb.sent - sent, 2, "two auths: one refused, one accepted")
  local prefix = require("vtabs.gen.protocol").CONTROL_PREFIX
  eq(sb.reannounced, 1, "the backend published the token it holds once")
  assert(sb.sent[sent + 1]:find(prefix .. token .. " ", 1, true), "the first framed with the fresh token")
  assert(sb.sent[#sb.sent]:find(prefix .. old .. " ", 1, true), "the second framed with the published one")
  assert(sb.sent[#sb.sent]:find(token, 1, true), "re-authed with the fresh token")
  eq(sb.control_token, token, "which the backend accepted")
  local auth = wezterm.json_parse(H.control_payload(sb.sent[#sb.sent]))
  eq(auth.token, token, "auth carries the current session")
  sb.vars.vtabs_token = token
  eq(sidebar.is_ready(sb), true)
  eq(sidebars_in(tab), 1)
end)

test("control records use the echoed session as proof while auth rotates to the current token", function()
  local win = window(1)
  local pane = win.tab_list[1].pane_list[1]
  state.set_token(pane:pane_id(), "new-session")
  pane.vars.vtabs_token = "old-session"
  sidebar.auth(pane)
  local framed = pane.sent[#pane.sent]
  local prefix = require("vtabs.gen.protocol").CONTROL_PREFIX
  assert(framed:sub(1, #prefix + #"old-session ") == prefix .. "old-session ")
  local auth = require("wezterm").json_parse(H.control_payload(framed))
  eq(auth.token, "new-session")
end)

test("a pane faking the title marker is never trusted and closes nothing", function()
  local win, gui = window(1)
  local tab = win.tab_list[1]
  local liar = fake.pane(tab, { title = "wez-vtabs:dead" })
  table.insert(tab.pane_list, 1, liar)
  sidebar.ensure(gui)
  eq(sidebar.is_ready(liar), false, "adoption is not trust")
  local sent = #liar.sent
  require("vtabs.view").sync(gui)
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
  state.session.scroll[4242] = 4
  state.forget_windows_except { [1] = true }
  eq(state.is_collapsed(4242), false)
  eq(state.session.scroll[4242], nil)
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

test("backend.env rides under the plugin's own keys", function()
  local cfg = config.setup {
    backend = { path = "/bin/wez-vtabs", env = { VTABS_LOG = "/tmp/vtabs.log", VTABS_USERVAR = "other", DEBUG = 1 } },
  }
  local env = backend.env(cfg, "local")
  eq(env.VTABS_LOG, "/tmp/vtabs.log")
  eq(env.VTABS_USERVAR, "vtabs")
  eq(env.DEBUG, nil)
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
  eq(built[1].raw.title, nil)
  assert(
    not built[1].raw.pane_title or not built[1].raw.pane_title:find "wez%-vtabs",
    "backend markers never leak into raw tab facts"
  )
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
  require("vtabs.view").sync(gui)
  assert(#sb.sent > sent, "the model still goes to the real sidebar")
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

test("detach tells the backend to quit; nothing is activated and no close action runs", function()
  local win, gui = window(2)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  win.active_tab_ref = tab
  sb.activate = function()
    error "the sidebar must not be activated to close it"
  end
  local acted = #win.actions
  sidebar.detach(gui, tab)
  assert(sb.sent[#sb.sent]:find('"quit"', 1, true), "quit sent")
  eq(#tab:panes(), 1, "the pane closed with its process")
  eq(#win.actions, acted, "CloseCurrentPane never ran")
  eq(state.sidebar_pane_id(tab:tab_id()), nil)
end)

test("a backend that will not quit is closed by activation only when nothing on its server can kill it", function()
  local win, gui = window(2)
  sidebar.ensure(gui)
  local tab, other = win.tab_list[1], win.tab_list[2]
  local sb = mark_ready(tab)
  sb.hung = true
  win.active_tab_ref = tab
  local clock = H.clock()
  local acted = #win.actions
  sidebar.detach(gui, tab)
  eq(#tab:panes(), 2, "quit went unheard")
  clock.advance(2100)
  -- the last rung, and the focus race it guards against: another handler moved the focus
  sb.activate = function()
    other:activate()
  end
  sidebar.ensure(gui)
  eq(#tab:panes(), 2, "the sidebar is left open")
  eq(#win.actions, acted, "CloseCurrentPane never ran")
  sb.activate = nil
  win.active_tab_ref = tab
  clock.advance(2100)
  sidebar.ensure(gui)
  eq(#tab:panes(), 1, "closed by activation once the focus holds")
  eq(#win.actions, acted + 1)
  clock.restore()
end)

test("an orphan tab closes with its sidebar's process; the active tab is never touched", function()
  local win, gui = window(2)
  attach_all(win, gui)
  local victim, other = win.tab_list[2], win.tab_list[1]
  local sb = mark_ready(victim)
  table.remove(victim.pane_list, 2)
  win.active_tab_ref = other
  victim.activate = function()
    error "the orphan tab must not be activated to close it"
  end
  local acted = #win.actions
  sidebar.close_orphan(gui, victim, sb)
  eq(#win.tab_list, 1, "the tab went with the pane")
  eq(win.active_tab_ref, other)
  eq(#win.actions, acted, "CloseCurrentTab never ran")
end)

-- What a reload and a spawn hand the inbox transport: the runtime directory, the env, and tokens
-- that live in one Lua VM only (transport.lua, state.lua).

local store = require "vtabs.store"
local transport = require "vtabs.transport"

local function last(list)
  return list[#list]
end

test("the runtime directory is the user's own: XDG_RUNTIME_DIR, else TMPDIR, never /tmp", function()
  local getenv = util.getenv
  local env = {}
  util.getenv = function(name)
    return env[name]
  end
  eq(util.runtime_dir(), nil, "no private directory: no transport")
  env.TMPDIR = "/var/folders/x"
  eq(util.runtime_dir(), "/var/folders/x/wez-vtabs")
  env.XDG_RUNTIME_DIR = "/run/user/1000"
  eq(util.runtime_dir(), "/run/user/1000/wez-vtabs")
  env.XDG_RUNTIME_DIR = ""
  eq(util.runtime_dir(), "/var/folders/x/wez-vtabs", "an empty variable is unset")
  util.getenv = getenv
  local frame = require "vtabs.frame"
  local rect = { w = 100, h = 100, x = 0, y = 0, cw = 50, ch = 50, radius = 0, border_width = 1 }
  local dir = util.runtime_dir
  util.runtime_dir = function()
    return nil
  end
  assert(frame.path_for(1, rect, {}):find "^/tmp/wez%-vtabs/", "the frame keeps its /tmp fallback")
  util.runtime_dir = dir
end)

test("the backend env names the inbox root only for a mux pane of this machine", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local dir = util.runtime_dir
  util.runtime_dir = function()
    return "/run/vtabs-test"
  end
  backend.register_local_domains { unix_domains = { { name = "localmux" } } }
  eq(backend.env(cfg, "localmux", nil).VTABS_INBOX_ROOT, "/run/vtabs-test")
  eq(backend.env(cfg, "local", nil).VTABS_INBOX_ROOT, nil, "a local pane is written to directly")
  eq(backend.env(cfg, "localmux", "build.example").VTABS_INBOX_ROOT, nil, "a proxied domain is another machine")
  eq(backend.env(cfg, "e2essh", nil).VTABS_INBOX_ROOT, nil)
  cfg.backend.inbox = false
  eq(backend.env(cfg, "localmux", nil).VTABS_INBOX_ROOT, nil, "the knob")
  cfg.backend.inbox = nil
  util.runtime_dir = function()
    return nil
  end
  eq(backend.env(cfg, "localmux", nil).VTABS_INBOX_ROOT, nil, "no private directory, no transport")
  util.runtime_dir = dir
end)

test("tokens never reach wezterm.GLOBAL; the sidebar mapping does", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  local shared = wezterm.GLOBAL.vtabs
  eq(shared.tokens, nil)
  eq(shared.sidebars[tostring(win.tab_list[1].id)], sb:pane_id())
end)

test(
  "a reload keeps the mapping but not the token: a fresh one is minted under the old proof and the inbox negotiated again",
  function()
    H.with_inbox(function()
      local win, gui = H.mux_window(1)
      local tab = win.tab_list[1]
      local sb = sidebar.find(tab)
      local old_session = fake.ready(gui, sb)
      local old_token = state.token_for(sb:pane_id())
      eq(sidebar.is_ready(sb), true)
      -- the reload: a new Lua VM with what GLOBAL carried, and nothing of this one's caches
      store.forget_pane(sb:pane_id())
      sidebar.forget_split(tab:tab_id())
      state.reload()
      eq(state.sidebar_pane_id(tab:tab_id()), sb:pane_id(), "the mapping came through")
      eq(state.token_for(sb:pane_id()), nil, "the token did not")
      eq(transport.state(sb), "off")
      sidebar.ensure(gui)
      eq(#tab:panes(), 2, "no second sidebar")
      local token = state.token_for(sb:pane_id())
      assert(token and token ~= old_token, "a fresh token")
      local auth = last(sb.sent)
      assert(auth:find('"auth"', 1, true) and auth:find(token, 1, true), "offered")
      assert(auth:find(old_token, 1, true), "framed with the proof the backend still holds")
      eq(sidebar.is_ready(sb), false, "not trusted before the echo")
      sb.vars.vtabs_token = token
      eq(sidebar.is_ready(sb), true)
      local session = fake.ready(gui, sb)
      assert(session ~= old_session, "a fresh session")
      eq(transport.state(sb), "active")
      eq(sidebars_in(tab), 1)
    end)
  end
)

test("the settings page after a reload is offered a fresh token once, and trusted again on its echo", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  local settings = require "vtabs.settings"
  assert(settings.open(gui))
  local page_tab, page = settings.find(gui:mux_window())
  local old = state.token_for(page:pane_id())
  page.vars.vtabs_token = old
  mark_ready(page_tab)
  eq(sidebar.is_ready(page), true)
  store.forget_pane(page:pane_id())
  state.reload()
  eq(state.token_for(page:pane_id()), nil)
  win.active_tab_ref = page_tab
  sidebar.ensure(gui)
  local token = state.token_for(page:pane_id())
  assert(token and token ~= old, "a fresh token")
  assert(last(page.sent):find(token, 1, true), "offered")
  local sent = #page.sent
  sidebar.ensure(gui)
  eq(#page.sent, sent, "offered once per VM")
  page.vars.vtabs_token = token
  eq(sidebar.is_ready(page), true)
end)

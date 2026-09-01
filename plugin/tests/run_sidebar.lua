local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local input = require "vtabs.input"
local geometry = require "vtabs.geometry"
local fake = require "fake_mux"
local backend = require "vtabs.backend"

local test, eq, sidebars_in, attach_all = H.test, H.eq, H.sidebars_in, H.attach_all
local mark_ready, window = H.mark_ready, H.window

test("ensure attaches one authenticated sidebar per tab and sends auth", function()
  local win, gui = window(2)
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    eq(sidebars_in(tab), 1, "tab " .. tab.id)
    local sb = sidebar.find(tab)
    eq(sb.split_args.size, 28)
    eq(sb.split_args.direction, "Left")
    assert(sb.sent[1]:find '"auth"', "auth sent")
    assert(sb.sent[1]:find(state.token_for(sb:pane_id()), 1, true), "token in auth")
    eq(sidebar.is_ready(sb), false)
    mark_ready(tab)
    eq(sidebar.is_ready(sb), true)
  end
  sidebar.ensure(gui)
  eq(sidebars_in(win.tab_list[1]), 1, "no duplicate on second ensure")
end)

test("attach refuses a tab that already has a sidebar, so a direct caller cannot double it", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  mark_ready(tab)
  eq(sidebars_in(tab), 1)
  local panes = #tab.pane_list
  -- new_tab, new_window and tear_off call attach outside ensure, and a poll can attach first.
  eq(sidebar.attach(tab), nil, "an attached tab refuses a second split")
  eq(sidebars_in(tab), 1)
  eq(#tab.pane_list, panes, "and no pane was added")
  -- The loser of that race would keep its marker and be demoted to content for good.
  state.session.attaching[tab:tab_id()] = nil
  eq(sidebar.attach(tab), nil, "the refusal is not the pending guard")
  eq(#tab.pane_list, panes)
  local before = #win.tab_list
  actions.new_tab(gui)
  eq(#win.tab_list, before + 1)
  eq(sidebars_in(win.tab_list[#win.tab_list]), 1, "and the path that spawns a tab still gets one")
end)

---Makes `own_socket` true and records every `wezterm cli` argv the plugin runs.
-- luacheck: push ignore 122
local function with_cli(fn)
  local real_getenv, real_procinfo, real_run = os.getenv, wezterm.procinfo, wezterm.run_child_process
  local calls = {}
  os.getenv = function(name)
    if name == "WEZTERM_UNIX_SOCKET" then
      return "/tmp/wezterm/gui-sock-4242"
    end
    return real_getenv(name)
  end
  wezterm.procinfo = {
    pid = function()
      return 4242
    end,
  }
  wezterm.run_child_process = function(args)
    calls[#calls + 1] = table.concat(args, " ")
    return true, "", ""
  end
  local ok, err = pcall(fn, calls)
  os.getenv, wezterm.procinfo, wezterm.run_child_process = real_getenv, real_procinfo, real_run
  if not ok then
    error(err, 0)
  end
  return calls
end
-- luacheck: pop

test("a pane split off the sidebar is moved to the content side, not left in its column", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  -- WezTerm split the sidebar, so the new shell shares the sidebar's columns rather than the
  -- content pane's: `panes_with_info` is the only thing that can tell the two apart.
  local stuck = fake.pane(tab, { cols = sb.cols })
  tab.pane_list[#tab.pane_list + 1] = stuck
  stuck.left, stuck.width, stuck.top = 0, sb.cols, 12
  sb.left, sb.width = 0, sb.cols
  content.left, content.width = sb.cols + 1, content.cols

  local calls = with_cli(function()
    assert(sidebar.rescue_splits(gui, tab), "the intruder is rescued")
  end)
  eq(#calls, 1, "one cli call, for the one pane in the wrong column")
  local want = string.format(
    "cli --no-auto-start split-pane --move-pane-id %d --pane-id %d --bottom",
    stuck:pane_id(),
    content:pane_id()
  )
  assert(calls[1]:find(want, 1, true), "moved under the content pane, not the sidebar: " .. calls[1])

  -- The content pane is where it belongs, and the sidebar is not a candidate to move at all.
  stuck.left, stuck.width = sb.cols + 1, stuck.cols
  eq(
    with_cli(function()
      eq(sidebar.rescue_splits(gui, tab), false, "a pane on the content side is left alone")
    end)[1],
    nil
  )
  eq(sidebars_in(tab), 1, "and the sidebar is never the pane that moves")
end)

test("a sidebar split sideways is rescued too, not just one split below it", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  -- A Left/Right split halves the sidebar's own node and puts a divider between the halves, so a
  -- 28-column sidebar becomes 13 with the intruder at 14. Observed from wezterm, not derived: the
  -- sidebar's own edge is then 13, *left* of the pane that just landed beside it, so any test
  -- against the observed edge misses the one split shape that most needs rescuing.
  local stuck = fake.pane(tab, { cols = 14 })
  tab.pane_list[#tab.pane_list + 1] = stuck
  sb.left, sb.width = 0, 13
  stuck.left, stuck.width = 14, 14
  content.left, content.width = 29, content.cols
  local calls = with_cli(function()
    assert(sidebar.rescue_splits(gui, tab), "a sideways split is an intruder too")
  end)
  eq(#calls, 1)
  assert(calls[1]:find("--move-pane-id " .. stuck:pane_id(), 1, true), calls[1])

  -- Mirrored: with the sidebar on the right, the intruder ends where the sidebar begins.
  config.setup { meta = "auto", position = "right", backend = { path = "/bin/wez-vtabs" } }
  sb.left, sb.width = 67, 13
  stuck.left, stuck.width = 52, 14
  content.left, content.width = 0, 51
  calls = with_cli(function()
    assert(sidebar.rescue_splits(gui, tab), "and on the other side")
  end)
  eq(#calls, 1)
  assert(calls[1]:find("--pane-id " .. content:pane_id(), 1, true), calls[1])
  config.setup { meta = "auto", backend = { path = "/bin/wez-vtabs" } }
end)

test("a pane that only claims the sidebar role never relocates anything", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  local content = sidebar.content_pane(tab)
  local stuck = fake.pane(tab, { cols = sb.cols })
  tab.pane_list[#tab.pane_list + 1] = stuck
  sb.left, sb.width = 0, sb.cols
  stuck.left, stuck.width = 0, sb.cols
  content.left, content.width = sb.cols + 1, content.cols
  -- Never authenticated: it holds the role by its title alone.
  eq(
    with_cli(function()
      eq(sidebar.rescue_splits(gui, tab), false, "an unauthenticated sidebar decides nothing")
    end)[1],
    nil
  )
  mark_ready(tab)
  assert(#with_cli(function()
    sidebar.rescue_splits(gui, tab)
  end) > 0, "and once it has authenticated, it does")
end)

test("split targets the content pane, whichever pane the pointer left active", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local content = sidebar.content_pane(tab)
  -- Under hover = follow the sidebar holds the pane, which is exactly when SplitPane misfires.
  sb:activate()
  local before = #tab.pane_list
  local pane = actions.split(gui, "Bottom")
  assert(pane, "a pane was created")
  eq(#tab.pane_list, before + 1)
  eq(pane.split_args.direction, "Bottom")
  assert(not sidebar.is_backend(pane), "the new pane is a shell, not a second sidebar")
  eq(pane._tab, tab)
  eq(sidebars_in(tab), 1, "and splitting never doubles the sidebar")
  assert(content ~= nil)
  local down = actions.split(gui, "Down")
  assert(down, "Down is taken as an alias")
  eq(down.split_args.direction, "Bottom", "for wezterm's own name")
  eq(actions.split(gui, "Up").split_args.direction, "Top")
  local warned = #wezterm.log
  eq(actions.split(gui, "Sideways"), nil, "an unknown direction is refused")
  assert(#wezterm.log > warned, "and says so")
end)

test("a sidebar with a new pane id but a known token is re-adopted, not duplicated", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local token = state.token_for(sb:pane_id())
  sb.id = sb.id + 1000
  tab.id = tab.id + 1000
  sb.vars = { vtabs_token = token }
  eq(sidebar.is_ready(sb), true)
  sidebar.ensure(gui)
  eq(sidebars_in(tab), 1)
  eq(#tab:panes(), 2)
end)

test("a pane spoofing vtabs_role or vtabs_token is never a sidebar", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local content = sidebar.content_pane(tab)
  content.vars = { vtabs_role = "sidebar", vtabs_token = state.token_for(sidebar.find(tab):pane_id()) }
  eq(sidebar.is_ready(content), false)
  sidebar.ensure(gui)
  eq(#win.tab_list, 1, "tab not closed as orphan")
  local before = #win.actions
  input.handle(gui, content, "vtabs", '{"t":"mouse","k":"down","b":"middle","x":3,"y":2}')
  eq(#win.actions, before, "spoofed event ignored")
end)

test("a ready settings page never wins the sidebar slot of its own tab", function()
  local settings = require "vtabs.settings"
  local win, gui = window(1)
  attach_all(win, gui)
  mark_ready(win.tab_list[1])
  local tab = settings.open(gui)
  local _, page = settings.find(win)
  assert(page and sidebar.is_settings(page), "the page opened, in the settings role")
  -- The page authenticates on the same bridge, and first: it is registered before the split.
  page.vars.vtabs_token = state.token_for(page:pane_id())
  eq(sidebar.is_ready(page), true, "it is ready, as a settings pane")
  sidebar.ensure(gui)
  local sb = sidebar.find(tab)
  assert(sb and sb:pane_id() ~= page:pane_id(), "the slot stays with the pane split for it")
  eq(sidebar.is_backend(page), false, "the page ranks none for the sidebar role")
  eq(sidebar.content_pane(tab):pane_id(), page:pane_id(), "the page is the tab's content")
  mark_ready(tab)
  sidebar.ensure(gui)
  eq(sidebar.find(tab):pane_id(), sb:pane_id(), "and keeps it once both panes are ready")
end)

test("orphaned sidebar closes its tab without touching the active tab", function()
  local win, gui = window(2)
  attach_all(win, gui)
  local victim = win.tab_list[2]
  mark_ready(victim)
  table.remove(victim.pane_list, 2)
  sidebar.ensure(gui)
  eq(#win.tab_list, 1)
  eq(win.active_tab_ref, win.tab_list[1])
end)

test("collapsed = hidden detaches, expand re-attaches", function()
  local win, gui = window(2)
  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden" }
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  sidebar.set_collapsed(gui, true)
  for _, tab in ipairs(win.tab_list) do
    eq(sidebars_in(tab), 0)
    eq(#tab:panes(), 1)
  end
  sidebar.set_collapsed(gui, false)
  eq(sidebars_in(win.active_tab_ref), 1, "expand splits the active tab at once")
  for _, tab in ipairs(win.tab_list) do
    if tab ~= win.active_tab_ref then
      eq(sidebars_in(tab), 0, "a background tab waits for its first activation")
    end
  end
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    eq(sidebars_in(tab), 1, "and gets one when visited")
  end
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("switching to a tab attaches its sidebar and corrects its width in the same action", function()
  local win, gui = window(3)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  local target = win.tab_list[3]
  eq(sidebars_in(target), 0, "a background tab has no sidebar until it is activated")
  actions.activate_tab(gui, target.id)
  eq(win.active_tab_ref, target)
  eq(sidebars_in(target), 1, "the switch attaches one without waiting for the poll")
  eq(sidebars_in(win.tab_list[1]), 1, "and does not touch the tab it came from")

  -- A tab whose sidebar drifted is put right by the same call, not a poll later.
  local sb = mark_ready(target)
  win.active_tab_ref = win.tab_list[1]
  geometry.reset(gui:window_id())
  target:set_split(18)
  eq(sb.cols, 18)
  local before = #win.actions
  actions.activate_tab(gui, target.id)
  eq(sb.cols, 28, "the width is corrected on arrival")
  assert(#win.actions > before, "which took an AdjustPaneSize")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a switch while collapsed to hidden attaches nothing, since hidden means no pane at all", function()
  local win, gui = window(2)
  config.setup { meta = "auto", collapsed = "hidden", backend = { path = "/bin/wez-vtabs" } }
  state.set_collapsed(gui:window_id(), true)
  local target = win.tab_list[2]
  actions.activate_tab(gui, target.id)
  eq(sidebars_in(target), 0, "no sidebar is spawned only to be detached again")
  state.set_collapsed(gui:window_id(), false)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("close_tab on a background tab restores the previous active tab", function()
  local win, gui = window(3)
  sidebar.ensure(gui)
  local first = win.tab_list[1]
  win.active_tab_ref = first
  actions.close_tab(gui, win.tab_list[3].id)
  eq(#win.tab_list, 2)
  eq(win.active_tab_ref, first)
end)

test("move_tab_to_slot keeps pin when dropped inside the pinned block", function()
  local win, gui = window(4)
  sidebar.ensure(gui)
  local ids = {}
  for i, t in ipairs(win.tab_list) do
    ids[i] = t.id
  end
  state.set_pinned(ids[1], true)
  state.set_pinned(ids[2], true)
  state.set_pinned(ids[3], true)
  actions.move_tab_to_slot(gui, ids[1], 3)
  eq(win.tab_list[3].id, ids[1])
  eq(state.is_pinned(ids[1]), true)
  actions.move_tab_to_slot(gui, ids[2], 4)
  eq(state.is_pinned(ids[2]), false)
  eq(win.tab_list[4].id, ids[2])
end)

test("reorder keeps hidden tabs in place", function()
  local win, gui = window(4)
  sidebar.ensure(gui)
  local ids = {}
  for i, t in ipairs(win.tab_list) do
    ids[i] = t.id
  end
  actions.reorder(gui, { ids[4], ids[1], ids[3] })
  eq(win.tab_list[1].id, ids[4])
  eq(win.tab_list[2].id, ids[2])
  eq(win.tab_list[3].id, ids[1])
  eq(win.tab_list[4].id, ids[3])
end)

test("activate_relative wraps over visible tabs only", function()
  local win, gui = window(3)
  sidebar.ensure(gui)
  local cfg = config.get()
  cfg.hooks.filter = function(tab)
    return tab.id ~= win.tab_list[2].id
  end
  win.active_tab_ref = win.tab_list[1]
  actions.activate_relative(gui, 1)
  eq(win.active_tab_ref, win.tab_list[3])
  actions.activate_relative(gui, 1)
  eq(win.active_tab_ref, win.tab_list[1])
  cfg.hooks.filter = nil
end)

test("reopen_closed pushes the entry back when spawning fails", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  win.spawn_tab = function()
    error "no such domain"
  end
  state.push_closed { cwd = "/tmp", domain = "gone" }
  actions.reopen_closed(gui)
  assert(state.pop_closed(), "entry restored")
end)

test("backend path resolves per domain and remote bootstrap is inline", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  eq(backend.resolve_path(cfg, "local"), "/bin/wez-vtabs")
  eq(backend.resolve_path(cfg, "desktop"), nil)
  backend.register_local_domains { unix_domains = { { name = "localmux" } } }
  eq(backend.resolve_path(cfg, "localmux"), "/bin/wez-vtabs")
  eq(backend.resolve_path(cfg, "localmux", "macie"), "/bin/wez-vtabs")
  eq(backend.resolve_path(cfg, "localmux", "archie"), nil, "proxied remote host is not local")
  eq(backend.env(cfg, "localmux").VTABS_TARGET, wezterm.target_triple)
  eq(backend.env(cfg, "localmux", "archie").VTABS_TARGET, nil)
  cfg = config.setup { backend = { path = { ["local"] = "/l", archie = "/a" } } }
  eq(backend.resolve_path(cfg, "localmux", "archie"), "/a")
  cfg = config.setup {
    backend = {
      path = function(_, host)
        return host == "archie" and "/h" or "/m"
      end,
    },
  }
  eq(backend.spawn_args(cfg, "localmux", "archie")[1], "/h")
  cfg = config.setup { backend = { path = { ["local"] = "/l", desktop = "/d" } } }
  eq(backend.resolve_path(cfg, "desktop"), "/d")
  cfg = config.setup { backend = {
    path = function(d)
      return "/fn/" .. d
    end,
  } }
  eq(backend.spawn_args(cfg, "x")[1], "/fn/x")
  cfg = config.setup {}
  local remote = backend.spawn_args(cfg, "desktop")
  eq(remote[2], "-c")
  assert(remote[3]:find("wez-vtabs", 1, true), "inline bootstrap script")
  eq(backend.env(cfg, "desktop").VTABS_TARGET, nil)
  eq(backend.env(cfg, "desktop").VTABS_BUILD, "0")
end)

test("a sidebar that never becomes ready is left in place and its domain not retried", function()
  local win, gui = window(2)
  for _, tab in ipairs(win.tab_list) do
    tab.pane_list[1].domain = "desktop"
  end
  config.setup { backend = { path = { ["local"] = "/bin/wez-vtabs", desktop = "/usr/bin/wez-vtabs" } } }
  attach_all(win, gui)
  eq(sidebars_in(win.tab_list[1]), 1)
  for _, tab in ipairs(win.tab_list) do
    state.session.seen[sidebar.find(tab):pane_id()] = 0
  end
  sidebar.ensure(gui)
  sidebar.ensure(gui)
  eq(sidebars_in(win.tab_list[1]), 1, "dead pane left alone")
  eq(#win.tab_list, 2)
  assert(state.session.failed_domains["desktop@"])
  local warned = 0
  for _, line in ipairs(wezterm.log) do
    if line:find("did not start", 1, true) then
      warned = warned + 1
    end
  end
  eq(warned, 1, "warned once")
  state.session.failed_domains["desktop@"] = nil
end)

test("tab_meta sanitises cwd, domain and title at the source", function()
  local win = fake.window(80)
  local tab = win:add_tab { title = "raw\155title" }
  local pane = tab.pane_list[1]
  pane.cwd = { file_path = "/tmp/\155evil" }
  local meta = sidebar.tab_meta(tab, pane)
  eq(meta.cwd, "/tmp/evil")
  eq(meta.title, "rawtitle")
  assert(utf8.len(meta.cwd) and utf8.len(meta.title), "valid utf-8 out")
  assert(util.shorten_path(meta.cwd, 8), "shorten_path no longer raises")
end)

local H = require "support.helpers"
local wezterm = require "wezterm"
local actions = require "vtabs.actions"
local config = require "vtabs.config"
local input = require "vtabs.input"
local keys = require "vtabs.keys"
local model = require "vtabs.model"
local platform = require "vtabs.platform"
local sidebar = require "vtabs.sidebar"
local spaces = require "vtabs.spaces"
local state = require "vtabs.state"
local theme_bridge = require "vtabs.theme_bridge"
local view = require "vtabs.view"
local wire = require "vtabs.wire"
local fake = require "fake_mux"

local test, eq = H.test, H.eq
local BACKEND = { path = "/bin/wez-vtabs" }
local DEFINITIONS = {
  { id = "home", icon = "H" },
  { id = "claude", name = "Claude", icon = "C", theme = { accent = "#f5c2e7" }, match = { proc = "claude" } },
  { id = "$host", icon = "R", match = { remote = true }, theme = "auto" },
}

local function array()
  return wire.array()
end

local function setup(opts)
  opts = opts or {}
  return config.setup {
    spaces = opts.spaces or DEFINITIONS,
    title = opts.title,
    hooks = opts.hooks,
    meta = "auto",
    backend = BACKEND,
  }
end

local function ready_with_spaces(gui, tab)
  local pane = sidebar.find(tab)
  fake.ready(gui, pane, { transport = false })
  return pane
end

local function messages(pane, tag)
  local out = {}
  for _, payload in ipairs(pane.sent) do
    for line in payload:gmatch "[^\n]+" do
      if line:find('"t":"' .. tag .. '"', 1, true) then
        out[#out + 1] = wezterm.json_parse(H.control_payload(line))
      end
    end
  end
  return out
end

local function resolution(wid, tabs, opts)
  opts = opts or {}
  local assignments, ids = {}, {}
  for _, entry in ipairs(tabs) do
    assignments[#assignments + 1] = {
      tab_id = entry.id,
      space = entry.space,
      manual = entry.manual == true,
      fingerprint = entry.fingerprint,
    }
    if entry.visible ~= false then
      ids[#ids + 1] = entry.id
    end
  end
  return {
    t = "spaces_resolved",
    window_id = wid,
    active = opts.active or "home",
    assignments = assignments,
    dynamics = opts.dynamics or {},
    follow = opts.follow,
    last_tabs = opts.last_tabs or {},
    summary = opts.summary or { { id = "home", name = "Home", count = #ids, unseen = false } },
    visible_tab_ids = ids,
    theme_overrides = opts.theme_overrides or {},
    warnings = opts.warnings or {},
  }
end

test("Lua sends raw space facts and preserves the final title-hook value", function()
  local win, gui = H.window(1)
  local cfg = setup {
    spaces = {
      { name = "malformed stays raw" },
      { id = "work", match = { title = "pretty*", cwd = {} }, theme = { accent = "#010203" } },
    },
    title = function()
      return "pretty title"
    end,
  }
  local survey = model.survey(gui)
  local body = spaces.body(cfg, gui:window_id(), survey.all, array)
  eq(body.tabs[1]["override"], "pretty title", "Rust receives the final title-hook source")
  eq(body.definitions[1].name, "malformed stays raw", "Lua does not run policy validation")
  eq(body.definitions[2].theme.accent, "#010203", "only host colour values are serialized")
  assert(wire.encode(body):find('"cwd":[]', 1, true), "an empty match list stays a JSON array")
  eq(body.window_id, win:window_id())
end)

test("malformed raw match values remain valid JSON for Rust to reject field-by-field", function()
  local win, gui = H.window(1)
  local cycle = {}
  cycle.self = cycle
  local cfg = setup {
    spaces = {
      { id = "nan", match = { remote = 0 / 0 } },
      { id = "mixed", match = { proc = { "zsh", named = true } } },
      { id = "cycle", match = { cwd = cycle } },
    },
  }
  local encoded = wire.encode(spaces.body(cfg, gui:window_id(), model.survey(gui).all, array))
  assert(encoded:find('"remote":null', 1, true), "non-finite numbers lower to JSON null")
  assert(encoded:find('"proc":null', 1, true), "mixed tables lower to JSON null")
  assert(encoded:find('"cwd":{"self":null}', 1, true), "cycles terminate at JSON null")
  assert(wezterm.json_parse(encoded), "one bad raw field cannot poison the spaces message")
  eq(win:window_id(), gui:window_id())
end)

test("before the first spaces result the projection is one unpartitioned tab list", function()
  local win, gui = H.window(2)
  local cfg = setup()
  local items = model.survey(gui).all
  state.set_space(win.tab_list[1].id, "claude", false)
  local projected = spaces.project(cfg, gui:window_id(), items)
  eq(#projected.visible, 2)
  eq(projected.space, nil)
  eq(projected.spaces, nil)
end)

test("a current Rust result atomically supplies assignment, visibility, summary and dynamics", function()
  local win, gui = H.window(2)
  local cfg, wid = setup(), gui:window_id()
  local one, two = win.tab_list[1].id, win.tab_list[2].id
  assert(spaces.accept(
    resolution(wid, {
      { id = one, space = "home", fingerprint = "a" },
      { id = two, space = "scratch", fingerprint = "b", visible = false },
    }, {
      dynamics = { { id = "scratch", name = "Scratch", seq = 1 } },
      last_tabs = { { space_id = "scratch", tab_id = two } },
      summary = {
        { id = "home", name = "Home", count = 1, unseen = false },
        { id = "scratch", name = "Scratch", count = 1, unseen = false },
      },
    }),
    wid
  ))
  eq(state.space_of(one), "home")
  eq(state.space_of(two), "scratch")
  eq(spaces.last_tab_in(wid, "scratch"), two)
  local projected = spaces.project(cfg, wid, model.survey(gui).all)
  eq(#projected.visible, 1)
  eq(projected.visible[1].tab_id, one)
  eq(projected.spaces[2].name, "Scratch")

  assert(not spaces.accept(resolution(wid + 1, { { id = one, space = "wrong" } }), wid), "wrong window")
  eq(state.space_of(one), "home")

  assert(spaces.accept(
    resolution(wid, {
      { id = one, space = "home", fingerprint = "c" },
      { id = two, space = "home", fingerprint = "d" },
    }),
    wid
  ))
  eq(#spaces.body(cfg, wid, model.survey(gui).all, array).dynamics, 0, "empty dynamics are authoritative")
end)

test("each pending route-hook request is evaluated and receives a fieldless answer", function()
  local win, gui = H.window(2, { attach = true })
  local calls = 0
  setup {
    hooks = {
      route = function(facts)
        calls = calls + 1
        eq(facts.title, "final title")
        return facts.tab_id == win.tab_list[2].id and "scratch" or nil
      end,
    },
  }
  local event = {
    window_id = gui:window_id(),
    tabs = {
      { tab_id = win.tab_list[1].id, title = "final title" },
      { tab_id = win.tab_list[2].id, title = "final title" },
    },
  }
  local first, second = sidebar.find(win.tab_list[1]), sidebar.find(win.tab_list[2])
  assert(spaces.answer_hook(gui, first, event))
  assert(spaces.answer_hook(gui, second, event))
  eq(calls, 4, "each pane's pending request runs once per requested tab")
  local a, b = messages(first, "space_route_hook_result"), messages(second, "space_route_hook_result")
  eq(#a, 1)
  eq(#b, 1)
  eq(a[1].routes["2"].space, "scratch")
  eq(b[1].routes["2"].space, "scratch")
end)

test("atomic wire sends full-window spaces facts to a sidebar", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  setup()
  local pane = ready_with_spaces(gui, win.tab_list[1])
  view.sync(gui)
  local sent = messages(pane, "spaces")
  eq(#sent, 1)
  eq(sent[1].window_id, gui:window_id())
  eq(#sent[1].tabs, 2)
  eq(#messages(pane, "begin"), 1)
  eq(#messages(pane, "commit"), 1)
end)

test("policy authority is stable across an active-tab ABA and stays updated in the background", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  setup()
  local wid = gui:window_id()
  local one = win.tab_list[1].id
  local a = sidebar.find(win.tab_list[1])
  local b = sidebar.find(win.tab_list[2])
  view.sync(gui)
  eq(wire.policy_pane(wid), a:pane_id())
  local a_sent = #a.sent
  theme_bridge.clear(wid)

  win.tab_list[2]:activate()
  win.tab_list[2]:set_title "changed while A is background"
  view.sync(gui)
  eq(wire.policy_pane(wid), a:pane_id(), "activation does not hand authority to B")
  assert(#a.sent > a_sent, "the stable authority receives semantic changes while background")

  input.handle(gui, b, "vtabs", wire.encode(resolution(wid, { { id = one, space = "claude" } })))
  input.handle(gui, b, "vtabs", '{"t":"theme_resolved","theme":{"bg":[9,9,9]}}')
  eq(state.space_of(one), nil, "B cannot publish shared state while active")
  eq(theme_bridge.get(wid), nil)

  input.handle(gui, a, "vtabs", wire.encode(resolution(wid, { { id = one, space = "home" } })))
  input.handle(gui, a, "vtabs", '{"t":"theme_resolved","theme":{"bg":[4,5,6]}}')
  eq(state.space_of(one), "home")
  eq(theme_bridge.get(wid).bg[1], 4)

  win.tab_list[1]:activate()
  view.sync(gui)
  eq(wire.policy_pane(wid), a:pane_id(), "returning to A does not revive a different authority")
  input.handle(gui, b, "vtabs", wire.encode(resolution(wid, { { id = one, space = "late" } })))
  eq(state.space_of(one), "home", "B's delayed result remains inert after A is active again")
end)

test("an active settings tab does not displace an existing policy authority", function()
  local win, gui = H.window(1, { attach = true, ready = true })
  setup()
  local content = win.tab_list[1]
  local authority = sidebar.find(content)
  view.sync(gui)
  local settings_tab, settings_pane = win:spawn_tab { args = { "/bin/wez-vtabs", "--role", "settings" } }
  state.set_token(settings_pane:pane_id(), "settings-authority")
  settings_pane.vars.vtabs_token = "settings-authority"
  local wid = gui:window_id()
  local result = wire.encode(resolution(wid, { { id = content.id, space = "claude" } }))
  theme_bridge.clear(wid)

  settings_tab:activate()
  view.sync(gui)
  eq(wire.policy_pane(wid), authority:pane_id())
  input.handle(gui, settings_pane, "vtabs", result)
  input.handle(gui, settings_pane, "vtabs", '{"t":"theme_resolved","theme":{"bg":[1,2,3]}}')
  eq(state.space_of(content.id), nil, "settings activity does not confer policy authority")
  eq(theme_bridge.get(wid), nil)

  input.handle(gui, authority, "vtabs", result)
  input.handle(gui, authority, "vtabs", '{"t":"theme_resolved","theme":{"bg":[4,5,6]}}')
  eq(state.space_of(content.id), "claude")
  eq(theme_bridge.get(wid).bg[1], 4)
end)

test("authority disappearance resets one successor and arms it only after fresh Ready and publish", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  setup()
  local wid = gui:window_id()
  local a = sidebar.find(win.tab_list[1])
  local b = sidebar.find(win.tab_list[2])
  view.sync(gui)
  eq(wire.policy_pane(wid), a:pane_id())
  local old_token = state.token_for(b:pane_id())
  fake.kill_pane(a)
  b.fail_send = true

  view.sync(gui)
  local fresh_token = state.token_for(b:pane_id())
  eq(wire.policy_pane(wid), b:pane_id())
  assert(fresh_token ~= old_token, "handoff rotates the successor token")
  assert(not wire.is_policy_authority(wid, b:pane_id()), "the successor is unarmed before fresh Ready")
  view.sync(gui)
  eq(state.token_for(b:pane_id()), fresh_token, "a failed reset auth does not rotate again")
  b.fail_send = false
  view.sync(gui)
  eq(state.token_for(b:pane_id()), fresh_token, "retry uses the same fresh token")
  local after_reset = #b.sent
  view.sync(gui)
  eq(#b.sent, after_reset, "a successful reset auth is not repeated")

  local current = wire.encode(resolution(wid, { { id = win.tab_list[1].id, space = "home" } }))
  input.handle(gui, b, "vtabs", current)
  eq(state.space_of(win.tab_list[1].id), nil, "a pre-Ready successor result is inert")
  b.vars.vtabs_token = fresh_token
  fake.ready(gui, b, { transport = false })
  assert(wire.is_policy_authority(wid, b:pane_id()), "fresh Ready plus a complete publish arms the successor")
  input.handle(gui, b, "vtabs", current)
  eq(state.space_of(win.tab_list[1].id), "home")
end)

local function authority_timeout_case(auth_write_fails)
  local clock = H.clock()
  local ok, err = pcall(function()
    local win, gui = H.window(3, { attach = true, ready = true })
    setup()
    local wid = gui:window_id()
    local a = sidebar.find(win.tab_list[1])
    local b = sidebar.find(win.tab_list[2])
    local c = sidebar.find(win.tab_list[3])
    view.sync(gui)
    eq(wire.policy_pane(wid), a:pane_id())

    b.fail_send = auth_write_fails
    fake.kill_pane(a)
    view.sync(gui)
    eq(wire.policy_pane(wid), b:pane_id())
    assert(not wire.is_policy_authority(wid, b:pane_id()))

    clock.advance(wire.POLICY_RESET_MS + 1)
    view.sync(gui)
    eq(wire.policy_pane(wid), c:pane_id(), "the timed-out candidate yields to another ready pane")
    assert(not wire.is_policy_authority(wid, c:pane_id()))
    local fresh_token = state.token_for(c:pane_id())
    c.vars.vtabs_token = fresh_token
    fake.ready(gui, c, { transport = false })
    assert(wire.is_policy_authority(wid, c:pane_id()))
  end)
  clock.restore()
  if not ok then
    error(err, 0)
  end
end

test("a successful authority reset auth without fresh Ready times out", function()
  authority_timeout_case(false)
end)

test("a permanently failing authority reset auth times out", function()
  authority_timeout_case(true)
end)

test("a timed-out sole survivor can recover through a new reset-safe handoff", function()
  local clock = H.clock()
  local ok, err = pcall(function()
    local win, gui = H.window(2, { attach = true, ready = true })
    setup()
    local wid = gui:window_id()
    local first = sidebar.find(win.tab_list[1])
    local survivor = sidebar.find(win.tab_list[2])
    view.sync(gui)
    eq(wire.policy_pane(wid), first:pane_id())

    fake.kill_pane(first)
    view.sync(gui)
    eq(wire.policy_pane(wid), survivor:pane_id())
    local timed_out_token = state.token_for(survivor:pane_id())
    assert(not wire.is_policy_authority(wid, survivor:pane_id()))

    clock.advance(wire.POLICY_RESET_MS + 1)
    view.sync(gui)
    eq(wire.policy_pane(wid), nil, "the sole timed-out candidate is dropped")

    local before = #messages(survivor, "begin")
    survivor.vars.vtabs_token = timed_out_token
    fake.ready(gui, survivor, { transport = false })
    eq(wire.policy_pane(wid), survivor:pane_id())
    eq(state.token_for(survivor:pane_id()), timed_out_token, "the late Ready proves the existing reset")
    eq(#messages(survivor, "begin"), before + 1, "recovery publishes a fresh complete batch")
    assert(wire.is_policy_authority(wid, survivor:pane_id()))

    view.sync(gui)
    eq(state.token_for(survivor:pane_id()), timed_out_token, "recovery does not rotate per poll")
  end)
  clock.restore()
  if not ok then
    error(err, 0)
  end
end)

test("a moved authority is not invalidated by either old/new window sync order", function()
  local function exercise(new_window_first)
    local source, source_gui = H.window(1, { attach = true, ready = true })
    setup()
    local tab = source.tab_list[1]
    local pane = sidebar.find(tab)
    view.sync(source_gui)
    eq(wire.policy_pane(source_gui:window_id()), pane:pane_id())

    local destination = fake.window()
    fake.move_tab(source, tab, destination)
    if not new_window_first then
      view.sync(source_gui)
    end
    view.sync(destination.gui)
    local fresh_token = state.token_for(pane:pane_id())
    pane.vars.vtabs_token = fresh_token
    fake.ready(destination.gui, pane, { transport = false })
    assert(wire.is_policy_authority(destination.id, pane:pane_id()))

    if new_window_first then
      view.sync(source_gui)
    end
    assert(wire.is_policy_authority(destination.id, pane:pane_id()), "the old window cannot invalidate the new owner")
    local result = wire.encode(resolution(destination.id, { { id = tab.id, space = "home" } }))
    input.handle(destination.gui, pane, "vtabs", result)
    eq(state.space_of(tab.id), "home")
  end

  exercise(false)
  exercise(true)
end)

test("a closed settings authority hands off reset-safely to its replacement", function()
  local win = fake.window()
  local _, first = win:spawn_tab { args = { "/bin/wez-vtabs", "--role", "settings" } }
  setup()
  state.set_token(first:pane_id(), "settings-first")
  first.vars.vtabs_token = "settings-first"
  view.sync(win.gui)
  eq(wire.policy_pane(win.id), first:pane_id())
  assert(wire.is_policy_authority(win.id, first:pane_id()))

  fake.kill_pane(first)
  view.sync(win.gui)
  eq(wire.policy_pane(win.id), nil)
  local _, replacement = win:spawn_tab { args = { "/bin/wez-vtabs", "--role", "settings" } }
  state.set_token(replacement:pane_id(), "settings-replacement")
  replacement.vars.vtabs_token = "settings-replacement"
  view.sync(win.gui)
  local fresh_token = state.token_for(replacement:pane_id())
  assert(fresh_token ~= "settings-replacement")
  assert(not wire.is_policy_authority(win.id, replacement:pane_id()))

  replacement.vars.vtabs_token = fresh_token
  fake.ready(win.gui, replacement, { transport = false })
  assert(wire.is_policy_authority(win.id, replacement:pane_id()))
end)

test("space convergence settles and a current-pane theme reaches host state", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  setup()
  local pane = ready_with_spaces(gui, win.tab_list[1])
  view.sync(gui)
  local wid = gui:window_id()
  local one, two = win.tab_list[1].id, win.tab_list[2].id
  theme_bridge.clear(wid)

  local resolved_json = string.format(
    '{"t":"spaces_resolved","window_id":%d,"active":"claude",'
      .. '"assignments":[{"tab_id":%d,"space":"home","manual":false,"fingerprint":"a"},'
      .. '{"tab_id":%d,"space":"claude","manual":false,"fingerprint":"b"}],'
      .. '"dynamics":[],"last_tabs":[],"summary":[{"id":"home","name":"Home","count":1,"unseen":false},'
      .. '{"id":"claude","name":"Claude","count":1,"unseen":false}],'
      .. '"visible_tab_ids":[%d],"theme_overrides":{},"warnings":[]}',
    wid,
    one,
    two,
    two
  )
  local before = #pane.sent
  input.handle(gui, pane, "vtabs", resolved_json)
  eq(#pane.sent, before + 1, "the newly applied assignment produces one converging sync")
  input.handle(gui, pane, "vtabs", resolved_json)
  eq(#pane.sent, before + 1, "the converged answer does not publish again")
  input.handle(gui, pane, "vtabs", '{"t":"theme_resolved","theme":{"bg":[4,5,6]}}')
  eq(theme_bridge.get(wid).bg[1], 4)
end)

test("space mux actions remain host-side while policy decisions do not", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  setup()
  ready_with_spaces(gui, win.tab_list[1])
  local wid, one, two = gui:window_id(), win.tab_list[1].id, win.tab_list[2].id
  spaces.accept(
    resolution(wid, {
      { id = one, space = "home" },
      { id = two, space = "claude", visible = false },
    }, {
      last_tabs = { { space_id = "claude", tab_id = two } },
      summary = {
        { id = "home", name = "Home", count = 1, unseen = false },
        { id = "claude", name = "Claude", count = 1, unseen = false },
      },
    }),
    wid
  )
  eq(actions.switch_space(gui, "claude"), true)
  eq(win.active_tab_ref.id, two)
  eq(actions.move_to_space(gui, two, "home", true), true)
  eq(state.space_of(two), "home")
  eq(state.space_manual(two), true)
end)

test("next_space and prev_space keep their public bindings", function()
  local bound = {}
  for _, binding in ipairs(keys.build {}) do
    bound[binding.vtabs] = binding
  end
  eq(bound.next_space.key, "e")
  eq(bound.next_space.mods, platform.SUPER)
  eq(bound.prev_space.mods, platform.SUPER2)
  assert(actions.action.next_space and actions.action.prev_space)
end)

config.setup { backend = BACKEND }

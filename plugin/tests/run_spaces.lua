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
  input.handle(
    gui,
    pane,
    "vtabs",
    '{"t":"ready","v":3,"cols":28,"rows":24,"paints":true,"caps":["atomic_sync","spaces_policy"],"n":1}'
  )
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

local function resolution(wid, generation, tabs, opts)
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
    generation = generation,
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

test("without spaces_policy the projection is one unpartitioned tab list", function()
  local win, gui = H.window(2)
  local cfg = setup()
  local items = model.survey(gui).all
  state.set_space(win.tab_list[1].id, "claude", false)
  local projected = spaces.project(cfg, gui:window_id(), items, false)
  eq(#projected.visible, 2)
  eq(projected.space, nil)
  eq(projected.spaces, nil)
end)

test("a current Rust result atomically supplies assignment, visibility, summary and dynamics", function()
  local win, gui = H.window(2)
  local cfg, wid = setup(), gui:window_id()
  local one, two = win.tab_list[1].id, win.tab_list[2].id
  assert(spaces.accept(
    resolution(wid, 4, {
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
    wid,
    4
  ))
  eq(state.space_of(one), "home")
  eq(state.space_of(two), "scratch")
  eq(spaces.last_tab_in(wid, "scratch"), two)
  local projected = spaces.project(cfg, wid, model.survey(gui).all, true)
  eq(#projected.visible, 1)
  eq(projected.visible[1].tab_id, one)
  eq(projected.spaces[2].name, "Scratch")

  assert(not spaces.accept(resolution(wid, 4, { { id = one, space = "wrong" } }), wid, 4), "duplicate generation")
  assert(not spaces.accept(resolution(wid + 1, 5, { { id = one, space = "wrong" } }), wid, 5), "wrong window")
  assert(not spaces.accept(resolution(wid, 6, { { id = one, space = "wrong" } }), wid, 5), "future result")
  eq(state.space_of(one), "home")

  assert(spaces.accept(
    resolution(wid, 5, {
      { id = one, space = "home", fingerprint = "c" },
      { id = two, space = "home", fingerprint = "d" },
    }),
    wid,
    5
  ))
  eq(#spaces.body(cfg, wid, model.survey(gui).all, array).dynamics, 0, "empty dynamics are authoritative")
end)

test("one route-hook batch is evaluated once per window generation and reused for every pane", function()
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
  local original_generation = wire.generation
  wire.generation = function()
    return 9
  end
  local event = {
    generation = 9,
    window_id = gui:window_id(),
    tabs = {
      { tab_id = win.tab_list[1].id, title = "final title" },
      { tab_id = win.tab_list[2].id, title = "final title" },
    },
  }
  local first, second = sidebar.find(win.tab_list[1]), sidebar.find(win.tab_list[2])
  assert(spaces.answer_hook(gui, first, event))
  assert(spaces.answer_hook(gui, second, event))
  wire.generation = original_generation
  eq(calls, 2, "one call per requested tab, not per backend pane")
  local a, b = messages(first, "space_route_hook_result"), messages(second, "space_route_hook_result")
  eq(#a, 1)
  eq(#b, 1)
  eq(a[1].routes["2"].space, "scratch")
  eq(b[1].routes["2"].space, "scratch")
end)

test("atomic wire sends full-window spaces facts to a capable sidebar", function()
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

test("space convergence advances once and the matching follow-up theme reaches host state", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  setup()
  local pane = ready_with_spaces(gui, win.tab_list[1])
  view.sync(gui)
  local wid = gui:window_id()
  local first_generation = assert(wire.generation(wid))
  local one, two = win.tab_list[1].id, win.tab_list[2].id
  theme_bridge.clear(wid)

  local resolved_json = string.format(
    '{"t":"spaces_resolved","generation":%%d,"window_id":%d,"active":"claude",'
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
  input.handle(gui, pane, "vtabs", string.format(resolved_json, first_generation))
  local follow_up = assert(wire.generation(wid))
  assert(follow_up > first_generation, "the newly applied assignment produces one converging sync")

  input.handle(
    gui,
    pane,
    "vtabs",
    string.format('{"t":"theme_resolved","generation":%d,"theme":{"bg":[1,2,3]}}', first_generation)
  )
  eq(theme_bridge.get(wid), nil, "the superseded generation cannot project host colour")

  input.handle(gui, pane, "vtabs", string.format(resolved_json, follow_up))
  eq(wire.generation(wid), follow_up, "the converged answer does not start a third generation")
  input.handle(
    gui,
    pane,
    "vtabs",
    string.format('{"t":"theme_resolved","generation":%d,"theme":{"bg":[4,5,6]}}', follow_up)
  )
  eq(theme_bridge.get(wid).bg[1], 4)
  eq(wire.generation(wid), follow_up, "host projection does not loop semantic sync")
end)

test("space mux actions remain host-side while policy decisions do not", function()
  local win, gui = H.window(2, { attach = true, ready = true })
  setup()
  ready_with_spaces(gui, win.tab_list[1])
  local wid, one, two = gui:window_id(), win.tab_list[1].id, win.tab_list[2].id
  spaces.accept(
    resolution(wid, 1, {
      { id = one, space = "home" },
      { id = two, space = "claude", visible = false },
    }, {
      last_tabs = { { space_id = "claude", tab_id = two } },
      summary = {
        { id = "home", name = "Home", count = 1, unseen = false },
        { id = "claude", name = "Claude", count = 1, unseen = false },
      },
    }),
    wid,
    1
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

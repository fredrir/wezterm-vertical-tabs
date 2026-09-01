local H = require "support.helpers"
local wezterm = require "wezterm"
local actions = require "vtabs.actions"
local config = require "vtabs.config"
local fake = require "fake_mux"
local frame = require "vtabs.frame"
local input = require "vtabs.input"
local keys = require "vtabs.keys"
local model = require "vtabs.model"
local platform = require "vtabs.platform"
local popover = require "vtabs.popover"
local sidebar = require "vtabs.sidebar"
local spaces = require "vtabs.spaces"
local state = require "vtabs.state"
local store = require "vtabs.store"
local theme = require "vtabs.theme"
local util = require "vtabs.util"
local view = require "vtabs.view"

local test, eq = H.test, H.eq

local BACKEND = { path = "/bin/wez-vtabs" }
local SPACES = {
  { id = "home", icon = "H" },
  { id = "claude", name = "Claude", icon = "C", theme = { accent = "#f5c2e7" }, match = { proc = "claude" } },
  { id = "$host", icon = "R", match = { remote = true }, theme = "auto" },
}

-- Facts are cached for one poll, so every poll here runs on a clock that has moved on.
local real_now = util.now_ms
local clock = nil
local function tick()
  clock = (clock or real_now()) + 1000
  util.now_ms = function()
    return clock
  end
end

local function poll(gui)
  tick()
  return model.survey(gui)
end

local function setup(opts)
  opts = opts or {}
  config.setup {
    spaces = opts.spaces or SPACES,
    hooks = opts.hooks,
    frame = opts.frame,
    meta = "auto",
    backend = BACKEND,
  }
end

---A window of `n` shells with spaces configured; nothing polled yet.
local function spaced(n, opts)
  local win, gui = H.window(n, opts)
  setup(opts)
  return win, gui
end

local function set_process(tab, name)
  tab.pane_list[1].process = "/usr/bin/" .. name
end

local function set_remote(tab, host)
  tab.pane_list[1].cwd = "file://" .. host .. "/home/f"
end

---A sidebar the wire will talk to: ready, and a v2 painter, which only the `ready` event declares.
local function painter(gui, tab)
  local sb = sidebar.find(tab)
  input.handle(gui, sb, "vtabs", '{"t":"ready","v":2,"cols":28,"rows":24,"paints":true,"n":1}')
  return sb
end

local function last_line(pane, kind)
  local found = nil
  for _, line in ipairs(pane.sent) do
    if line:find('"t":"' .. kind .. '"', 1, true) then
      found = line
    end
  end
  return found and wezterm.json_parse(found) or nil
end

local function logged(needle)
  local n = 0
  for _, line in ipairs(wezterm.log) do
    if line:find(needle, 1, true) then
      n = n + 1
    end
  end
  return n
end

local function read_state()
  local f = assert(io.open(state.file, "r"))
  local body = f:read "a"
  f:close()
  return body
end

---A fresh Lua VM against the same mux: the file is all that is left, and no cache survives.
local function restart_vm(win, body)
  local f = assert(io.open(state.file, "w"))
  f:write(body)
  f:close()
  wezterm.GLOBAL.vtabs = nil
  for _, tbl in pairs(state.session) do
    for k in pairs(tbl) do
      tbl[k] = nil
    end
  end
  store.forget_window(win:window_id())
  for _, tab in ipairs(win.tab_list) do
    store.forget_tab(tab.id)
  end
  state.reload()
end

test("rules: globs are anchored, lists are any-of, a bare cwd is a prefix, every field must match", function()
  local m = spaces.matches
  eq(m({ proc = "cla*" }, { proc = "claude" }), true)
  eq(m({ proc = "cla*" }, { proc = "xclaude" }), false, "anchored at the start")
  eq(m({ proc = "claude" }, { proc = "claude2" }), false, "anchored at the end")
  eq(m({ proc = "a.b" }, { proc = "axb" }), false, "a dot is literal")
  eq(m({ domain = { "tls:*", "ssh:*" } }, { domain = "ssh:pi" }), true, "a list is any-of")
  eq(m({ cwd = "~/work" }, { cwd = "~/work/x" }), true, "a bare cwd is a prefix")
  eq(m({ cwd = "~/work" }, { cwd = "~/workshop" }), false, "of whole segments")
  eq(m({ cwd = "~/work/*" }, { cwd = "~/work" }), false, "a glob is only a glob")
  eq(m({ proc = "claude", host = "pi" }, { proc = "claude" }), false, "a missing fact never matches")
  eq(m({ remote = true }, { remote = false }), false)
  eq(m(nil, { proc = "zsh" }), false, "an entry without match is not a rule")
  local cfg = config.setup { spaces = SPACES, backend = BACKEND }
  eq(spaces.route(cfg, { proc = "claude" }), "claude")
  eq(spaces.route(cfg, { proc = "zsh" }), nil, "the default entry routes nothing")
  eq(spaces.route(cfg, { remote = true, host = "pi" }), "pi", "$host expands")
  eq(spaces.route(cfg, { remote = true }), nil, "a template missing its fact is skipped")
  eq(spaces.expand("$host", { host = string.rep("x", 60) }), string.rep("x", 48), "capped")
end)

test("validate: entries without an id, duplicates and bad fields are dropped with one warning each", function()
  local cfg = config.setup {
    spaces = { { name = "no id" }, { id = "a" }, { id = "a" }, { id = "b", theme = 3 }, { id = "c", match = "zsh" } },
    backend = BACKEND,
  }
  eq(#cfg.spaces, 3)
  eq(cfg.spaces[1].id, "a")
  eq(cfg.spaces[2].theme, nil, "a bad theme is dropped, the entry kept")
  eq(cfg.spaces[3].match, nil, "so is a bad match")
  eq(logged "spaces: an entry without an id", 1)
  eq(logged "duplicate id", 1)
  eq(config.setup({ spaces = "home", backend = BACKEND }).spaces[1], nil, "a non-list resets without raising")
  eq(logged "spaces must be a list of table entries", 1)
end)

test("without spaces nothing changes: every tab is visible and the wire carries no spaces", function()
  local win, gui = H.ready_window(3)
  local survey = model.survey(gui)
  eq(#survey.visible, 3)
  eq(survey.spaces, nil)
  eq(state.space_of(win.tab_list[1].id), nil)
  local sb = painter(gui, win.tab_list[1])
  view.sync(gui)
  local sent = last_line(sb, "model")
  eq(sent.spaces, nil)
  eq(sent.space, nil)
  eq(popover.items(gui, win.tab_list[1].id)[4].disabled, true, "Move to space is greyed out")
end)

test("first sight: a rule places a tab, the rest land in the window's space, the sidebar follows", function()
  local win, gui = spaced(3)
  set_process(win.tab_list[2], "claude")
  win.active_tab_ref = win.tab_list[1]
  local survey = poll(gui)
  eq(state.space_of(win.tab_list[1].id), "home")
  eq(state.space_of(win.tab_list[2].id), "claude")
  eq(state.space_of(win.tab_list[3].id), "home")
  eq(survey.space, "home")
  eq(#survey.visible, 2)
  eq(survey.spaces[1].id, "home")
  eq(survey.spaces[1].count, 2)
  eq(survey.spaces[2].id, "claude")
  eq(survey.spaces[2].count, 1)
  eq(#survey.spaces, 2, "a template with no tab is not listed")
  win.tab_list[2]:activate()
  survey = poll(gui)
  eq(survey.space, "claude", "follows the active tab")
  eq(#survey.visible, 1)
end)

test("sticky: a tab moves into a space that wants it and stays put when it stops matching", function()
  local win, gui = spaced(2)
  poll(gui)
  local tab = win.tab_list[2]
  eq(state.space_of(tab.id), "home")
  set_process(tab, "claude")
  poll(gui)
  eq(state.space_of(tab.id), "claude", "moved in")
  set_process(tab, "zsh")
  poll(gui)
  eq(state.space_of(tab.id), "claude", "no rule wants it back")
  set_process(tab, "ssh")
  set_remote(tab, "pi")
  local survey = poll(gui)
  eq(state.space_of(tab.id), "pi", "a later match moves it again")
  eq(survey.spaces[3].id, "pi")
  eq(survey.spaces[3].icon, "R", "a dynamic space wears its template's icon")
  eq(survey.spaces[3].count, 1)
end)

test("a hand move pins the tab until the Auto row hands it back to the rules", function()
  local win, gui = spaced(2)
  poll(gui)
  local tab = win.tab_list[2]
  eq(actions.move_to_space(gui, tab.id, "claude", true), true)
  eq(state.space_of(tab.id), "claude")
  eq(state.space_manual(tab.id), true)
  set_process(tab, "ssh")
  set_remote(tab, "pi")
  poll(gui)
  eq(state.space_of(tab.id), "claude", "never re-routed while pinned")
  actions.move_to_space(gui, tab.id, nil, false)
  poll(gui)
  eq(state.space_of(tab.id), "pi", "Auto asks the rules again")
  eq(state.space_manual(tab.id), false)
end)

test("moving the active tab hands the view to its neighbour; the last tab of a space takes the view along", function()
  local win, gui = spaced(3)
  win.active_tab_ref = win.tab_list[1]
  poll(gui)
  actions.move_to_space(gui, win.tab_list[1].id, "claude", true)
  eq(win.active_tab_ref, win.tab_list[2], "the next tab in home took over")
  eq(poll(gui).space, "home")
  actions.switch_space(gui, "claude")
  eq(win.active_tab_ref, win.tab_list[1])
  eq(poll(gui).space, "claude")
  actions.move_to_space(gui, win.tab_list[1].id, "home", true)
  eq(poll(gui).space, "home", "nothing left in claude, so the sidebar followed the tab")
end)

test("switching to an empty space keeps the current tab on screen and never spawns; a new tab lands there", function()
  local win, gui = spaced(2, { spaces = { { id = "home" }, { id = "scratch" } } })
  poll(gui)
  eq(actions.switch_space(gui, "scratch"), true)
  local survey = poll(gui)
  eq(survey.space, "scratch")
  eq(#survey.visible, 0)
  eq(#win.tab_list, 2, "nothing spawned")
  eq(win.active_tab_ref, win.tab_list[1], "the tab on screen did not change")
  local tab = actions.new_tab(gui)
  survey = poll(gui)
  eq(state.space_of(tab:tab_id()), "scratch")
  eq(#survey.visible, 1)
  eq(actions.switch_space(gui, "nowhere"), false)
  eq(actions.switch_space(gui, "scratch"), false, "already there")
end)

test("cycling wraps through the switcher's order, from a binding and from the sidebar's keyboard mode", function()
  local win, gui = spaced(3)
  set_process(win.tab_list[3], "claude")
  win.active_tab_ref = win.tab_list[1]
  poll(gui)
  actions.cycle_space(gui, 1)
  eq(poll(gui).space, "claude")
  actions.cycle_space(gui, 1)
  eq(poll(gui).space, "home", "wraps")
  actions.cycle_space(gui, -1)
  eq(poll(gui).space, "claude")

  local win2, gui2 = H.ready_window(2)
  setup()
  set_process(win2.tab_list[2], "claude")
  win2.active_tab_ref = win2.tab_list[1]
  poll(gui2)
  state.set_focus(gui2:window_id(), true)
  tick()
  input.key(gui2, sidebar.find(win2.tab_list[1]), { key = "]", mods = {} })
  eq(poll(gui2).space, "claude", "] in keyboard mode")
end)

test("closing the active tab activates its neighbour in the same space, not the physical one", function()
  local win, gui = spaced(3)
  set_process(win.tab_list[1], "claude")
  win.active_tab_ref = win.tab_list[3]
  poll(gui)
  eq(poll(gui).space, "home")
  actions.close_tab(gui, win.tab_list[3].id)
  eq(win.active_tab_ref, win.tab_list[2], "home's other tab, not the physical first")
  eq(poll(gui).space, "home")
end)

test("next/prev tab, close others and the popover count stay inside the space", function()
  local win, gui = spaced(4)
  set_process(win.tab_list[2], "claude")
  set_process(win.tab_list[4], "claude")
  win.active_tab_ref = win.tab_list[1]
  poll(gui)
  actions.activate_relative(gui, 1)
  eq(win.active_tab_ref, win.tab_list[3], "skips claude's tab")
  actions.activate_relative(gui, 1)
  eq(win.active_tab_ref, win.tab_list[1], "wraps inside home")
  eq(#actions.others(gui, win.tab_list[1].id), 1, "close others sees only home")
end)

test("a closed tab remembers its space and a reopened one returns to it", function()
  local win, gui = spaced(2)
  set_process(win.tab_list[2], "claude")
  win.active_tab_ref = win.tab_list[1]
  poll(gui)
  -- the poll that learns which tabs exist, so the next one can tell which left
  sidebar.ensure(gui)
  local gone = win.tab_list[2]
  actions.close_tab(gui, gone.id)
  tick()
  sidebar.ensure(gui)
  actions.reopen_closed(gui)
  local reopened = win.tab_list[#win.tab_list]
  assert(reopened ~= gone, "a new tab")
  eq(state.space_of(reopened.id), "claude")
end)

test("assignments persist with the pins and come back only once the mux is proven alive", function()
  local win, gui = spaced(2)
  set_process(win.tab_list[2], "claude")
  poll(gui)
  local t1, t2 = win.tab_list[1], win.tab_list[2]
  actions.move_to_space(gui, t1.id, "claude", true)
  local body = read_state()
  assert(body:find('"space_of"', 1, true), "assignments on disk")
  assert(body:find('"space_manual"', 1, true), "and the manual flags")
  assert(not body:find("active_space", 1, true), "the active space is per GUI process")
  restart_vm(win, body)
  eq(state.pins_pending(), true)
  eq(state.space_of(t2.id), nil, "held back")
  local ghost = fake.pane(t1, { title = "wez-vtabs:abcd" })
  table.insert(t1.pane_list, 1, ghost)
  tick()
  sidebar.ensure(gui)
  eq(state.space_of(t2.id), "claude")
  eq(state.space_of(t1.id), "claude")
  eq(state.space_manual(t1.id), true)
  eq(state.pins_pending(), false)
  state.forget_windows_except {}
  eq(state.active_space(gui:window_id()), nil, "forgotten with the window")
end)

test("the active space's theme reaches the wire and the frame; a switch bumps the theme rev", function()
  local win, gui = H.ready_window(2)
  setup { frame = { zen = true, border = "accent" } }
  set_process(win.tab_list[2], "claude")
  win.active_tab_ref = win.tab_list[1]
  local sb = painter(gui, win.tab_list[1])
  tick()
  view.sync(gui)
  local home = last_line(sb, "theme")
  eq(home.overrides.accent, nil, "home inherits the user's theme")
  local sent = last_line(sb, "model")
  eq(sent.space, "home")
  eq(#sent.spaces, 2)
  eq(sent.spaces[2].icon, "C")
  eq(sent.spaces[2].active, nil, "the wire carries only the active id")
  actions.switch_space(gui, "claude")
  tick()
  view.sync(gui)
  local claude = last_line(sb, "theme")
  eq(claude.overrides.accent, "#f5c2e7")
  assert(claude.rev > home.rev, "a new rev, so every pane repaints")
  local palette = gui:effective_config().resolved_palette
  local want = theme.resolve(util.merge(config.get().theme, { accent = "#f5c2e7" }), palette).accent
  eq(frame.colours(gui, config.get()).border, string.format("#%02x%02x%02x", want[1], want[2], want[3]))
  eq(spaces.accent_of(config.get(), "pi", palette), spaces.accent_of(config.get(), "pi", palette), "auto is stable")
  assert(spaces.accent_of(config.get(), "pi", palette):match "^#%x%x%x%x%x%x$", "and a hex colour")
end)

test("the route hook runs first; an unknown id is a dynamic space that lives while it holds a tab", function()
  local win, gui = spaced(2, {
    hooks = {
      route = function(meta)
        if meta.title == "t2" then
          return "scratch"
        end
        return nil
      end,
    },
  })
  win.active_tab_ref = win.tab_list[1]
  local survey = poll(gui)
  eq(state.space_of(win.tab_list[2].id), "scratch")
  eq(survey.spaces[3].id, "scratch")
  eq(survey.spaces[3].name, "scratch")
  eq(survey.spaces[3].count, 1)
  actions.switch_space(gui, "scratch")
  eq(poll(gui).space, "scratch")
  actions.close_tab(gui, win.tab_list[2].id)
  tick()
  sidebar.ensure(gui)
  survey = poll(gui)
  eq(#survey.spaces, 2, "gone with its last tab")
  eq(survey.space, "home", "and the view fell back to the default")

  local win2, gui2 = spaced(1, {
    hooks = {
      route = function()
        error "boom"
      end,
    },
  })
  poll(gui2)
  eq(state.space_of(win2.tab_list[1].id), "home")
  eq(logged "route hook failed", 1)
end)

test("next_space and prev_space ride on e; the action factories exist", function()
  local bound = {}
  for _, binding in ipairs(keys.build {}) do
    bound[binding.vtabs] = binding
  end
  eq(bound.next_space.key, "e")
  eq(bound.next_space.mods, platform.SUPER)
  eq(bound.prev_space.mods, platform.SUPER2)
  assert(actions.action.next_space and actions.action.prev_space)
  assert(actions.action.switch_space "claude")
  assert(actions.action.move_to_space "claude")
end)

test("Move to space opens a level listing the spaces; picking one moves the tab and closes the menu", function()
  local win, gui, _, pop = H.open_popover(2)
  setup()
  tick()
  view.sync(gui)
  eq(popover.run(gui, "space"), true)
  eq(pop.level, "spaces")
  local body = popover.wire_body(gui)
  eq(body.header.title, "Move to space")
  eq(body.items[1].id, "space:home")
  eq(body.items[1].disabled, true, "the space it is in")
  eq(body.items[2].id, "space:claude")
  eq(body.items[2].label, "C Claude")
  eq(body.items[#body.items].id, "space_auto")
  eq(body.items[#body.items].disabled, true, "nothing to hand back yet")
  eq(popover.run(gui, "space:claude"), true)
  eq(popover.get(gui:window_id()), nil, "closed")
  eq(state.space_of(win.tab_list[2].id), "claude")
  eq(state.space_manual(win.tab_list[2].id), true)

  local _, gui2, _, pop2 = H.open_popover(1)
  setup()
  popover.run(gui2, "space")
  eq(popover.back(gui2), true)
  eq(pop2.level, "root", "Esc steps back to the root level")
end)

test("more spaces than the wire allows are refused one at a time, never the whole model", function()
  local n = 0
  local win, gui = spaced(1, {
    spaces = { { id = "home" } },
    hooks = {
      route = function()
        n = n + 1
        return "s" .. n
      end,
    },
  })
  for i = 1, spaces.MAX + 3 do
    win:add_tab { title = "x" .. i }
  end
  local survey = poll(gui)
  eq(#survey.spaces, spaces.MAX)
  eq(logged "is the most a window can show", 1)
end)

util.now_ms = real_now
config.setup { backend = BACKEND }

local here = arg[0]:match "^(.*)[/\\]" or "."
package.path = here .. "/../?.lua;" .. here .. "/?.lua;" .. package.path
package.preload.wezterm = function()
  return require "wezterm_stub"
end

local passed, failed = 0, 0
local function test(name, fn)
  local ok, err = pcall(fn)
  if ok then
    passed = passed + 1
  else
    failed = failed + 1
    print("FAIL " .. name .. ": " .. tostring(err))
  end
end

local function strip(s)
  return (s:gsub("\27%[[%d;?]*[A-Za-z]", ""))
end

local function eq(a, b, msg)
  if a ~= b then
    error((msg or "") .. string.format(" expected %s got %s", tostring(b), tostring(a)), 2)
  end
end

local function usub(s, i, j)
  local from = utf8.offset(s, i)
  local to = utf8.offset(s, j + 1)
  return from and s:sub(from, (to or #s + 1) - 1) or ""
end

local function row_text(data, row)
  local seg = data:match("\27%[" .. row .. ";1H(.-)\27%[" .. (row + 1) .. ";1H")
    or data:match("\27%[" .. row .. ";1H(.*)$")
  seg = seg:gsub("\27%[%d+;%d+H.-\27%[0m$", "")
  return strip(seg)
end

local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local hit = require "vtabs.hit"
local model = require "vtabs.model"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local input = require "vtabs.input"
local geometry = require "vtabs.geometry"
local fake = require "fake_mux"
local backend = require "vtabs.backend"

backend.root = here .. "/.."
state.file = os.tmpname()

test("merge nests tables and replaces lists", function()
  local out = util.merge({ a = { b = 1, c = 2 }, l = { 1, 2 } }, { a = { c = 3 }, l = { 9 } })
  eq(out.a.b, 1)
  eq(out.a.c, 3)
  eq(#out.l, 1)
end)

test("truncate keeps width budget for wide chars", function()
  eq(util.truncate("hello world", 5, "…"), "hell…")
  eq(util.truncate("hi", 5, "…"), "hi")
  eq(util.width(util.truncate("ünïcödé text", 6, "…")), 6)
  local cjk = util.truncate("日本語のタイトル", 7, "…")
  assert(util.width(cjk) <= 7, "cjk width " .. util.width(cjk))
  eq(util.truncate("abc", 0, "…"), "")
end)

test("sanitize strips control and C1 characters", function()
  eq(util.sanitize "a\27]52;c;xx\7b\194\133c", "a]52;c;xxbc")
  eq(util.sanitize(nil), "")
end)

test("config validates enums, width and tear_off aliases", function()
  local cfg = config.setup { position = "top", width = 2, tear_off = "outside" }
  eq(cfg.position, "left")
  eq(cfg.width, 28)
  eq(cfg.tear_off, true)
  eq(config.setup({ width = 20 }).width, 20)
  assert(config.setup({}).glyphs.close)
end)

local function palette(bg, fg)
  return { background = bg, foreground = fg, ansi = {}, brights = {} }
end

test("theme ladder is monotone on dark and light schemes with readable dim", function()
  for _, p in ipairs { palette("#1e1e2e", "#cdd6f4"), palette("#fdf6e3", "#657b83"), palette("#ffffff", "#24292f") } do
    local t = theme.resolve({}, p)
    local dark = theme.contrast(t.fg, t.bg) > 1 and (t.fg[1] + t.fg[2] + t.fg[3]) > (t.bg[1] + t.bg[2] + t.bg[3])
    local function lum(c)
      return c[1] + c[2] + c[3]
    end
    if dark then
      assert(lum(t.bg) < lum(t.hover_bg) and lum(t.hover_bg) < lum(t.active_bg), "dark ladder")
    else
      assert(lum(t.bg) > lum(t.hover_bg) and lum(t.hover_bg) > lum(t.active_bg), "light ladder")
    end
    assert(theme.contrast(t.dim, t.bg) >= 3.0, "dim contrast " .. theme.contrast(t.dim, t.bg))
    assert(theme.contrast(t.active_bg, t.hover_bg) > 1.05, "active vs hover distinguishable")
  end
end)

test("theme uses scheme tab_bar only when its ladder is monotone", function()
  local p = palette("#1e1e2e", "#cdd6f4")
  p.tab_bar =
    { background = "#000000", inactive_tab_hover = { bg_color = "#333333" }, active_tab = { bg_color = "#555555" } }
  eq(theme.resolve({}, p).bg[1], 0)
  p.tab_bar.active_tab.bg_color = "#111111"
  assert(theme.resolve({}, p).bg[1] ~= 0, "non-monotone tab_bar rejected")
  eq(theme.resolve({ use_scheme_tab_bar = true }, p).bg[1], 0)
end)

local function items()
  return {
    {
      tab_id = 1,
      index = 1,
      is_active = false,
      is_pinned = true,
      title = "pinned one",
      icon = "P",
      has_unseen = false,
    },
    {
      tab_id = 2,
      index = 2,
      is_active = true,
      is_pinned = false,
      title = "active tab with a really long title",
      icon = "A",
      has_unseen = false,
    },
    { tab_id = 3, index = 3, is_active = false, is_pinned = false, title = "third", icon = "T", has_unseen = true },
  }
end

local function view(over)
  local cfg = config.setup(over and over.opts or {})
  local v = {
    cols = 28,
    rows = 10,
    items = items(),
    theme = theme.resolve({}, palette("#1e1e2e", "#cdd6f4")),
    cfg = cfg,
    glyphs = cfg.glyphs,
    scroll = 0,
  }
  for k, val in pairs(over or {}) do
    if k ~= "opts" then
      v[k] = val
    end
  end
  return v
end

test("every rendered row is exactly cols wide in many configurations", function()
  local long = items()
  long[3].title = "a very long title that has unseen output and overflows"
  local cjk = items()
  cjk[2].title = "日本語のタイトルがとても長いです"
  cjk[3].title = "한국어 🎉 emoji"
  local variants = {
    view(),
    view { hover = { x = 27, y = 4 } },
    view { hover = { x = 3, y = 5 } },
    view { items = long },
    view { items = cjk, hover = { x = 5, y = 4 } },
    view { opts = { show_index = true, close_button = "always" } },
    view { opts = { width = 8, show_index = true }, cols = 8 },
    view { opts = { width = 12 }, cols = 12, hover = { x = 12, y = 4 } },
    view { opts = { padding = { top = 0, left = 0, right = 0 } } },
    view { drag = { tab_id = 3, over_index = 1, active = true, outside = true } },
    view { footer = { "footer text that is definitely too long for the sidebar", { text = "x", id = "s" } } },
  }
  for vi, v in ipairs(variants) do
    local r = render.render(v)
    for row = 1, v.rows do
      eq(util.width(row_text(r.data, row)), v.cols, "variant " .. vi .. " row " .. row)
    end
  end
end)

test("layout: padding, pinned, separator, tabs, new tab", function()
  local r = render.render(view())
  eq(r.hits[1].kind, "space")
  eq(r.hits[2].tab_id, 1)
  eq(r.hits[3].kind, "separator")
  eq(r.hits[4].tab_id, 2)
  eq(r.hits[5].tab_id, 3)
  eq(r.hits[6].kind, "new_tab")
  eq(r.total_rows, 6)
  assert(strip(r.data):find "New Tab")
  assert(strip(r.data):find "…", "long title truncated")
  local sep = render.render(view { opts = { separator = "none" } })
  eq(sep.hits[3].tab_id, 2)
end)

test("close column is reserved so hover does not reflow the title", function()
  local plain = render.render(view())
  local hovered = render.render(view { hover = { x = 5, y = 5 } })
  local a, b = row_text(plain.data, 5), row_text(hovered.data, 5)
  eq(a:sub(1, 20), b:sub(1, 20))
  assert(hovered.hits[5].close, "close span on hover")
  eq(hovered.hits[5].close.to, 27)
  eq(usub(b, hovered.hits[5].close.from, hovered.hits[5].close.to), "x")
  assert(plain.hits[4].close, "active row shows close")
  eq(plain.hits[5].close, nil)
end)

test("unseen marker survives hover and always-close", function()
  local r = render.render(view { hover = { x = 5, y = 5 }, opts = { close_button = "always" } })
  assert(row_text(r.data, 5):find("•", 1, true), "unseen dot in marker column")
end)

test("pinned compact rows show pin glyph and no close", function()
  local r = render.render(view { hover = { x = 27, y = 2 } })
  eq(r.hits[2].close, nil)
  assert(row_text(r.data, 2):find("*", 1, true))
end)

test("drag ghost reorders, renumbers and previews pin state", function()
  local r = render.render(view { drag = { tab_id = 3, over_index = 1, active = true } })
  eq(r.hits[2].tab_id, 3)
  eq(r.hits[2].pinned, true)
  eq(r.hits[3].tab_id, 1)
  eq(r.hits[4].kind, "separator")
  local r2 = render.render(view { drag = { tab_id = 3, over_index = 2, active = true } })
  eq(r2.hits[4].tab_id, 3)
  eq(r2.hits[4].pinned, false)
  eq(r2.hits[5].tab_id, 2)
end)

test("scroll clamps, ensure_visible follows active, footer is sticky", function()
  local many = {}
  for i = 1, 30 do
    many[i] =
      { tab_id = i, index = i, is_active = i == 25, is_pinned = false, title = "t" .. i, icon = "", has_unseen = false }
  end
  local r = render.render(view { items = many, rows = 10, scroll = 999, footer = { "space: work" } })
  eq(r.scroll, 23)
  eq(r.hits[10].kind, "footer")
  r = render.render(view { items = many, rows = 10, scroll = 0, ensure_visible = 25 })
  local found = false
  for row = 1, 10 do
    if r.hits[row].tab_id == 25 then
      found = true
    end
  end
  assert(found, "active row visible after ensure_visible")
  assert(r.data:find("▐", 1, true), "scroll indicator drawn")
end)

test("hit helpers: drop slot, double click, pin block", function()
  local hits = {
    { kind = "space" },
    { kind = "tab", slot = 1 },
    { kind = "tab", slot = 2 },
    { kind = "new_tab" },
    { kind = "space" },
  }
  eq(hit.drop_slot(hits, 3, 5, 1), 2)
  eq(hit.drop_slot(hits, 4, 5, 1), 3)
  eq(hit.drop_slot(hits, 5, 5, 1), 3)
  eq(hit.drop_slot(hits, 1, 5, 1), 1)
  eq(hit.on_inner_edge(28, 28, "left"), true)
  eq(hit.on_inner_edge(27, 28, "left"), false)
  eq(hit.on_inner_edge(1, 28, "right"), true)
  local double, last = hit.double_click(nil, "space", 1000, 400)
  eq(double, false)
  double, last = hit.double_click(last, "space", 1200, 400)
  eq(double, true)
  eq(last, nil)
  eq(hit.should_pin(3, 2, true), true)
  eq(hit.should_pin(2, 1, true), true)
  eq(hit.should_pin(3, 2, false), false)
  eq(hit.should_pin(0, 0, false), false)
  eq(hit.should_pin(1, 0, false), false)
end)

test("keys: tiers, aliases, overrides, dedupe and unknown names", function()
  local all = keys.build {}
  assert(#all >= 24)
  eq(#keys.build(false), 0)
  local custom = keys.build { new_tab = false, toggle_sidebar = { key = "s", mods = "ALT" }, bogus = { key = "z" } }
  local found_toggle, found_new = false, false
  for _, k in ipairs(custom) do
    if k.key == "s" and k.mods == "ALT" then
      found_toggle = true
    end
    if k.key == "t" and k.mods == "CMD" then
      found_new = true
    end
  end
  assert(found_toggle and not found_new)
  assert(wezterm.log[#wezterm.log]:find "unknown key name")
  local cfg = { keys = { { key = "w", mods = "CMD", action = "mine" } } }
  keys.apply(cfg, config.setup {})
  local mine = 0
  for _, k in ipairs(cfg.keys) do
    if k.key == "w" and k.mods == "CMD" then
      mine = mine + 1
      eq(k.action, "mine")
    end
  end
  eq(mine, 1)
end)

test("state persists pins to GLOBAL and file, closed list round-trips", function()
  state.set_pinned(7, true)
  assert(state.is_pinned(7))
  assert(wezterm.GLOBAL.vtabs.pinned["7"])
  local f = io.open(state.file)
  local body = f:read "a"
  f:close()
  assert(body:find '"7"', "pin written to file")
  state.set_pinned(7, false)
  state.push_closed { cwd = "/tmp" }
  eq(state.pop_closed().cwd, "/tmp")
  eq(state.pop_closed(), nil)
  state.set_space(7, "work")
  eq(state.space_of(7), "work")
  state.forget_tab(7)
  eq(state.space_of(7), nil)
end)

test("model.ordered is pinned-first and stable", function()
  local ordered = model.ordered(items())
  eq(ordered[1].tab_id, 1)
  eq(ordered[2].tab_id, 2)
  eq(ordered[3].tab_id, 3)
end)

local function setup_window(n)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
  local win = fake.window()
  for i = 1, n or 2 do
    win:add_tab { title = "t" .. i }
  end
  return win, win.gui
end

local function sidebars_in(tab)
  local n = 0
  for _, p in ipairs(tab:panes()) do
    if sidebar.is_backend(p) then
      n = n + 1
    end
  end
  return n
end

local function mark_ready(tab)
  local sb = sidebar.find(tab)
  sb.vars.vtabs_token = state.token_for(sb:pane_id())
  return sb
end

test("ensure attaches one authenticated sidebar per tab and sends auth", function()
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
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

test("a sidebar with a new pane id but a known token is re-adopted, not duplicated", function()
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(1)
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

test("orphaned sidebar closes its tab without touching the active tab", function()
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
  local victim = win.tab_list[2]
  mark_ready(victim)
  table.remove(victim.pane_list, 2)
  sidebar.ensure(gui)
  eq(#win.tab_list, 1)
  eq(win.active_tab_ref, win.tab_list[1])
end)

test("collapse detaches, expand re-attaches", function()
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  sidebar.set_collapsed(gui, true)
  for _, tab in ipairs(win.tab_list) do
    eq(sidebars_in(tab), 0)
    eq(#tab:panes(), 1)
  end
  sidebar.set_collapsed(gui, false)
  for _, tab in ipairs(win.tab_list) do
    eq(sidebars_in(tab), 1)
  end
end)

test("close_tab on a background tab restores the previous active tab", function()
  local win, gui = setup_window(3)
  sidebar.ensure(gui)
  local first = win.tab_list[1]
  win.active_tab_ref = first
  actions.close_tab(gui, win.tab_list[3].id)
  eq(#win.tab_list, 2)
  eq(win.active_tab_ref, first)
end)

test("move_tab_to_slot keeps pin when dropped inside the pinned block", function()
  local win, gui = setup_window(4)
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
  local win, gui = setup_window(4)
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
  local win, gui = setup_window(3)
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

test("middle click through input closes the clicked tab", function()
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
  local first = win.tab_list[1]
  win.active_tab_ref = first
  local sb = mark_ready(first)
  require("vtabs.view").sync(gui, { force = true })
  local hits = state.session.hits[sb:pane_id()]
  eq(hits[3].tab_id, win.tab_list[2].id)
  input.handle(gui, sb, "vtabs", '{"t":"mouse","k":"down","b":"middle","x":3,"y":3}')
  eq(#win.tab_list, 1)
  eq(win.active_tab_ref, first)
end)

test("reopen_closed pushes the entry back when spawning fails", function()
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(2)
  for _, tab in ipairs(win.tab_list) do
    tab.pane_list[1].domain = "desktop"
  end
  config.setup { backend = { path = { ["local"] = "/bin/wez-vtabs", desktop = "/usr/bin/wez-vtabs" } } }
  sidebar.ensure(gui)
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

-- implementer-1: identity, persistence, backend protocol ----------------------

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
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
  local victim = win.tab_list[2]
  local sb = sidebar.find(victim)
  table.remove(victim.pane_list, 2)
  sidebar.ensure(gui)
  eq(#win.tab_list, 2, "unauthenticated sidebar keeps its tab")
  sb.vars.vtabs_token = state.token_for(sb:pane_id())
  sidebar.ensure(gui)
  eq(#win.tab_list, 1)
end)

test("collapsing leaves an unauthenticated sidebar alone", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  sidebar.set_collapsed(gui, true)
  eq(#tab:panes(), 2, "not closed without a token")
  mark_ready(tab)
  sidebar.ensure(gui)
  eq(#tab:panes(), 1)
  sidebar.set_collapsed(gui, false)
end)

test("a failed domain is retried after a minute", function()
  local win, gui = setup_window(1)
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
  local win = setup_window(1)
  sidebar.ensure(win.gui)
  local sb = sidebar.find(win.tab_list[1])
  assert(sb.split_args.set_environment_variables.VTABS_BG:match "^#%x%x%x%x%x%x$", "hex background passed")
end)

test("a marker title never reaches the rendered tab list", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  tab:set_title "wez-vtabs:abcd"
  local built = model.build(gui)
  eq(built[1].title:find "wez%-vtabs", nil)
end)

test("a content pane faking the marker cannot empty its own tab", function()
  local win, gui = setup_window(2)
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
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(1)
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

-- ===================== implementer-2: interaction / geometry / focus =====================

local function rgb(c)
  return table.concat(c, ",")
end

test("theme bg is the terminal background; elevation restores the raised tint", function()
  local t = theme.resolve({}, fake.palette)
  eq(rgb(t.bg), "30,30,46")
  local raised = theme.resolve({ elevation = 0.06 }, fake.palette)
  assert(raised.bg[1] > t.bg[1] and raised.bg[3] > t.bg[3], "elevation lifts bg toward fg")
end)

test("accent is cursor_bg when it clears 3.0 contrast, else ansi[5]", function()
  eq(rgb(theme.resolve({}, fake.palette).accent), "245,224,220")
  local low = util.merge(fake.palette, { cursor_bg = "#242438" })
  assert(theme.contrast({ 36, 36, 56 }, { 30, 30, 46 }) < 3.0, "fixture cursor is low contrast")
  eq(rgb(theme.resolve({}, low).accent), "137,180,250")
end)

local function last_action(win)
  return win.actions[#win.actions].action
end

test("AdjustPaneSize Right adds delta to split first.cols, Left subtracts (tab.rs:1294; headless 28-33-28)", function()
  local win = fake.window(80)
  local tab = win:add_tab { title = "g" }
  tab.pane_list[1]:split { direction = "Left", top_level = true, size = 28 }
  eq(tab.pane_list[1].cols, 28)
  eq(tab.pane_list[2].cols, 51)
  win.gui:perform_action(wezterm.action.AdjustPaneSize { "Right", 5 }, tab.pane_list[1])
  eq(tab.pane_list[1].cols, 33)
  eq(tab.pane_list[2].cols, 46)
  win.gui:perform_action(wezterm.action.AdjustPaneSize { "Left", 5 }, tab.pane_list[1])
  eq(tab.pane_list[1].cols, 28)
end)

test("window growth drifts the sidebar 50/50; correct claws it back in one AdjustPaneSize", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local sb = sidebar.find(win.tab_list[1])
  eq(sb.cols, 28)
  win:resize(40)
  eq(sb.cols, 48, "adjust_x_size gave the sidebar half of the delta")
  local before = #win.actions
  assert(geometry.correct(gui), "correction ran")
  eq(#win.actions - before, 1, "exactly one action")
  eq(last_action(win).action, "AdjustPaneSize")
  eq(last_action(win).arg[1], "Left")
  eq(last_action(win).arg[2], 20)
  eq(sb.cols, 28)
  eq(geometry.correct(gui), false, "second pass is a no-op")
end)

test("split Left puts the sidebar in first, split Right in second, so a right sidebar grows with Left", function()
  config.setup { position = "right", backend = { path = "/bin/wez-vtabs" } }
  local win = fake.window(80)
  win:add_tab { title = "r" }
  local gui = win.gui
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  eq(sb.split_args.direction, "Right")
  eq(sb.cols, 28)
  win:resize(-20)
  eq(sb.cols, 18)
  assert(geometry.correct(gui), "correction ran")
  eq(last_action(win).arg[1], "Left")
  eq(last_action(win).arg[2], 10)
  eq(sb.cols, 28)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a divider drag with an unchanged window becomes the desired width until config reload", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  tab:set_split(34)
  eq(geometry.correct(gui), false, "drag adopted, not fought")
  eq(geometry.desired(gui:window_id()), 34)
  eq(geometry.correct(gui), false)
  eq(sb.cols, 34)
  geometry.reset(gui:window_id())
  assert(geometry.correct(gui), "config reload drops the adopted width")
  eq(sb.cols, 28)
end)

test("correction with several content panes activates the sidebar and restores focus", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local extra = fake.pane(tab, { cols = sidebar.content_pane(tab).cols })
  tab.pane_list[#tab.pane_list + 1] = extra
  extra:activate()
  win:resize(10)
  assert(geometry.correct(gui), "correction ran")
  eq(tab.active, extra, "focus restored")
  eq(sidebar.find(tab).cols, 28)
end)

test("correction is skipped while a tab drag is in flight", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  win:resize(20)
  state.session.drag[gui:window_id()] = { tab_id = win.tab_list[1].id }
  eq(geometry.correct(gui), false)
  eq(sidebar.find(win.tab_list[1]).cols, 38)
  state.session.drag[gui:window_id()] = nil
  assert(geometry.correct(gui))
  eq(sidebar.find(win.tab_list[1]).cols, 28)
end)

local view_mod = require "vtabs.view"

local function drag_setup()
  local win, gui = setup_window(3)
  sidebar.ensure(gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  win.active_tab_ref = win.tab_list[1]
  view_mod.sync(gui, { force = true })
  return win, gui
end

local function mouse(gui, sb, kind, button, x, y)
  input.handle(gui, sb, "vtabs", string.format('{"t":"mouse","k":"%s","b":"%s","x":%d,"y":%d}', kind, button, x, y))
end

---Presses row `y` and reports the drag, with the dwell already elapsed unless `hold` says otherwise.
local function press_row(gui, sb, y, hold)
  mouse(gui, sb, "down", "left", 5, y)
  local drag = state.session.drag[gui:window_id()]
  if drag and not hold then
    drag.began = drag.began - 200
  end
  return drag
end

test("press keeps the sidebar of the clicked tab focused and points the drag at it", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local drag = press_row(gui, sb1, 4)
  eq(win.active_tab_ref, win.tab_list[3])
  eq(win.tab_list[3].active, sidebar.find(win.tab_list[3]), "sidebar holds focus, not the shell")
  eq(drag.pane_id, sidebar.find(win.tab_list[3]):pane_id())
end)

test("one row of drift never arms a drag; three rows plus the dwell reorders on release", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local ids = {}
  for i, t in ipairs(win.tab_list) do
    ids[i] = t.id
  end
  press_row(gui, sb1, 4)
  local sb3 = sidebar.find(win.tab_list[3])
  mouse(gui, sb3, "drag", "left", 5, 3)
  eq(state.session.drag[gui:window_id()].active, false, "one row is jitter")
  mouse(gui, sb3, "up", "left", 5, 3)
  eq(win.tab_list[3].id, ids[3], "order untouched")

  press_row(gui, sb1, 4)
  mouse(gui, sb3, "drag", "left", 5, 1)
  assert(state.session.drag[gui:window_id()].active, "three rows arms the drag")
  mouse(gui, sb3, "up", "left", 5, 1)
  eq(win.tab_list[1].id, ids[3], "dragged tab took the first slot")
end)

test("a drag that starts before the dwell elapses is jitter", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  press_row(gui, sb1, 4, "hold")
  local sb3 = sidebar.find(win.tab_list[3])
  mouse(gui, sb3, "drag", "left", 5, 1)
  eq(state.session.drag[gui:window_id()].active, false)
end)

test("drag events from a pane other than the drag origin are dropped", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  press_row(gui, sb1, 4)
  local sb2 = sidebar.find(win.tab_list[2])
  mouse(gui, sb2, "drag", "left", 5, 1)
  eq(state.session.drag[gui:window_id()].active, false)
end)

test("a drag whose pane has no hit map is dropped instead of dropping at slot 1", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local drag = press_row(gui, sb1, 2)
  state.session.hits[sb1:pane_id()] = nil
  mouse(gui, sb1, "drag", "left", 5, 5)
  eq(drag.active, false)
  eq(drag.over_index, nil)
end)

test("right click opens the menu on release, never while the button is held", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local before = #win.actions
  mouse(gui, sb1, "down", "right", 5, 3)
  eq(#win.actions, before, "nothing opens under a held button")
  mouse(gui, sb1, "up", "right", 5, 3)
  eq(last_action(win).action, "InputSelector")
end)

test("hover=press restores content focus on release, hover=follow keeps the sidebar", function()
  local win, gui = drag_setup()
  local tab = win.tab_list[1]
  local sb1 = sidebar.find(tab)
  press_row(gui, sb1, 2)
  eq(tab.active, sb1)
  mouse(gui, sb1, "up", "left", 5, 2)
  eq(tab.active, sb1, "follow leaves the sidebar active")
  config.setup { hover = "press", backend = { path = "/bin/wez-vtabs" } }
  press_row(gui, sb1, 2)
  mouse(gui, sb1, "up", "left", 5, 2)
  assert(tab.active ~= sb1, "press mode hands focus back")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("mouse move repaints on a row change and stays quiet inside the row", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local sent = #sb1.sent
  mouse(gui, sb1, "move", "none", 5, 3)
  local repainted = #sb1.sent
  assert(repainted > sent, "crossing into a row repaints")
  mouse(gui, sb1, "move", "none", 6, 3)
  eq(#sb1.sent, repainted, "same row, same spans, no frame")
end)

test("base64_decode round-trips and refuses malformed input", function()
  eq(util.base64_decode "bA==", "l")
  eq(util.base64_decode "G1tB", "\27[A")
  eq(util.base64_decode "", "")
  eq(util.base64_decode "bA", "l")
  eq(util.base64_decode "b*==", nil)
  eq(util.base64_decode "b", nil)
  eq(util.base64_decode(nil), nil)
end)

local function key_setup(index)
  local win, gui = drag_setup()
  win.active_tab_ref = win.tab_list[index or 1]
  local tab = win.active_tab_ref
  return win, gui, tab, sidebar.find(tab), sidebar.content_pane(tab)
end

test("a key at a sidebar outside keyboard mode is typed into the content pane, which takes focus", function()
  local _, gui, tab, sb, content = key_setup()
  sb:activate()
  local before = #content.sent
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"l","raw":"bA=="}')
  eq(#content.sent, before + 1)
  eq(content.sent[#content.sent], "l")
  eq(tab.active, content, "focus handed back to the shell")
end)

test("raw carrying an OSC or bracketed-paste introducer is dropped, focus still returns", function()
  local _, gui, tab, sb, content = key_setup(2)
  local before = #content.sent
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"x","raw":"G10wOyE="}')
  eq(#content.sent, before, "OSC introducer never reaches the shell")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_setup(3)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"x","raw":"G1syMDB+eA=="}')
  eq(#content2.sent, 0, "bracketed paste never reaches the shell")
end)

test("a burst from one sidebar pane is rate-limited to one forward", function()
  local _, gui, _, sb, content = key_setup()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"a","raw":"YQ=="}')
  eq(content.sent[#content.sent], "a")
  local n = #content.sent
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"b","raw":"Yg=="}')
  eq(#content.sent, n, "second key in the same window dropped")
end)

test("without raw only a lone printable key is forwarded", function()
  local _, gui, tab, sb, content = key_setup()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"enter"}')
  eq(#content.sent, 0, "named keys send nothing")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_setup(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"c","mods":["ctrl"]}')
  eq(#content2.sent, 0, "ctrl chords send nothing")
  local _, gui3, _, sb3, content3 = key_setup(3)
  input.handle(gui3, sb3, "vtabs", '{"t":"key","key":"z"}')
  eq(content3.sent[1], "z")
end)

-- Verbatim backend output; the strings match backend/src/event.rs `key_events`.
test("safe_key_bytes takes one key press per shape and refuses a command line", function()
  for _, ok in ipairs { "x", "\u{e6}", "\r", "\n", "\t", "\8", "\3", "\127", "\0", "\27" } do
    eq(input.safe_key_bytes(ok), ok, "accepts " .. #ok .. " byte(s)")
  end
  for _, seq in ipairs { "\27[A", "\27[1;5D", "\27[3~", "\27OH", "\27b" } do
    eq(input.safe_key_bytes(seq), seq, "accepts an ESC-prefixed key")
  end
  for _, bad in ipairs {
    "id > /tmp/pwn\r",
    "ab",
    "\27[Ax",
    "\27]0;x\7",
    "\27[200~",
    "\27[201~",
    "\27P0q",
    string.rep("x", 17),
    "",
  } do
    eq(input.safe_key_bytes(bad), nil, "rejects " .. string.format("%q", bad))
  end
  eq(input.safe_key_bytes(nil), nil)
  eq(input.safe_key_bytes "\xff\xfe", nil, "invalid utf-8 is not one codepoint")
end)

test("a raw payload carrying a whole command line never reaches the content pane", function()
  local _, gui, tab, sb, content = key_setup()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"x","raw":"aWQgPiAvdG1wL3B3bg0="}')
  eq(#content.sent, 0, "the probe payload is dropped")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_setup(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"up","raw":"G1tB"}')
  eq(content2.sent[#content2.sent], "\27[A", "a real arrow key still gets through")
end)

test("backend-shaped key events reach the content pane as the exact bytes they carried", function()
  local _, gui, tab, sb, content = key_setup()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"enter","raw":"DQ=="}')
  eq(content.sent[#content.sent], "\r")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_setup(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"c","mods":["ctrl"],"raw":"Aw=="}')
  eq(content2.sent[#content2.sent], "\3", "ctrl chords are forwarded verbatim when raw is present")
  local _, gui3, tab3, sb3, content3 = key_setup(3)
  input.handle(gui3, sb3, "vtabs", '{"t":"key","key":"escape"}')
  eq(#content3.sent, 0, "a key the backend could not capture sends nothing")
  eq(tab3.active, content3, "focus still returns to the shell")
end)

test("a key from a background tab's sidebar is never forwarded", function()
  local win, gui = drag_setup()
  win.active_tab_ref = win.tab_list[1]
  local other = win.tab_list[2]
  local content = sidebar.content_pane(other)
  input.handle(gui, sidebar.find(other), "vtabs", '{"t":"key","key":"l","raw":"bA=="}')
  eq(#content.sent, 0)
end)

test("a key from a sidebar in another domain than its content pane is dropped", function()
  local _, gui, _, sb, content = key_setup()
  sb.domain = "desktop"
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"l","raw":"bA=="}')
  eq(#content.sent, 0)
end)

---Counts tab switches by wrapping the shared Tab metatable for the duration of `fn`.
local function count_switches(win, fn)
  local Tab = getmetatable(win.tab_list[1])
  local original = Tab.activate
  local switches = 0
  Tab.activate = function(self)
    switches = switches + 1
    return original(self)
  end
  local ok, err = pcall(fn)
  Tab.activate = original
  if not ok then
    error(err, 0)
  end
  return switches
end

test("reorder restores the active tab once for the whole batch", function()
  local win, gui = setup_window(4)
  sidebar.ensure(gui)
  local ids = {}
  for i, t in ipairs(win.tab_list) do
    ids[i] = t.id
  end
  win.active_tab_ref = win.tab_list[1]
  local before = #win.actions
  local switches = count_switches(win, function()
    actions.reorder(gui, { ids[4], ids[3], ids[2], ids[1] })
  end)
  local moves = #win.actions - before
  assert(moves >= 2, "several tabs moved, got " .. moves)
  eq(switches, moves + 1, "one restore, not one per move")
  eq(win.active_tab_ref.id, ids[1])
end)

test("close_others restores the kept tab once, not after every close", function()
  local win, gui = setup_window(4)
  sidebar.ensure(gui)
  win.active_tab_ref = win.tab_list[1]
  local kept = win.tab_list[1].id
  local switches = count_switches(win, function()
    actions.close_others(gui, kept)
  end)
  eq(#win.tab_list, 1)
  eq(win.active_tab_ref.id, kept)
  eq(switches, 4, "three closes plus one restore")
end)

os.remove(state.file)
print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

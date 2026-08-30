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

test("use_scheme_tab_bar is a deprecated no-op: the sidebar keeps the terminal background", function()
  local p = palette("#1e1e2e", "#cdd6f4")
  p.tab_bar =
    { background = "#000000", inactive_tab_hover = { bg_color = "#333333" }, active_tab = { bg_color = "#555555" } }
  eq(theme.resolve({}, p).bg[1], 30, "no background is borrowed")
  eq(theme.resolve({ use_scheme_tab_bar = true }, p).bg[1], 30)
  local warned = 0
  for _, line in ipairs(wezterm.log) do
    if line:find("use_scheme_tab_bar is deprecated", 1, true) then
      warned = warned + 1
    end
  end
  eq(warned, 1, "warned once")
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

-- These assert renderer mechanics at a fixed layout, so they state it rather than tracking defaults.
local LEGACY_LAYOUT = {
  row_gap = 0,
  padding = { top = 1, left = 1, right = 1 },
  separator = "rule",
  pinned_style = "compact",
  new_tab_button = "row",
  meta = false,
  toggle_button = false,
}

local function legacy(opts)
  return util.merge(LEGACY_LAYOUT, opts or {})
end

local function view(over)
  local cfg = config.setup(legacy(over and over.opts))
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
  assert(strip(r.data):find "New tab")
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
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
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

test("the marker pattern matches the backend title and nothing else", function()
  for _, title in ipairs { "wez-vtabs:c7a00ec8", "wez-vtabs:00", "wez-vtabs:abcd" } do
    eq(sidebar.marker(title), true, title)
  end
  for _, title in ipairs { "wez-vtabs", "wez-vtabs --version", "wez-vtabs:deploy prod", "my wez-vtabs:00" } do
    eq(sidebar.marker(title), false, title)
  end
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
  local win, gui = setup_window(1)
  local tab, liar = marker_tab(win)
  liar.domain = "desktop"
  sidebar.ensure(gui)
  eq(#liar.sent, 0, "never auth'd")
  sidebar.ensure(gui)
  eq(sidebar.is_backend(liar), false, "treated as content")
  eq(sidebars_in(tab), 1, "the tab still gets a real sidebar")
end)

test("adopt=auto skips domains this process never spawned a backend in", function()
  local win, gui = setup_window(1)
  config.setup { backend = { path = { ["local"] = "/bin/wez-vtabs" } } }
  local tab, liar = marker_tab(win, "desktop")
  sidebar.ensure(gui)
  eq(#liar.sent, 0, "never auth'd")
  sidebar.ensure(gui)
  eq(sidebar.is_backend(liar), false, "treated as content")
  eq(sidebars_in(tab), 1)
end)

test("adopt=true takes over the same pane", function()
  local win, gui = setup_window(1)
  config.setup { adopt = true, backend = { path = { ["local"] = "/bin/wez-vtabs" } } }
  local _, liar = marker_tab(win, "desktop")
  sidebar.ensure(gui)
  assert(liar.sent[1] and liar.sent[1]:find '"auth"', "auth sent")
  eq(sidebar.is_ready(liar), false, "still needs the echo")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("adopt=false never auths a marker pane", function()
  local win, gui = setup_window(1)
  config.setup { adopt = false, backend = { path = "/bin/wez-vtabs" } }
  local tab, liar = marker_tab(win)
  sidebar.ensure(gui)
  eq(#liar.sent, 0, "never auth'd")
  sidebar.ensure(gui)
  eq(sidebars_in(tab), 1, "falls back to a fresh sidebar")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("an adopted pane is abandoned once its window closes", function()
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(1)
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
  local win, gui = setup_window(2)
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
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
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

test("the accent chain is cursor_bg, then tab_bar active, then ansi[5], each behind both gates", function()
  local base = fake.palette
  -- Mocha's rosewater cursor clears 3.0 against the page but is 1.06 from the foreground.
  assert(theme.contrast({ 245, 224, 220 }, { 205, 214, 244 }) < 1.2, "fixture cursor is fg-coloured")
  eq(rgb(theme.resolve({}, base).accent), "137,180,250", "falls through to ansi[5]")
  local usable = util.merge(base, { cursor_bg = "#f38ba8" })
  eq(rgb(theme.resolve({}, usable).accent), "243,139,168", "a cursor that clears both gates wins")
  local no_cursor = util.merge(base, { cursor_bg = "#242438" })
  no_cursor.tab_bar = { active_tab = { bg_color = "#74c7ec" } }
  eq(rgb(theme.resolve({}, no_cursor).accent), "116,199,236", "then the scheme's active tab colour")
  local flat = util.merge(base, { cursor_bg = "#242438" })
  flat.tab_bar = { active_tab = { bg_color = "#94e2d5" } }
  eq(rgb(theme.resolve({}, flat).accent), "137,180,250", "a tab colour too close to fg is skipped too")
  eq(rgb(theme.resolve({ accent = "#ff0000" }, base).accent), "255,0,0", "a user accent still wins")
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
  local sb = mark_ready(win.tab_list[1])
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
  local sb = mark_ready(tab)
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
  local sb = mark_ready(tab)
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
  mark_ready(tab)
  local extra = fake.pane(tab, { cols = sidebar.content_pane(tab).cols })
  tab.pane_list[#tab.pane_list + 1] = extra
  extra:activate()
  win:resize(10)
  assert(geometry.correct(gui), "correction ran")
  eq(tab.active, extra, "focus restored")
  eq(sidebar.find(tab).cols, 28)
end)

test("a sidebar that only carries the title marker is never resized", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  win:resize(20)
  local before = #win.actions
  eq(geometry.correct(gui), false, "an unauthenticated pane is not corrected")
  eq(#win.actions, before, "no AdjustPaneSize")
  eq(sb.cols, 38)
  mark_ready(tab)
  assert(geometry.correct(gui), "the same pane is corrected once it echoes its token")
  eq(sb.cols, 28)
end)

test("a zoomed pane suspends adoption and correction until it is unzoomed", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  sb.zoomed = true
  sb.cols = tab:width()
  local before = #win.actions
  eq(geometry.correct(gui), false, "zoom is not a divider drag")
  eq(#win.actions, before, "no AdjustPaneSize while zoomed")
  eq(geometry.desired(gui:window_id()), 28, "full-window zoom width not latched")
  sb.zoomed = false
  sb.cols = 28
  eq(geometry.correct(gui), false, "unzoom restores the width it had")
  eq(geometry.desired(gui:window_id()), 28)
end)

test("an adopted width is clamped to a plausible sidebar", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  tab:set_split(78)
  eq(geometry.correct(gui), false, "drag adopted")
  eq(geometry.desired(gui:window_id()), 60, "clamped to tab width minus the content margin")
  geometry.reset(gui:window_id())
  assert(geometry.correct(gui), "a reset re-asserts cfg.width")
  geometry.settle(gui:window_id(), 0)
  eq(geometry.correct(gui), false, "baseline recorded")
  tab:set_split(1)
  eq(geometry.correct(gui), false, "drag adopted")
  eq(geometry.desired(gui:window_id()), 8, "clamped to the minimum width")
end)

test("an unreachable width is attempted until it stops moving, then left alone", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  win:resize(-71)
  eq(tab:width(), 9, "a window too narrow to hold the sidebar")
  assert(geometry.correct(gui), "first attempt moves it as far as the split allows")
  eq(sb.cols, 7, "clamped to width - 2")
  local issued = 0
  for _ = 1, 10 do
    if geometry.correct(gui) then
      issued = issued + 1
    end
  end
  assert(issued <= 4, "the retry is bounded; a mux gets a few polls to catch up, got " .. issued)
  local settled = #win.actions
  eq(geometry.correct(gui), false)
  eq(geometry.correct(gui), false)
  eq(#win.actions, settled, "no AdjustPaneSize and no activate once it is known unreachable")
  win:resize(40)
  assert(geometry.correct(gui), "a window resize unblocks the retry")
end)

test("a font or dpi change is corrected, not adopted as a divider drag", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  for _, p in ipairs(tab:panes()) do
    p.cell_width = 14
  end
  tab:set_split(20)
  assert(geometry.correct(gui), "a wider cell is not the user dragging the divider")
  eq(geometry.desired(gui:window_id()), 28, "cfg.width still wins")
  eq(sb.cols, 28)
end)

test("geometry.sync corrects on a tab change and rate-gates otherwise", function()
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  local first, second = win.tab_list[1], win.tab_list[2]
  win.active_tab_ref = first
  assert(geometry.sync(gui, first.id) == false, "nothing to correct")
  win:resize(20)
  eq(geometry.sync(gui, first.id), false, "same tab inside the observe window is skipped")
  eq(sidebar.find(first).cols, 38)
  win.active_tab_ref = second
  assert(geometry.sync(gui, second.id), "a tab change corrects at once")
  eq(sidebar.find(second).cols, 28)
end)

test("correction is skipped while a tab drag is in flight", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
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

test("fast typing is forwarded whole; a flood is cut off at the burst budget", function()
  local _, gui, tab, sb, content = key_setup()
  for _ = 1, 25 do
    input.handle(gui, sb, "vtabs", '{"t":"key","key":"a","raw":"YQ=="}')
  end
  eq(#content.sent, 20, "20 keys of burst, no refill inside one stub tick")
  eq(tab.active, content, "focus stays handed over across the dropped tail")
end)

test("a paste event is delivered whole to the content pane", function()
  local _, gui, tab, sb, content = key_setup()
  input.handle(gui, sb, "vtabs", '{"t":"paste","data":"aGVsbG8gd29ybGQ="}')
  eq(content.pasted[#content.pasted], "hello world")
  eq(#content.sent, 0, "a paste is not typed key by key")
  eq(tab.active, content)
end)

test("a paste is refused when it is oversized, malformed or from a background tab", function()
  local _, gui, _, sb, content = key_setup()
  input.handle(gui, sb, "vtabs", '{"t":"paste","data":"!!!!"}')
  eq(#content.pasted, 0, "malformed base64 dropped")
  local _, gui2, tab2, sb2, content2 = key_setup(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"paste","dropped":"size"}')
  eq(#content2.pasted, 0, "the backend's oversize form carries nothing to paste")
  eq(tab2.active, content2, "focus still returns to the shell")
  local win3, gui3 = drag_setup()
  win3.active_tab_ref = win3.tab_list[1]
  local other = win3.tab_list[2]
  input.handle(gui3, sidebar.find(other), "vtabs", '{"t":"paste","data":"aGk="}')
  eq(#sidebar.content_pane(other).pasted, 0, "background tab dropped")
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

-- Sequences backend/src/parser.rs names "unknown": F5, SS3 Z, and a CSI it does not decode.
test("a key the backend could not name is still forwarded by its raw bytes", function()
  local _, gui, tab, sb, content = key_setup()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"unknown","raw":"G1sxNX4="}')
  eq(content.sent[#content.sent], "\27[15~", "F5 reaches the shell")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_setup(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"unknown","raw":"G09a"}')
  eq(content2.sent[#content2.sent], "\27OZ")
  local _, gui3, _, sb3, content3 = key_setup(3)
  input.handle(gui3, sb3, "vtabs", '{"t":"key","key":"unknown"}')
  eq(#content3.sent, 0, "without raw there is nothing to forward")
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

test("is_sidebar_pane answers for any backend pane and changes nothing while it answers", function()
  local vtabs = require "vtabs.sidebar"
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  local pid = sb:pane_id()
  sb.vars.vtabs_token = state.token_for(pid)
  state.session.ready[pid] = nil
  local mapped = state.sidebar_pane_id(tab:tab_id())
  assert(vtabs.is_backend(sb), "a backend pane is skippable before anyone authenticates it")
  eq(state.session.ready[pid], nil, "answering never promotes the pane to ready")
  eq(state.sidebar_pane_id(tab:tab_id()), mapped, "no map mutation")
  assert(vtabs.is_ready(sb), "the trusted predicate is the one that promotes")
  eq(state.session.ready[pid], true)
  eq(vtabs.is_backend(sidebar.content_pane(tab)), false, "a content pane is not a backend")
  eq(vtabs.is_backend(nil), false)
end)

test("the window title names the content pane while the sidebar holds focus", function()
  local view_only = require "vtabs.view"
  local sb = { pane_id = 7, title = "wez-vtabs:deadbeef" }
  local shell = { pane_id = 8, title = "nvim" }
  local tab = { tab_id = 3, tab_index = 1, tab_title = "" }
  eq(view_only.window_title(tab, sb, { tab }, { sb, shell }), "nvim")
  eq(view_only.window_title(tab, sb, { tab, tab }, { sb, shell }), "[2/2] nvim")
  eq(view_only.window_title(tab, shell, { tab }, { sb, shell }), nil, "wezterm's default is left alone")
  eq(view_only.window_title(tab, sb, { tab }, { sb }), nil, "no content pane, no opinion")
  eq(view_only.window_title(nil, nil, nil, nil), nil)
  eq(config.setup({}).window_title, true, "registered by default")
  eq(config.setup({ window_title = false }).window_title, false, "opt out leaves the event alone")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("an unreachable contrast gate stops at the target colour instead of mixing past it", function()
  for _, p in ipairs { palette("#2b2b2b", "#4a4a4a"), palette("#ffffff", "#cccccc") } do
    local t = theme.resolve({}, p)
    assert(theme.contrast(t.fg, t.active_bg) < 3.5, "the fixture ceiling is below the meta gate")
    eq(rgb(t.meta_fg), rgb(t.fg), "meta_fg lands on fg, never past it")
    for i = 1, 3 do
      assert(t.meta_fg[i] >= 0 and t.meta_fg[i] <= 255, "channel in range")
    end
  end
  local reachable = theme.resolve({}, palette("#1e1e2e", "#cdd6f4"))
  assert(theme.contrast(reachable.meta_fg, reachable.active_bg) >= 3.5, "a reachable gate is met")
  assert(rgb(reachable.meta_fg) ~= rgb(reachable.fg), "and meta_fg is still quieter than fg")
end)

-- bug-hunter regression pins ------------------------------------------------

test("a mux window resize is not a divider drag, even though the pane size arrives a poll late", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  eq(sb.cols, 28)
  -- A mux client learns the new window size before the server sends back the new pane sizes.
  win.cols = win.cols + 40
  geometry.correct(gui)
  for _, tab in ipairs(win.tab_list) do
    tab:adjust_x_size(40)
  end
  eq(sb.cols, 48, "adjust_x_size gave the sidebar half of the delta")
  eq(geometry.desired(gui:window_id()), 28, "the drift was adopted as the user's width")
  geometry.correct(gui)
  eq(sb.cols, 28)
end)

test("the two-step mux resize is corrected, and a real divider drag is still adopted", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline")
  win:resize_mux(30)
  eq(geometry.correct(gui), false, "the window grew but the panes have not heard yet")
  win:settle_mux()
  eq(sb.cols, 43, "wezterm dealt the sidebar half the delta")
  assert(geometry.correct(gui), "the late pane size is a resize, not a drag")
  eq(sb.cols, 28)
  eq(geometry.desired(gui:window_id()), 28, "nothing was adopted")

  geometry.reset(gui:window_id())
  geometry.settle(gui:window_id(), 0)
  eq(geometry.correct(gui), false, "baseline on the settled window")
  tab:set_split(34)
  eq(geometry.correct(gui), false, "same tab width, same pixels: this one is a drag")
  eq(geometry.desired(gui:window_id()), 34)
end)

test("a paste is charged by its size, so a second large one waits for the budget", function()
  local _, gui, tab, sb, content = key_setup()
  local big = string.rep("eHh4", 21845) .. "eA=="
  input.handle(gui, sb, "vtabs", string.format('{"t":"paste","data":"%s"}', big))
  eq(#content.pasted, 1, "the first 64 KiB paste goes through")
  eq(#content.pasted[1], 64 * 1024)
  input.handle(gui, sb, "vtabs", string.format('{"t":"paste","data":"%s"}', big))
  eq(#content.pasted, 1, "the second is over budget")
  eq(tab.active, content, "focus is still handed over")
end)

test("the sidebar's own resize event corrects at once, inside the observe gate", function()
  local win, gui = drag_setup()
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  geometry.sync(gui, tab.id)
  win:resize_mux(30)
  win:settle_mux()
  eq(sb.cols, 43, "wezterm dealt the sidebar half the delta")
  eq(geometry.sync(gui, tab.id), false, "the poll gate is still closed")
  eq(sb.cols, 43)
  input.handle(gui, sb, "vtabs", '{"t":"resize","cols":43,"rows":30}')
  eq(sb.cols, 28, "the backend reporting its own size is never gated")
end)

local palettes = require "palettes"

-- P1-spec §6.1: the gates every scheme must clear, and the ceiling clamp where it cannot.
local CEILING_LIMITED = { ["Solarized Dark"] = true, ["Solarized Light"] = true }

test("every §6.1 gate holds on all ten palettes, or is declared ceiling-limited", function()
  local limited = {}
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local c, where = theme.contrast, " on " .. p.name
    local ceiling = c(t.fg, t.active_bg)
    assert(c(t.meta_fg, t.active_bg) >= math.min(3.5, ceiling) - 0.001, "meta_fg vs active_bg" .. where)
    assert(c(t.close_fg, t.active_bg) >= 3.0 - 0.001, "close_fg vs active_bg" .. where)
    assert(c(t.close_hover_fg, t.active_bg) >= 3.0 - 0.001, "close_hover_fg vs active_bg" .. where)
    assert(c(t.border, t.bg) >= 2.5 - 0.001, "border vs bg" .. where)
    assert(c(t.border_idle, t.bg) >= 2.0 - 0.001, "border_idle vs bg" .. where)
    assert(c(t.scroll_fg, t.bg) >= 2.0 - 0.001, "scroll_fg vs bg" .. where)
    assert(c(t.accent, t.bg) >= 3.0 - 0.001, "accent vs bg" .. where)
    if ceiling < 3.5 then
      limited[p.name] = true
      eq(rgb(t.meta_fg), rgb(t.fg), "ceiling-limited meta_fg is fg exactly" .. where)
    end
  end
  eq(rgb(util.sorted_keys(limited)), rgb(util.sorted_keys(CEILING_LIMITED)), "exactly the declared two")
end)

test("title_idle is quieted only when the scheme has 5.0 of contrast to spend", function()
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local quiet = theme.contrast(t.fg, t.bg) >= 5.0
    if quiet then
      assert(rgb(t.title_idle) ~= rgb(t.fg), "quieted on " .. p.name)
    else
      eq(rgb(t.title_idle), rgb(t.fg), "left alone on " .. p.name)
    end
  end
  local dark = theme.resolve({}, palettes[1])
  local flat = theme.resolve({}, palette("#002b36", "#839496"))
  assert(theme.contrast(flat.fg, flat.bg) < 5.0)
  eq(rgb(flat.title_idle), rgb(flat.fg))
  assert(rgb(dark.title_idle) ~= rgb(dark.fg))
end)

local function hex(h)
  local r, g, b = h:match "^#(%x%x)(%x%x)(%x%x)$"
  return table.concat({ tonumber(r, 16), tonumber(g, 16), tonumber(b, 16) }, ",")
end

test("close_hover_fg keeps the scheme's red where the red already clears its gate", function()
  local untouched = 0
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local red = theme.resolve({ close_fg = p.ansi[2] }, p).close_fg
    assert(theme.contrast(t.close_hover_fg, t.active_bg) >= 3.0 - 0.001, "gate met on " .. p.name)
    if theme.contrast(red, t.active_bg) >= 3.0 then
      eq(rgb(t.close_hover_fg), hex(p.ansi[2]), "a red that already clears the gate is not desaturated")
      untouched = untouched + 1
    end
  end
  assert(untouched >= 3, "the 3.0 gate leaves most schemes' red alone, got " .. untouched)
end)

test("unseen_fg keeps a distinct hue when ansi[4] clears the page, else follows the accent", function()
  local base = palettes[1]
  local t = theme.resolve({}, base)
  eq(rgb(t.unseen_fg), rgb(theme.resolve({ unseen_fg = base.ansi[4] }, base).unseen_fg))
  local dull = util.merge(base, {})
  dull.ansi = { base.ansi[1], base.ansi[2], base.ansi[3], "#20202c", base.ansi[5] }
  local low = theme.resolve({}, dull)
  eq(rgb(low.unseen_fg), rgb(low.accent), "a dim ansi[4] falls back to the accent")
end)

test("a private window derives every accent-tinted surface from private_accent", function()
  local base = palettes[1]
  local normal = theme.resolve({}, base)
  local private = theme.resolve({}, base, { private = true })
  eq(rgb(private.accent), rgb(private.private_accent))
  assert(rgb(private.accent) ~= rgb(normal.accent), "hue actually moves")
  for _, key in ipairs { "active_bg", "focus_bg", "drag_bg" } do
    assert(rgb(private[key]) ~= rgb(normal[key]), key .. " follows the private accent")
  end
end)

test("ensure_contrast returns fg untouched, and never past target when the gate is unreachable", function()
  local p = palettes[1]
  local t = theme.resolve({}, p)
  assert(theme.contrast(t.border, t.bg) >= 2.5, "a reachable gate stops as soon as it is met")
  local flat = theme.resolve({}, palette("#2b2b2b", "#4a4a4a"))
  eq(rgb(flat.meta_fg), rgb(flat.fg), "unreachable gate stops at the target")
  eq(rgb(theme.resolve({ meta_fg = "#123456" }, p).meta_fg), "18,52,86", "a user value is taken verbatim")
end)

test("theme exports mix and luminance for the renderer", function()
  eq(type(theme.mix), "function")
  eq(type(theme.luminance), "function")
  eq(rgb(theme.mix({ 0, 0, 0 }, { 100, 200, 250 }, 0.5)), "50,100,125")
  assert(theme.luminance { 255, 255, 255 } > theme.luminance { 0, 0, 0 })
end)

test("every §6.3 key is present and overridable", function()
  local groups = {
    "bg fg dim accent title_idle meta_fg active_bg active_fg hover_bg hover_fg focus_bg",
    "pinned_fg separator border border_idle new_tab_fg close_fg close_hover_fg unseen_fg",
    "private_accent drag_bg drag_fg scroll_fg scroll_idle_fg",
  }
  -- accent and close_hover_fg are the two keys §6.1 gates after resolving the user's value.
  local GATED = { accent = true, close_hover_fg = true }
  local t = theme.resolve({}, palettes[1])
  for _, group in ipairs(groups) do
    for key in group:gmatch "%S+" do
      assert(t[key], "missing key " .. key)
      if not GATED[key] then
        eq(rgb(theme.resolve({ [key] = "#010203" }, palettes[1])[key]), "1,2,3", "override ignored for " .. key)
      end
    end
  end
  eq(rgb(theme.resolve({ accent = "#89b4fa" }, palettes[1]).accent), "137,180,250", "a passing accent is kept")
  local lifted = theme.resolve({ accent = "#010203" }, palettes[1]).accent
  assert(rgb(lifted) ~= "1,2,3" and theme.contrast(lifted, t.bg) >= 3.0, "an unreadable accent is lifted")
end)

test("shorten_path elides middle components and keeps the basename", function()
  local sp = util.shorten_path
  eq(sp("~/projects/wez-plugins/vertical-tabs", 20), "~/p/w/vertical-tabs")
  eq(sp("~/projects/wezterm-vertical-tabs/plugin/vtabs", 20), "~/p/w/plugin/vtabs")
  eq(sp("/usr/local/share/doc/wezterm/examples", 20), "/u/l/s/d/w/examples")
  eq(sp("~/Documents/notes", 20), "~/Documents/notes", "a path that fits is untouched")
  assert(sp("~/projects/api", 12) ~= sp("~/projects/web", 12), "siblings stay distinguishable")
  eq(sp("~/projects/api", 12), "~/p/api")
  eq(sp("~/a/very-long-basename-that-alone-overflows", 20), "…/very-long-basenam…")
  eq(util.width(sp("~/a/very-long-basename-that-alone-overflows", 20)), 20)
  eq(sp("~", 20), "~")
  eq(sp("", 20), "")
  eq(sp(nil, 20), "")
  eq(sp("~/x", 0), "")
  for _, budget in ipairs { 4, 8, 12, 20, 40 } do
    local out = sp("~/projects/wezterm-vertical-tabs/plugin/vtabs", budget)
    assert(util.width(out) <= budget, "budget " .. budget .. " overflowed with " .. out)
  end
end)

test("the P1 defaults and their aliases pass validation without warning", function()
  local before = #wezterm.log
  local cfg = config.setup {}
  eq(cfg.padding.top, 0)
  eq(cfg.row_gap, 1)
  eq(cfg.tab_height, "card")
  eq(cfg.meta, "auto")
  eq(cfg.separator, "gap")
  eq(cfg.pinned_style, "dense")
  eq(cfg.new_tab_button, "ghost")
  eq(cfg.corners, "chamfer")
  eq(cfg.scroll_indicator, "auto")
  eq(cfg.titlebar, "auto")
  eq(cfg.toggle_button, true)
  eq(#wezterm.log, before, "no warnings on the defaults")
  eq(config.setup({ new_tab_button = true }).new_tab_button, "ghost")
  eq(config.setup({ scroll_indicator = true }).scroll_indicator, "auto")
  eq(config.setup({ scroll_indicator = false }).scroll_indicator, "never")
  eq(config.setup({ meta = true }).meta, "auto")
  eq(config.setup({ new_tab_button = false }).new_tab_button, false)
  eq(config.setup({ meta = false }).meta, false)
  eq(#wezterm.log, before, "aliases do not warn either")
end)

test("each new key rejects a bad value and keeps its default", function()
  for key, bad in pairs {
    tab_height = "tall",
    meta = "path",
    new_tab_button = "button",
    corners = "round",
    scroll_indicator = "sometimes",
    titlebar = "native",
    pinned_style = "tiny",
  } do
    eq(config.setup({ [key] = bad })[key], config.defaults[key], key .. " reset")
  end
  eq(config.setup({ row_gap = -1 }).row_gap, 1)
  eq(config.setup({ row_gap = "two" }).row_gap, 1)
  eq(config.setup({ toggle_button = "yes" }).toggle_button, true)
  eq(config.setup({ row_gap = 3 }).row_gap, 3, "a valid value survives")
end)

test("tab_height and meta stay consistent, and press mode forces an always-on close button", function()
  eq(config.setup({ tab_height = "row" }).meta, false)
  eq(config.setup({ meta = false }).tab_height, "row")
  eq(config.setup({ tab_height = "card" }).meta, "auto")
  eq(config.setup({ hover = "press" }).close_button, "always")
  eq(config.setup({ hover = "press", close_button = "never" }).close_button, "never")
  eq(config.setup({ hover = "follow" }).close_button, "hover")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

local platform = require "vtabs.platform"

-- Retina-ish: 8.4 px cells across, 19 px down, so 70/8.4 -> 9 cols and 28/19 -> 2 rows.
local RETINA = { cols = 28, viewport_rows = 30, pixel_width = 235, pixel_height = 570 }

local function strip_geom(dims, over)
  local opts = {
    is_mac = true,
    integrated_buttons = true,
    native_button_style = true,
    is_full_screen = false,
    position = "left",
    padding_top = 1,
    toggle_button = true,
    card_x1 = 2,
  }
  for k, v in pairs(over or {}) do
    opts[k] = v
  end
  return platform.strip_geometry(dims, opts)
end

test("the macOS strip reserves the traffic lights from the pane's own cell size", function()
  local g = strip_geom(RETINA)
  eq(g.cols, math.ceil(70 / (235 / 28)), "70 px of buttons, never a hardcoded column count")
  eq(g.cols, 9)
  eq(g.rows, 3, "max(reserve 2, toggle 2) + padding_top 1")
  eq(g.toggle_row, 2)
  eq(g.toggle_x, 11, "clear of the reserve")
  local wide = strip_geom { cols = 28, viewport_rows = 30, pixel_width = 560, pixel_height = 570 }
  eq(wide.cols, 4, "bigger cells need fewer of them")
end)

test("every branch that is not a windowed left-hand macOS sidebar reserves no columns", function()
  eq(strip_geom(RETINA, { is_full_screen = true }).cols, 0)
  eq(strip_geom(RETINA, { position = "right" }).cols, 0)
  eq(strip_geom(RETINA, { native_button_style = false }).cols, 0)
  eq(strip_geom(RETINA, { integrated_buttons = false }).cols, 0)
  eq(strip_geom(RETINA, { is_mac = false }).cols, 0, "linux and windows")
  eq(strip_geom({}, {}).cols, 0, "no dimensions, no guess")
  eq(strip_geom({ cols = 0, viewport_rows = 0, pixel_width = 0, pixel_height = 0 }).cols, 0)
end)

test("the strip is toggle plus padding when nothing is reserved", function()
  local linux = { is_mac = false }
  eq(strip_geom(RETINA, linux).rows, 2, "toggle row plus padding_top")
  eq(strip_geom(RETINA, { is_mac = false, toggle_button = false }).rows, 1)
  eq(strip_geom(RETINA, { is_mac = false, padding_top = 0 }).rows, 1, "the P1 default padding")
  eq(strip_geom(RETINA, { is_mac = false, toggle_button = false, padding_top = 0 }).rows, 0)
  eq(strip_geom(RETINA, linux).toggle_row, 1)
  eq(strip_geom(RETINA, linux).toggle_x, 2, "card_x1")
  eq(strip_geom(RETINA, { is_mac = false, card_x1 = 4 }).toggle_x, 4)
end)

test("the toggle span never reaches past the strip into the first card row", function()
  for _, over in ipairs {
    {},
    { is_mac = false },
    { is_full_screen = true },
    { position = "right" },
    { padding_top = 0 },
    { is_mac = false, padding_top = 0 },
    { padding_top = 3 },
  } do
    local g = strip_geom(RETINA, over)
    local span_last = math.min(g.toggle_row + 1, g.rows)
    assert(span_last <= g.rows, "toggle span inside the strip")
    assert(g.toggle_row <= g.rows, "toggle row inside the strip")
  end
end)

test("macOS window decorations are set only for a left sidebar the user has not configured", function()
  local vtabs = dofile(here .. "/../init.lua")
  assert(platform.is_mac, "the stub target triple is darwin")
  local function decorations(opts, preset)
    local cfg = { keys = {} }
    cfg.window_decorations = preset
    vtabs.apply_to_config(cfg, opts)
    return cfg.window_decorations
  end
  eq(decorations {}, "INTEGRATED_BUTTONS|RESIZE")
  eq(decorations { position = "right" }, nil, "a right sidebar reserves nothing, so it opts out")
  eq(decorations { titlebar = "plain" }, nil)
  eq(decorations({}, "TITLE|RESIZE"), "TITLE|RESIZE", "a user value is never overwritten")
  local before = #wezterm.log
  decorations({}, "RESIZE")
  assert(#wezterm.log > before, "RESIZE alone hides the buttons, so it warns")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

os.remove(state.file)
print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

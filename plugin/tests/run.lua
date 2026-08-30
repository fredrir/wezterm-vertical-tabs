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
    if sidebar.is_sidebar(p) then
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
  eq(sidebar.is_sidebar(sb), true)
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
  eq(sidebar.is_sidebar(content), false)
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
  table.remove(victim.pane_list, 2)
  sidebar.ensure(gui)
  eq(#win.tab_list, 1)
  eq(win.active_tab_ref, win.tab_list[1])
end)

test("collapse detaches, expand re-attaches", function()
  local win, gui = setup_window(2)
  sidebar.ensure(gui)
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

os.remove(state.file)
print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

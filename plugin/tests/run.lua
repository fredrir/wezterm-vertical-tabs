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
local anim = require "vtabs.anim"
local ansi = require "vtabs.ansi"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local hit = require "vtabs.hit"
local glyphs = require "vtabs.glyphs"
local model = require "vtabs.model"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local input = require "vtabs.input"
local geometry = require "vtabs.geometry"
local popover = require "vtabs.popover"
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
  local seamless = theme.resolve({ elevation = 0 }, p).bg[1]
  eq(theme.resolve({}, p).bg[1] > seamless, true, "the page is the terminal background plus the tint")
  local borrowed = theme.resolve({ use_scheme_tab_bar = true }, p).bg
  local plain = theme.resolve({}, p).bg
  eq(table.concat(borrowed, ","), table.concat(plain, ","), "nothing is borrowed")
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

---The fixture pins the geometry the positional tests count on, so schema default changes cannot
---silently move every row index in this file.
local function view(over)
  local opts = over and over.opts or {}
  opts.row_gap = opts.row_gap or 0
  opts.separator = opts.separator or "rule"
  -- pinned to the shipped defaults after addendum 2: pad / title / pad, no gap row
  if opts.meta == nil then
    opts.meta = false
  end
  local cfg = config.setup(opts)
  local v = {
    cols = 28,
    rows = 10,
    items = items(),
    theme = theme.resolve({}, palette("#1e1e2e", "#cdd6f4")),
    cfg = cfg,
    glyphs = glyphs.resolve(cfg.glyphs, {}),
    scroll = 0,
    strip = { rows = 0 },
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

test("layout: pinned block, separator, padded cards, ghost card", function()
  local r = render.render(view())
  eq(r.hits[1].id, 1, "pinned entry is one dense row")
  eq(r.hits[1].part, "title")
  eq(r.hits[1].pinned, true)
  eq(r.hits[2].kind, "separator")
  eq(r.hits[3].id, 2)
  eq(r.hits[3].part, "pad", "a card opens with a blank row")
  eq(r.hits[4].part, "title", "its content is centred")
  eq(r.hits[4].id, 2)
  eq(r.hits[5].part, "pad")
  eq(r.hits[5].slot, r.hits[3].slot)
  eq(r.hits[6].id, 3)
  eq(r.total_rows, 8)
  for row = 8, 10 do
    eq(r.hits[row].kind, "new_tab", "ghost row " .. row)
  end
  assert(strip(r.data):find "New tab")
  assert(strip(r.data):find "…", "long title truncated")
  local sep = render.render(view { opts = { separator = "none" } })
  eq(sep.hits[2].id, 2, "no separator row")
end)

test("close column is reserved so hover does not reflow the title", function()
  local plain = render.render(view())
  local hovered = render.render(view { hover = { x = 5, y = 7 } })
  local a, b = row_text(plain.data, 7), row_text(hovered.data, 7)
  eq(a:sub(1, 20), b:sub(1, 20))
  eq(hit.span(hovered.hits[7], 25), "close", "close span on the title row")
  eq(hit.span(hovered.hits[7], 27), "close")
  eq(hit.span(hovered.hits[7], 24), nil)
  eq(hit.span(hovered.hits[7], 28), nil)
  eq(hit.span(hovered.hits[6], 26), nil, "a pad row offers no sub-target")
  eq(usub(b, 26, 26), "✖", "glyph sits one col inside the card edge")
  eq(hit.span(plain.hits[4], 26), "close", "active card shows close")
  eq(hit.span(plain.hits[7], 26), nil, "idle card does not")
  local meta = render.render(view { opts = { meta = "auto" }, hover = { x = 5, y = 4 } })
  eq(meta.hits[5].part, "meta")
  eq(hit.span(meta.hits[5], 26), "close", "and on the meta row when there is one")
end)

test("unseen marker survives hover and always-close", function()
  local r = render.render(view { hover = { x = 5, y = 7 }, opts = { close_button = "always" } })
  eq(usub(row_text(r.data, 7), 3, 3), "•", "unseen dot survives in the gutter")
end)

test("pinned entries are one dense row with a pin span, never a close span", function()
  local r = render.render(view { hover = { x = 27, y = 1 } })
  eq(hit.span(r.hits[1], 26), "pin")
  eq(hit.span(r.hits[1], 26) == "close", false, "never a close span")
  eq(usub(row_text(r.data, 1), 26, 26), "*", "pin glyph in the close column")
  eq(r.hits[2].kind, "separator", "no pad or gap row after a dense entry")
  local full = render.render(view { opts = { pinned_style = "full" }, hover = { x = 27, y = 2 } })
  eq(full.hits[1].part, "pad", "pinned_style=full gives the pinned entry a padded card")
  eq(full.hits[2].part, "title")
end)

test("drag ghost reorders, previews pin state and keeps the plan length", function()
  local idle = render.render(view())
  local r = render.render(view { drag = { tab_id = 3, over_index = 1, active = true } })
  eq(r.hits[2].id, 3, "ghost keeps its armed height inside the pinned block")
  eq(r.hits[2].pinned, true)
  eq(r.hits[4].id, 1)
  eq(r.hits[5].kind, "separator")
  eq(r.total_rows, idle.total_rows, "plan length is constant across the pin boundary")
  local r2 = render.render(view { drag = { tab_id = 3, over_index = 2, active = true } })
  eq(r2.hits[4].id, 3)
  eq(r2.hits[4].pinned, false)
  eq(r2.hits[6].id, 2)
  eq(r2.total_rows, idle.total_rows)
end)

test("scroll clamps, ensure_visible follows active, footer is sticky", function()
  local many = {}
  for i = 1, 30 do
    many[i] =
      { tab_id = i, index = i, is_active = i == 25, is_pinned = false, title = "t" .. i, icon = "", has_unseen = false }
  end
  local r = render.render(view { items = many, rows = 10, scroll = 999, footer = { "space: work" } })
  eq(r.total_rows, 90, "3 rows per card")
  eq(r.scroll, 84, "clamped to max_scroll")
  eq(r.hits[10].kind, "footer", "footer is the last row, below the ghost card")
  eq(r.hits[7].kind, "new_tab")
  r = render.render(view { items = many, rows = 10, scroll = 0, ensure_visible = 25 })
  local rows_seen = 0
  for row = 1, 10 do
    if r.hits[row].id == 25 then
      rows_seen = rows_seen + 1
    end
  end
  assert(rows_seen >= 2, "the whole card is in view, not just its title row")
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
  eq(hit.drop_slot(hits, 3, 5), 2)
  eq(hit.drop_slot(hits, 4, 5), 3)
  eq(hit.drop_slot(hits, 5, 5), 3)
  eq(hit.drop_slot(hits, 1, 5), 1)
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
  config.setup { meta = "auto", backend = { path = "/bin/wez-vtabs" } }
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

---Sidebars attach on activation, so a test that wants them all has to visit every tab.
local function attach_all(win, gui)
  local was = win.active_tab_ref
  for _, tab in ipairs(win.tab_list) do
    win.active_tab_ref = tab
    sidebar.ensure(gui)
  end
  win.active_tab_ref = was
  sidebar.ensure(gui)
end

local function mark_ready(tab)
  local sb = sidebar.find(tab)
  sb.vars.vtabs_token = state.token_for(sb:pane_id())
  return sb
end

test("ensure attaches one authenticated sidebar per tab and sends auth", function()
  local win, gui = setup_window(2)
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
  local win, gui = setup_window(1)
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
  attach_all(win, gui)
  local victim = win.tab_list[2]
  mark_ready(victim)
  table.remove(victim.pane_list, 2)
  sidebar.ensure(gui)
  eq(#win.tab_list, 1)
  eq(win.active_tab_ref, win.tab_list[1])
end)

test("collapsed = hidden detaches, expand re-attaches", function()
  local win, gui = setup_window(2)
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
  local row = nil
  for r, h in pairs(hits) do
    if h.kind == "tab" and h.id == win.tab_list[2].id and h.part == "title" then
      row = r
    end
  end
  assert(row, "second tab has a card")
  local function middle(kind)
    input.handle(gui, sb, "vtabs", string.format('{"t":"mouse","k":"%s","b":"middle","x":3,"y":%d}', kind, row))
  end
  middle "down"
  eq(#win.tab_list, 2, "the press alone closes nothing; the overlay would die on the release")
  middle "up"
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

-- implementer-1: P1 render track -------------------------------------------

local function frame_rows(v)
  local r = render.render(v)
  local rows = {}
  for row = 1, v.rows do
    rows[row] = row_text(r.data, row)
  end
  return rows, r
end

---Design frames are written only when asked for, so `just check` touches nothing outside the repo.
local FRAME_DIR = os.getenv "VTABS_DUMP_FRAMES"

local function dump_frame(name, v)
  local rows = frame_rows(v)
  if not FRAME_DIR or FRAME_DIR == "" then
    return rows
  end
  os.execute("mkdir -p " .. FRAME_DIR)
  local f = io.open(FRAME_DIR .. "/" .. name .. ".txt", "w")
  if not f then
    return rows
  end
  local tens, ones = {}, {}
  for x = 1, v.cols do
    tens[x] = x % 10 == 0 and tostring(x // 10) or " "
    ones[x] = tostring(x % 10)
  end
  f:write(table.concat(tens), "\n", table.concat(ones), "\n")
  for _, line in ipairs(rows) do
    f:write(line, "\n")
  end
  f:close()
  return rows
end

local function p1_items()
  return {
    {
      tab_id = 1,
      index = 1,
      is_active = false,
      is_pinned = true,
      title = "dotfiles",
      meta = "~/dotfiles",
      icon = "~",
      has_unseen = false,
    },
    {
      tab_id = 2,
      index = 2,
      is_active = true,
      is_pinned = false,
      title = "wezterm-vertical-tabs",
      meta = "~/projects/wez-plugins",
      icon = "v",
      has_unseen = false,
    },
    {
      tab_id = 3,
      index = 3,
      is_active = false,
      is_pinned = false,
      title = "claude",
      meta = "~/projects/api",
      icon = "*",
      has_unseen = true,
    },
  }
end

local function p1_view(over)
  local v = view(over)
  v.items = (over and over.items) or p1_items()
  return v
end

test("P1 frames: every row is exactly cols wide in every new state", function()
  local variants = {
    p1_view { opts = { separator = "gap" } },
    p1_view { hover = { x = 5, y = 4 } },
    p1_view { hover = { x = 26, y = 4 } },
    p1_view { strip = { rows = 3, toggle = { row = 2, x = 11, x1 = 10, x2 = 13 } } },
    p1_view { strip = { rows = 2, toggle = { row = 1, x = 2, x1 = 1, x2 = 4 } }, opts = { position = "right" } },
    p1_view { private = true },
    p1_view { opts = { meta = false } },
    p1_view { opts = { icons = false, close_button = "never" } },
    p1_view { opts = { pinned_style = "full" } },
    p1_view { hover = { x = 5, y = 9 } },
    p1_view { rows = 6 },
    p1_view { rows = 4 },
    p1_view { opts = { padding = { top = 0, left = 0, right = 0 } } },
    p1_view { opts = { width = 40 }, cols = 40 },
    p1_view { drag = { tab_id = 3, over_index = 1, active = true, outside = true } },
    p1_view { scroll = 3, rows = 8 },
    p1_view { footer = { { icon = "f", text = "main · 3 dirty", id = "git" } } },
  }
  for i, v in ipairs(variants) do
    local rows = frame_rows(v)
    for row = 1, v.rows do
      eq(util.width(rows[row]), v.cols, "variant " .. i .. " row " .. row)
    end
  end
end)

test("P1 grid: landmarks derive from cols and padding", function()
  local rows = frame_rows(p1_view { opts = { separator = "gap" } })
  eq(usub(rows[1], 5, 5), "~", "icon at icon_x")
  eq(usub(rows[1], 7, 14), "dotfiles", "title at title_x1")
  eq(usub(rows[4], 5, 5), "v", "the icon rides the centred title row")
  eq(usub(rows[4], 3, 3), " ", "theme.title_active is hue-distinct here, so no bar")
  eq(usub(rows[3], 3, 3), " ", "and a pad row carries nothing at all")
  local metaed = frame_rows(p1_view { opts = { separator = "gap", meta = "auto" } })
  eq(usub(metaed[5], 7, 25), "~/p/wez-plugins    ", "meta at meta_x1, elided in the middle")
  local wide = frame_rows(p1_view { opts = { width = 40, separator = "gap" }, cols = 40 })
  eq(usub(wide[1], 7, 14), "dotfiles", "title column does not move with width")
end)

test("P1 chamfer: the card's own first and last row, right side only", function()
  local rows = frame_rows(p1_view { rows = 12, opts = { separator = "gap" } })
  eq(usub(rows[4], 27, 27), " ", "the active card is square")
  local hovered_rows = frame_rows(p1_view { rows = 12, hover = { x = 5, y = 7 }, opts = { separator = "gap" } })
  eq(usub(hovered_rows[6], 27, 27), "▙", "a hovered card chamfers on its first row")
  eq(usub(hovered_rows[7], 27, 27), " ", "never on the title row, which is no longer an edge")
  eq(usub(hovered_rows[8], 27, 27), "▛", "and closes on its last")
  assert(usub(rows[4], 2, 2) ~= "▙", "col 2 is the gutter, never a chamfer")
  local one_row =
    frame_rows(p1_view { rows = 12, hover = { x = 5, y = 3 }, opts = { tab_height = "row", separator = "gap" } })
  for _, line in ipairs(one_row) do
    assert(not line:find("▙", 1, true) and not line:find("▛", 1, true), "1-row cards are square")
  end
  local dense = frame_rows(p1_view { rows = 12, hover = { x = 5, y = 1 }, opts = { separator = "gap" } })
  assert(not dense[1]:find("▙", 1, true), "a hovered dense pinned row stays square")
end)

test("P1 hits: one record per row across the whole card", function()
  local r = render.render(p1_view { hover = { x = 5, y = 4 }, opts = { separator = "gap" } })
  for _, row in ipairs { 3, 4, 5 } do
    eq(r.hits[row].kind, "tab", "row " .. row)
    eq(r.hits[row].id, 2)
    eq(r.hits[row].slot, 2)
    eq(r.hits[row].x1, 3)
    eq(r.hits[row].x2, 27)
  end
  eq(r.hits[3].part, "pad")
  eq(r.hits[4].part, "title")
  eq(r.hits[5].part, "pad")
  eq(hit.in_card(r.hits[3], 2), false, "cols 1-2 are page, not card")
  eq(hit.in_card(r.hits[3], 28), false, "col 28 is the thumb channel")
  eq(hit.in_card(r.hits[3], 3), true)
  eq(hit.drop_slot(r.hits, 3, 10), 2, "a pad row drops at its own slot")
  eq(hit.drop_slot(r.hits, 4, 10), 2, "the title row too")
  eq(hit.drop_slot(r.hits, 9, 10), 4, "below the last card")
  local gapped = render.render(p1_view { rows = 14, opts = { separator = "gap", row_gap = 1 } })
  local gap_row
  for row = 1, 14 do
    if gapped.hits[row].part == "gap" then
      gap_row = gap_row or row
    end
  end
  assert(gap_row, "row_gap = 1 still emits gap rows")
  eq(hit.drop_slot(gapped.hits, gap_row, 14), gapped.hits[gap_row].slot + 1, "a gap row drops below its card")
end)

test("P1 ghost card: outlined, sticky, exactly cols wide idle and hovered", function()
  local idle, r = frame_rows(p1_view { opts = { separator = "gap" } })
  eq(usub(idle[8], 3, 3), "╭")
  eq(usub(idle[8], 27, 27), "╮")
  eq(usub(idle[9], 5, 5), "+")
  eq(usub(idle[9], 7, 14), "New tab ", "label at title_x1")
  eq(usub(idle[10], 3, 3), "╰")
  eq(usub(idle[10], 27, 27), "╯")
  for row = 8, 10 do
    eq(r.hits[row].kind, "new_tab")
    eq(r.hits[row].x1, 3)
    eq(r.hits[row].x2, 27)
  end
  local hover = frame_rows(p1_view { hover = { x = 5, y = 9 }, opts = { separator = "gap" } })
  for row = 8, 10 do
    eq(hover[row], idle[row], "hover redraws no glyph, only recolours row " .. row)
  end
  eq(util.width(hover[8]), 28)
  local tight = render.render(p1_view { rows = 6, opts = { separator = "gap" } })
  eq(tight.hits[6].kind, "new_tab", "degrades to a single row")
  local tiny = render.render(p1_view { rows = 2, opts = { separator = "gap" } })
  eq(tiny.hits[2].kind ~= "new_tab", true, "and drops out entirely rather than starving the list")
end)

test("item 7: the ghost card's hover is one border step and no inline band", function()
  local function ghost(over)
    local v = p1_view(over)
    local rows, r = frame_rows(v)
    local top
    for row = 1, v.rows do
      if r.hits[row] and r.hits[row].kind == "new_tab" then
        top = top or row
      end
    end
    return top, rows, r, v
  end
  local top, idle_rows, idle = ghost { rows = 20, opts = { separator = "gap" } }
  local hover_top, hover_rows, hovered, v = ghost { rows = 20, hover = { x = 5, y = 19 }, opts = { separator = "gap" } }
  eq(hover_top, top, "hover moves nothing")
  for i = 0, 2 do
    eq(hover_rows[top + i], idle_rows[top + i], "hover redraws no glyph on ghost row " .. i)
  end
  eq(usub(idle_rows[top], 3, 3), "╭", "and the corners stay closed")
  eq(usub(idle_rows[top], 27, 27), "╮")
  eq(usub(idle_rows[top + 2], 3, 3), "╰")
  eq(usub(idle_rows[top + 2], 27, 27), "╯")
  assert(idle.rows[top]:find(ansi.fg(v.theme.border_idle), 1, true), "the idle border is border_idle")
  assert(hovered.rows[top]:find(ansi.fg(v.theme.ghost_border_hover), 1, true), "hover takes the half step to hue")
  assert(not hovered.rows[top]:find(ansi.fg(v.theme.accent), 1, true), "but never all the way to the accent")
  assert(not hovered.rows[top + 1]:find(ansi.bg(v.theme.hover_bg), 1, true), "the label keeps the page behind it")
  assert(not idle.rows[top + 1]:find(ansi.bg(v.theme.hover_bg), 1, true), "in both states")

  assert(
    theme.contrast(v.theme.ghost_border_hover, v.theme.border_idle)
      > theme.contrast(v.theme.border, v.theme.border_idle),
    "and it is a bigger step than border alone, which is what read as no step at all"
  )
end)

test("P1 strip: reserve rows, action cluster, never over a list row", function()
  local v = p1_view { strip = { rows = 3, cols = 0, toggle_row = 2 }, opts = { separator = "gap" } }
  local rows, r = frame_rows(v)
  eq(usub(rows[2], 3, 3), "«", "with no lights the cluster starts where the toggle already was")
  eq(usub(rows[2], 6, 6), "+", "and new tab follows one span along")
  eq(r.hits[1].kind, "strip")
  eq(r.hits[1].x1, nil, "strip reserve is not clickable")
  eq(r.hits[2].kind, "action")
  eq(r.hits[2].x1, 2)
  eq(r.hits[2].x2, 7)
  eq(hit.span(r.hits[2], 2), "toggle")
  eq(hit.span(r.hits[2], 4), "toggle")
  eq(hit.span(r.hits[2], 5), "new_tab", "the spans are contiguous, with no dead cell between")
  eq(hit.span(r.hits[2], 7), "new_tab")
  eq(hit.span(r.hits[2], 8), nil)
  eq(r.hits[3].kind, "action", "the band is 2 rows and stays inside the strip")
  eq(r.hits[4].kind, "tab", "the list starts below the strip")
  local right = frame_rows(p1_view {
    strip = { rows = 2, cols = 0, toggle_row = 1 },
    opts = { position = "right", separator = "gap" },
  })
  eq(usub(right[1], 2, 2), "»", "position=right mirrors the padding and flips the toggle glyph")
  eq(usub(right[1], 5, 5), "+")
end)

test("addendum 2 A8a: every action column derives from the reserve, whatever it measures", function()
  -- 70 px of buttons is 9 cells at the default macOS font and 10 at the next cell width up
  for _, reserve in ipairs { 9, 10 } do
    local v = p1_view {
      strip = { rows = 4, cols = reserve, toggle_row = 1 },
      opts = { separator = "gap", strip_actions = { "toggle", "new_tab", "settings" } },
    }
    v.cfg.hooks.settings = function() end
    local rows, r = frame_rows(v)
    local base = reserve + 2
    eq(usub(rows[1], base, base), "«", reserve .. ": first glyph two columns clear of the last light")
    eq(usub(rows[1], base + 3, base + 3), "+")
    eq(usub(rows[1], base + 6, base + 6), "⚙")
    local spans = r.hits[1].spans
    eq(#spans, 3)
    eq(spans[1].x1, base - 1, reserve .. ": the first span opens on the reserve's last column")
    eq(spans[1].x2, base + 1)
    eq(spans[2].x1, base + 2, reserve .. ": contiguous and non-overlapping")
    eq(spans[3].x2, base + 7)
    for _, row in ipairs { 1, 2 } do
      eq(r.hits[row].kind, "action", reserve .. ": both reserved rows take the click")
    end
    eq(r.hits[3].kind, "strip", reserve .. ": the alignment row does not")

    v.cfg.hooks.settings = nil
    local _, dropped = frame_rows(v)
    eq(#dropped.hits[1].spans, 2, reserve .. ": settings is not drawn without a hook to answer it")
  end
end)

test("addendum 2 A8d: hovering one action lights only its own three columns", function()
  local base = p1_view { strip = { rows = 3, cols = 0, toggle_row = 2 }, opts = { separator = "gap" } }
  local idle = render.render(base)
  local lit = render.render(p1_view {
    strip = { rows = 3, cols = 0, toggle_row = 2 },
    hover = { x = 6, y = 2 },
    opts = { separator = "gap" },
  })
  assert(not idle.rows[2]:find(ansi.bg(base.theme.hover_bg), 1, true), "nothing is lit while the pointer is away")
  local body = strip(lit.rows[2])
  eq(usub(body, 6, 6), "+", "the hovered glyph is still its own")
  local plan = require("vtabs.layout").plan(p1_view {
    strip = { rows = 3, cols = 0, toggle_row = 2 },
    hover = { x = 6, y = 2 },
    opts = { separator = "gap" },
  })
  eq(plan.rows[2].lit_id, "new_tab", "and only that action is lit")
  eq(plan.rows[3].lit_id, "new_tab", "on both rows of the band")
  local off = require("vtabs.layout").plan(p1_view {
    strip = { rows = 3, cols = 0, toggle_row = 2 },
    hover = { x = 9, y = 2 },
    opts = { separator = "gap" },
  })
  eq(off.rows[2].lit_id, nil, "a column between the cluster and the list lights nothing")
end)

test("addendum 2 §8: the rail keeps only what fits, centred", function()
  local narrow = p1_view { rows = 16, cols = 5, opts = { separator = "gap", width = 8 } }
  narrow.rail = true
  narrow.strip = { rows = 2, cols = 0, toggle_row = 1 }
  local rows, r = frame_rows(narrow)
  eq(usub(rows[1], 3, 3), "«", "one action, centred at ceil(width / 2)")
  eq(#r.hits[1].spans, 1)
  local wide = p1_view { rows = 16, cols = 9, opts = { separator = "gap", width = 9 } }
  wide.rail = true
  wide.strip = { rows = 2, cols = 0, toggle_row = 1 }
  local wide_rows, wr = frame_rows(wide)
  eq(#wr.hits[1].spans, 2, "nine columns hold the pair")
  eq(usub(wide_rows[1], 2, 2), "«")
  eq(usub(wide_rows[1], 5, 5), "+")
  local reserved = p1_view { rows = 16, cols = 9, opts = { separator = "gap", width = 9 } }
  reserved.rail = true
  reserved.strip = { rows = 4, cols = 9, toggle_row = 3 }
  local _, rr = frame_rows(reserved)
  eq(#rr.hits[3].spans, 1, "under the lights there is only room for the first")
end)

test("P1 scroll: thumb by state, edge fade, footer below the ghost card", function()
  local many = {}
  for i = 1, 12 do
    many[i] = {
      tab_id = i,
      index = i,
      is_active = i == 1,
      is_pinned = false,
      title = "tab " .. i,
      meta = "~/p" .. i,
      icon = "t",
      has_unseen = false,
    }
  end
  local v = p1_view { items = many, rows = 12, scroll = 4, footer = { "space: work" } }
  local rows, r = frame_rows(v)
  eq(r.hits[12].kind, "footer", "footer occupies the last row")
  eq(r.hits[9].kind, "new_tab")
  local thumb_rows = 0
  for row = 1, 8 do
    if usub(rows[row], 28, 28) == "▐" then
      thumb_rows = thumb_rows + 1
    end
  end
  assert(thumb_rows > 0, "thumb drawn in col 28")
  local quiet = render.render(p1_view { items = many, rows = 12, scroll = 4 })
  assert(quiet.data:find("▐", 1, true), "thumb still drawn when nothing is hovered")
  local none = render.render(p1_view { items = many, rows = 12, opts = { scroll_indicator = "never" } })
  assert(not none.data:find("▐", 1, true), "never means never")
  local short = render.render(p1_view { rows = 20 })
  assert(not short.data:find("▐", 1, true), "no thumb when everything fits")
end)

test("P1 glyph guard: groups substitute together, N glyphs survive", function()
  local base = config.setup({}).glyphs
  local plain = glyphs.resolve(base, {})
  eq(plain.corners, "chamfer")
  eq(plain.chamfer_top, "▙")
  local no_block = glyphs.resolve(base, { custom_block_glyphs = false })
  eq(no_block.corners, "square")
  eq(no_block.chamfer_top, " ")
  eq(no_block.active, "|")
  eq(no_block.scroll, "|")
  local wide = glyphs.resolve(base, { treat_east_asian_ambiguous_width_as_wide = true })
  eq(wide.active, "|", "ambiguous bar substitutes")
  eq(wide.ellipsis, "...")
  eq(wide.chamfer_top, "▙", "neutral block glyphs survive")
  eq(wide.scroll, "▐")
  eq(wide.toggle_left, "<", "the ambiguous guillemets substitute quietly instead of tripping the width backstop")
  eq(wide.frame_tl, "+", "ghost frame substitutes as a unit")
  eq(wide.frame_dash, "-", "including its neutral member")
  local v14 = glyphs.resolve(base, { unicode_version = 14 })
  eq(v14.frame_tl, "╭", "unicode_version alone does not select ambiguous width")
  eq(v14.active, "▎")
  local ascii = p1_view { opts = { separator = "gap" } }
  ascii.glyphs = wide
  local rows = frame_rows(ascii)
  for row = 1, ascii.rows do
    eq(util.width(rows[row]), 28, "ascii ghost frame row " .. row)
  end
end)

test("P1 private window: header, inert hit, accent shift", function()
  local rows, r = frame_rows(p1_view { private = true, opts = { separator = "gap" } })
  eq(usub(rows[1], 7, 13), "Private")
  eq(r.hits[1].kind, "space", "the header is inert")
  eq(r.hits[2].kind, "space")
  eq(r.hits[3].id, 1, "the list starts below it")
end)

test("P1 strip defaults to padding.top when no caller wired one up", function()
  local v = p1_view { opts = { padding = { top = 2, left = 1, right = 1 } } }
  v.strip = nil
  local r = render.render(v)
  eq(r.hits[1].kind, "strip", "the reserve owns the rows above the first card")
  eq(r.hits[2].kind, "strip")
  eq(r.hits[3].id, 1, "the list starts below it")
  local flush = p1_view { opts = { padding = { top = 0, left = 1, right = 1 } } }
  flush.strip = nil
  eq(render.render(flush).hits[1].id, 1, "padding.top = 0 is flush")
end)

test("P1 pin span exists only while the glyph is drawn", function()
  local idle = render.render(p1_view { opts = { separator = "gap" } })
  eq(hit.span(idle.hits[1], 26), nil, "no invisible click target on a dense row")
  local hovered = render.render(p1_view { hover = { x = 5, y = 1 }, opts = { separator = "gap" } })
  eq(hit.span(hovered.hits[1], 26), "pin")
end)

test("P1 hover=press shows the close button on every card", function()
  local press = render.render(p1_view { opts = { separator = "gap", hover = "press" } })
  eq(hit.span(press.hits[7], 26), "close", "an idle card still offers close in press mode")
  local follow = render.render(p1_view { opts = { separator = "gap", hover = "follow" } })
  eq(hit.span(follow.hits[7], 26), nil)
end)

test("P1 rule separator goes through the glyph guard", function()
  local base = config.setup({}).glyphs
  local v = p1_view { opts = { separator = "rule" } }
  v.glyphs = glyphs.resolve(base, { treat_east_asian_ambiguous_width_as_wide = true })
  local rows = frame_rows(v)
  eq(usub(rows[2], 3, 3), "-", "ascii rule when the box glyph is unsafe")
  eq(util.width(rows[2]), 28)
end)

test("P1 edge fade lands on a painted row, never on a gap", function()
  local many = {}
  for i = 1, 12 do
    many[i] = {
      tab_id = i,
      index = i,
      is_active = false,
      is_pinned = false,
      title = "tab " .. i,
      meta = "~/p" .. i,
      icon = "t",
      has_unseen = false,
    }
  end
  -- scroll = 3 puts a gap row first: the fade has to skip it
  local r = render.render(p1_view { items = many, rows = 12, scroll = 3, opts = { row_gap = 1 } })
  local faded_rows = 0
  for line in r.data:gmatch "\27%[38;2;%d+;%d+;%d+m" do
    if line then
      faded_rows = faded_rows + 1
    end
  end
  assert(faded_rows > 0, "frame paints")
  eq(r.hits[1].part, "gap", "first list row is a gap at this offset")
  local plain = render.render(p1_view { items = many, rows = 12, scroll = 0, opts = { row_gap = 1 } })
  assert(r.data ~= plain.data, "a scrolled frame differs from the unscrolled one")
end)

test("P1 sibling paths stay distinguishable on the meta line", function()
  local function sibling_items(a, b)
    return {
      { tab_id = 1, index = 1, is_active = false, is_pinned = false, title = "api", meta = a, icon = "t" },
      { tab_id = 2, index = 2, is_active = false, is_pinned = false, title = "web", meta = b, icon = "t" },
    }
  end
  local v = p1_view {
    items = sibling_items("~/work/acme/services/api", "~/work/acme/services/web"),
    opts = { separator = "gap" },
  }
  local rows = frame_rows(v)
  local first, second = usub(rows[2], 6, 25), usub(rows[5], 6, 25)
  assert(first ~= second, "siblings must not collapse to their shared parent")
  assert(first:find("api", 1, true), "basename kept: " .. first)
  assert(second:find("web", 1, true), "basename kept: " .. second)
  eq(util.width(rows[2]), 28)
  local windows = p1_view {
    items = sibling_items([[C:\Users\me\work\acme\api]], [[C:\Users\me\work\acme\web]]),
    opts = { separator = "gap" },
  }
  -- routed to shorten_path; it splits on "/" only, so windows siblings still collapse (util.lua)
  local win_rows = frame_rows(windows)
  eq(util.width(win_rows[2]), 28)
  eq(util.width(win_rows[5]), 28)
  local sep = config.get().meta_sep
  local composite = p1_view {
    items = sibling_items(
      "nvim" .. sep .. "~/work/acme/services/api",
      "SSH:archie" .. sep .. "~/work/acme/services/web"
    ),
    opts = { separator = "gap" },
  }
  local comp = frame_rows(composite)
  assert(usub(comp[2], 6, 25):find("api", 1, true), "the tail after the separator is the path")
  assert(usub(comp[5], 6, 25):find("web", 1, true))
  eq(util.width(comp[5]), 28)
end)

test("P1 fuzz: every CUP row and hit key stays inside the pane", function()
  local function row_keys(data)
    local rows = {}
    for r in data:gmatch "\27%[(%d+);1H" do
      rows[#rows + 1] = tonumber(r)
    end
    return rows
  end
  local checked = 0
  for _, rows in ipairs { 1, 2, 3, 4, 5, 6, 8, 12, 20 } do
    for _, strip_n in ipairs { 0, 1, 2, 3, 5 } do
      for _, n in ipairs { 0, 1, 3, 8 } do
        for _, footer_n in ipairs { 0, 1, 3 } do
          local list = {}
          for i = 1, n do
            list[i] = {
              tab_id = i,
              index = i,
              is_active = i == 1,
              is_pinned = i == 2,
              title = "t" .. i,
              meta = "~/p" .. i,
              icon = "t",
              has_unseen = false,
            }
          end
          local footer = {}
          for i = 1, footer_n do
            footer[i] = "f" .. i
          end
          local v = p1_view { items = list, rows = rows, footer = footer }
          v.strip = { rows = strip_n, toggle = strip_n > 0 and { row = 1, x = 2, x1 = 1, x2 = 4 } or nil }
          local r = render.render(v)
          checked = checked + 1
          for _, row in ipairs(row_keys(r.data)) do
            assert(row >= 1 and row <= rows, string.format("CUP row %d outside 1..%d", row, rows))
          end
          for row in pairs(r.hits) do
            assert(row >= 1 and row <= rows, string.format("hit row %d outside 1..%d", row, rows))
          end
          for row in pairs(r.rows) do
            assert(row >= 1 and row <= rows, string.format("row text %d outside 1..%d", row, rows))
          end
        end
      end
    end
  end
  assert(checked >= 200, "fuzzed " .. checked .. " layouts")
end)

test("P2 row diff: only changed rows are sent, and they rejoin the same frame", function()
  local view_mod = require "vtabs.view"
  local dims = { cols = 28, viewport_rows = 20 }
  local base = render.render(p1_view { rows = 20 })
  state.session.frames[901] = nil
  eq(view_mod.payload_for(901, base, dims, false), base.data, "a cold pane gets the whole frame")
  state.session.frames[901] = { cols = 28, rows = 20, text = base.rows, n = base.rows_n }
  eq(view_mod.payload_for(901, base, dims, false), nil, "an unchanged frame sends nothing")

  local hovered = render.render(p1_view { rows = 20, hover = { x = 5, y = 6 } })
  local payload = view_mod.payload_for(901, hovered, dims, false)
  assert(payload, "hover changed something")
  local sent = 0
  for _ in payload:gmatch "\27%[%d+;1H" do
    sent = sent + 1
  end
  assert(sent <= 3, "hover moved " .. sent .. " rows")
  assert(#payload < #hovered.data, "diff is smaller than the frame")

  local joined = { ansi.HIDE_CURSOR }
  for row = 1, hovered.rows_n do
    joined[#joined + 1] = ansi.cup(row, 1) .. hovered.rows[row]
  end
  joined[#joined + 1] = ansi.RESET
  eq(table.concat(joined), hovered.data, "rows rejoin byte-for-byte into the full frame")

  eq(view_mod.payload_for(901, hovered, { cols = 30, viewport_rows = 20 }, false), hovered.data, "dims change repaints")
  eq(view_mod.payload_for(901, hovered, dims, true), hovered.data, "force repaints")
  state.session.frames[901] = nil
end)

test("P2 row diff: a colour-only change is still re-sent", function()
  local view_mod = require "vtabs.view"
  local dims = { cols = 28, viewport_rows = 20 }
  local plain = render.render(p1_view { rows = 20, hover = { x = 5, y = 6 } })
  state.session.frames[902] = { cols = 28, rows = 20, text = plain.rows, n = plain.rows_n }
  local on_close = render.render(p1_view { rows = 20, hover = { x = 26, y = 6 } })
  local payload = view_mod.payload_for(902, on_close, dims, false)
  assert(payload, "close_hover_fg is a real change even though the text matches")
  eq(strip(plain.rows[6]), strip(on_close.rows[6]), "same glyphs, different colour")
  state.session.frames[902] = nil
end)

test("sanitize always returns valid UTF-8, whatever bytes arrive", function()
  local cases = {
    { "\155", "raw C1 0x9b" },
    { "a\155b", "raw C1 inside text" },
    { "~/p/\194", "truncated 2-byte lead" },
    { "~/p/\226\130", "truncated 3-byte" },
    { "\240\159\146", "truncated 4-byte" },
    { "\192\175", "overlong slash" },
    { "\224\128\175", "3-byte overlong" },
    { "\237\160\128", "surrogate half D800" },
    { "\237\191\191", "surrogate half DFFF" },
    { "\244\144\128\128", "past U+10FFFF" },
    { "\128\129\130", "lone continuations" },
    { "\27]0;title\7", "OSC in a title" },
    { "ok\0bad", "embedded NUL" },
  }
  for _, case in ipairs(cases) do
    local out = util.sanitize(case[1])
    assert(utf8.len(out) ~= nil, case[2] .. " left invalid UTF-8: " .. string.format("%q", out))
    assert(not out:find "[%z\1-\31\127]", case[2] .. " left a control char")
    util.width(out)
    util.truncate(out, 8, "…")
    util.shorten_path(out, 8)
  end
  eq(util.sanitize "~/projects/api", "~/projects/api", "clean text is untouched")
  eq(util.sanitize "日本語", "日本語", "multibyte survives")
  eq(util.sanitize "safe\226\128\174gpj.exe", "safegpj.exe", "RLO cannot disguise a filename")
  for _, cp in ipairs { "\226\128\170", "\226\128\171", "\226\128\172", "\226\128\173", "\226\128\174" } do
    eq(util.sanitize("a" .. cp .. "b"), "ab", "bidi override U+202A-202E stripped")
  end
  eq(util.sanitize "\226\129\166ا\226\129\169", "\226\129\166ا\226\129\169", "isolates U+2066-2069 kept")
  eq(util.sanitize "a\194\133b", "ab", "C1 in UTF-8 form still goes")
end)

test("sanitize fuzz: 500 random byte strings never break width or render", function()
  math.randomseed(20260830)
  local fuzz_items = {}
  for n = 1, 500 do
    local bytes = {}
    for _ = 1, math.random(1, 24) do
      bytes[#bytes + 1] = string.char(math.random(0, 255))
    end
    local raw = table.concat(bytes)
    local clean = util.sanitize(raw)
    assert(utf8.len(clean) ~= nil, string.format("case %d not valid UTF-8: %q", n, clean))
    fuzz_items[1] = {
      tab_id = 1,
      index = 1,
      is_active = true,
      is_pinned = false,
      title = raw,
      meta = raw,
      icon = raw,
      has_unseen = false,
    }
    local v = p1_view { items = fuzz_items, rows = 8 }
    local r = render.render(v)
    for row = 1, 8 do
      if r.rows[row] then
        eq(util.width(row_text(r.data, row)), 28, string.format("case %d row %d", n, row))
      end
    end
  end
end)

test("one unrenderable tab does not stop the other sidebars", function()
  local win, gui = setup_window(2)
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  local view_mod = require "vtabs.view"
  view_mod.sync(gui, { force = true })
  local second = sidebar.find(win.tab_list[2])
  local before = #second.sent
  local victim = sidebar.content_pane(win.tab_list[1])
  victim.get_title = function()
    error "pane went away mid-render"
  end
  win.tab_list[1].get_title = function()
    error "tab went away mid-render"
  end
  view_mod.sync(gui, { force = true })
  assert(#second.sent > before, "the healthy sidebar still got its frame")
end)

test("P1 screenshots: icon weight, chamfer, toggle surface, dashed ghost", function()
  local v = p1_view { rows = 20, hover = { x = 5, y = 8 } }
  v.strip = { rows = 2, toggle = { row = 1, x = 2, x1 = 1, x2 = 4 } }
  local r = render.render(v)
  local rows = {}
  for row = 1, v.rows do
    rows[row] = row_text(r.data, row)
  end

  local active_row, hover_row
  for row = 1, v.rows do
    local h = r.hits[row]
    if h and h.kind == "tab" and h.part == "title" then
      if h.id == 2 then
        active_row = row
      elseif h.id == 3 then
        hover_row = row
      end
    end
  end
  assert(active_row and hover_row, "found both cards")

  eq(usub(rows[active_row], 27, 27), " ", "the active card is square")
  eq(usub(rows[hover_row - 1], 27, 27), "▙", "and the hovered one carries the chamfer on its edges")
  eq(usub(rows[hover_row + 1], 27, 27), "▛")

  assert(r.rows[active_row]:find(ansi.fg(v.theme.meta_fg), 1, true), "icon paints at meta weight")

  local lit = render.render(p1_view {
    rows = 20,
    hover = { x = 3, y = 1 },
    strip = { rows = 2, cols = 0, toggle_row = 1 },
  })
  assert(lit.rows[1]:find(ansi.bg(v.theme.hover_bg), 1, true), "the toggle span reads as a button when hovered")
  assert(not r.rows[1]:find(ansi.bg(v.theme.hover_bg), 1, true), "and is bare otherwise")

  local ghost_top
  for row = 1, v.rows do
    if r.hits[row] and r.hits[row].kind == "new_tab" then
      ghost_top = ghost_top or row
    end
  end
  eq(usub(rows[ghost_top], 4, 5), "╌╌", "every cell between the corners is dashed")
  eq(usub(rows[ghost_top], 25, 26), "╌╌", "at both ends")
  eq(usub(rows[ghost_top], 6, 7), "╌╌", "and in between: the dash lives inside the glyph")
  eq(usub(rows[ghost_top + 1], 3, 3), "╎", "the sides are dashed too")
  eq(usub(rows[ghost_top + 1], 27, 27), "╎")
  eq(usub(rows[ghost_top + 2], 4, 5), "╌╌", "the bottom rail closes the same way")
  eq(usub(rows[ghost_top + 2], 25, 26), "╌╌")
  local over = render.render(p1_view { rows = 20, hover = { x = 5, y = 19 } })
  local over_rows = {}
  for row = 1, 20 do
    over_rows[row] = row_text(over.data, row)
  end
  local top
  for row = 1, 20 do
    if over.hits[row] and over.hits[row].kind == "new_tab" then
      top = top or row
    end
  end
  eq(usub(over_rows[top], 4, 5), "╌╌", "and the hovered ghost stays dashed")
end)

local function popover_rect(over)
  local rect = {
    x = 3,
    y = 6,
    w = 24,
    h = 5,
    scrim = 0.5,
    rows = {
      { spans = { { x = 2, text = "Close tab", bold = true } }, hit = { kind = "popover", id = "close" } },
      { spans = { { x = 2, text = "Pin tab" } }, hit = { kind = "popover", id = "pin" } },
      { spans = { { x = 2, text = "Rename" } }, hit = { kind = "popover", id = "rename", disabled = true } },
      { spans = { { x = 2, text = "" } } },
      { spans = { { x = 2, text = "Move to new window" } }, hit = { kind = "popover", id = "tear_off" } },
    },
  }
  for k, v in pairs(over or {}) do
    rect[k] = v
  end
  return rect
end

test("P2 composite: the popover owns its rows and scrims the rest", function()
  local v = p1_view { rows = 20 }
  v.popover = popover_rect()
  local r = render.render(v)
  local rows = {}
  for row = 1, v.rows do
    rows[row] = row_text(r.data, row)
  end
  for row = 1, v.rows do
    eq(util.width(rows[row]), 28, "row " .. row .. " stays cols wide")
  end
  assert(rows[6]:find("Close tab", 1, true), "first item painted at the rect origin")
  eq(usub(rows[6], 4, 12), "Close tab", "span x is relative to the rect")
  eq(r.hits[6].kind, "popover")
  eq(r.hits[6].id, "close")
  eq(r.hits[8].disabled, true)
  eq(r.hits[9].kind, "popover", "a row with no hit is inert, not a scrim")
  eq(r.hits[9].id, nil)
  for _, row in ipairs { 1, 5, 11, 20 } do
    eq(r.hits[row].kind, "scrim", "row " .. row .. " outside the rect")
    eq(r.hits[row].id, nil)
  end
end)

test("P2 composite: the scrim fades foreground and background", function()
  local plain = render.render(p1_view { rows = 20 })
  local v = p1_view { rows = 20 }
  v.popover = popover_rect()
  local scrimmed = render.render(v)
  local active_row
  for row = 1, 20 do
    if plain.hits[row] and plain.hits[row].kind == "tab" and plain.hits[row].id == 2 then
      active_row = active_row or row
    end
  end
  assert(active_row and active_row < 6, "the active card sits above the popover")
  assert(scrimmed.rows[active_row] ~= plain.rows[active_row], "a scrimmed row repaints")
  eq(strip(scrimmed.rows[active_row]), strip(plain.rows[active_row]), "same glyphs, dimmer colours")
  local theme_bg = string.format("48;2;%d;%d;%d", v.theme.bg[1], v.theme.bg[2], v.theme.bg[3])
  local card_bg = string.format("48;2;%d;%d;%d", v.theme.active_bg[1], v.theme.active_bg[2], v.theme.active_bg[3])
  assert(plain.rows[active_row]:find(card_bg, 1, true), "the card paints its own bg unscrimmed")
  assert(not scrimmed.rows[active_row]:find(card_bg, 1, true), "and a faded one under the scrim")
  assert(not scrimmed.rows[active_row]:find(theme_bg .. "m" .. "%s*$"), "still not flat page bg")
end)

test("P2 composite: spans are clamped to the rect", function()
  local v = p1_view { rows = 20 }
  v.popover = popover_rect {
    rows = { { spans = { { x = 1, text = string.rep("wide", 40) } }, hit = { kind = "popover", id = "x" } } },
    h = 1,
  }
  local r = render.render(v)
  eq(util.width(row_text(r.data, 6)), 28, "an over-long span truncates, it does not widen the row")
  eq(usub(row_text(r.data, 6), 27, 28), "  ", "and stops at the rect edge")
end)

test("P2 rail: grid, cards and chrome at 5 and 9 cols", function()
  for _, cols in ipairs { 5, 9 } do
    local v = p1_view { rows = 16, cols = cols, opts = { width = math.max(cols, 8) } }
    v.rail = true
    v.strip = { rows = 2, toggle = { row = 1, x = math.ceil(cols / 2), x1 = 1, x2 = cols } }
    local r = render.render(v)
    local rows = {}
    for row = 1, v.rows do
      rows[row] = row_text(r.data, row)
    end
    for row = 1, v.rows do
      eq(util.width(rows[row]), cols, cols .. "-col rail row " .. row)
    end
    local icon_x = math.ceil(cols / 2)
    local first, second
    for row = 1, v.rows do
      local h = r.hits[row]
      if h and h.kind == "tab" and h.part == "title" then
        first = first or row
        if first and row > first then
          second = second or row
        end
      end
    end
    eq(usub(rows[first], icon_x, icon_x), "~", "icon centred at ceil(cols/2)")
    eq(r.hits[first].x1, 1, "the whole rail is the card")
    eq(r.hits[first].x2, cols)
    eq(hit.span(r.hits[first], icon_x), nil, "a rail card has no close span, by construction")
    local unpinned
    for row = 1, v.rows do
      local h = r.hits[row]
      if h and h.kind == "tab" and h.part == "title" and not h.pinned then
        unpinned = unpinned or row
      end
    end
    eq(r.hits[unpinned - 1].part, "pad", "a rail card keeps the expanded card's rows")
    eq(r.hits[unpinned + 1].part, "pad", "pads and all")
    eq(r.hits[unpinned + 1].slot, r.hits[unpinned].slot, "every row of the slot carries it")
    eq(r.hits[unpinned - 1].slot, r.hits[unpinned].slot)
    eq(usub(rows[unpinned + 1], icon_x, icon_x), " ", "but only the middle row paints")
    eq(r.hits[first + 1].part, nil, "a pinned rail entry keeps no gap, so the block stays solid")
    for row = 1, v.rows do
      assert(not rows[row]:find("▙", 1, true), "no chamfer at " .. cols .. " cols")
    end
    local ghost
    for row = 1, v.rows do
      if r.hits[row] and r.hits[row].kind == "new_tab" then
        ghost = ghost or row
      end
    end
    eq(usub(rows[ghost], 1, 1), "╭", "the rail ghost is the same outlined card")
    eq(usub(rows[ghost + 1], icon_x, icon_x), "+", "with the + on its middle row")
    eq(usub(rows[ghost + 2], cols, cols), "╯")
    assert(second, "the second card is a separate hit record")
  end
end)

test("item 2: a rail slot occupies exactly the rows the expanded card does", function()
  local layout = require "vtabs.layout"
  for _, height in ipairs { "card", "tall" } do
    local function planned(rail, cols)
      local v = p1_view { rows = 20, cols = cols, opts = { separator = "gap", width = math.max(cols, 8) } }
      v.cfg.tab_height = height
      v.rail = rail
      v.strip = { rows = 2, toggle = { row = 1, x = 2, x1 = 1, x2 = 4 } }
      return v, layout.plan(v)
    end
    local wide_v, wide = planned(false, 28)
    local rail_v, rail = planned(true, 5)
    local carded = 0
    for row = 1, 20 do
      local a, b = wide.hits[row], rail.hits[row]
      eq(b.kind == "tab", a.kind == "tab", height .. ": row " .. row .. " is a card in both modes or neither")
      if a.kind == "tab" and b.kind == "tab" then
        carded = carded + 1
        eq(b.slot, a.slot, height .. ": row " .. row .. " slot")
        eq(b.part, a.part, height .. ": row " .. row .. " part")
        eq(b.x1, 1, "the whole rail row is the card")
        eq(b.x2, rail_v.cols)
      end
    end
    assert(carded >= 7, height .. ": the comparison actually covered the cards")

    local wide_rows = frame_rows(wide_v)
    local rail_rows = frame_rows(rail_v)
    local wide_icon, rail_icon
    for row = 1, 20 do
      if usub(wide_rows[row], wide.grid.icon_x, wide.grid.icon_x) == "v" then
        wide_icon = wide_icon or row
      end
      if usub(rail_rows[row], rail.grid.icon_x, rail.grid.icon_x) == "v" then
        rail_icon = rail_icon or row
      end
    end
    eq(rail_icon, wide_icon, height .. ": the rail icon lands on the expanded card's icon row")
    local slot_top
    for row = 1, 20 do
      if rail.hits[row].kind == "tab" and rail.hits[row].slot == rail.hits[rail_icon].slot then
        slot_top = slot_top or row
      end
    end
    eq(rail_icon - slot_top + 1, layout.icon_row(height == "tall" and 5 or 3), height .. ": middle of the card")
  end
end)

test("P2 rail: the private header and a footer survive having no title column", function()
  local v = p1_view {
    rows = 16,
    cols = 5,
    private = true,
    footer = { { icon = "f", text = "main - 3 dirty", id = "git" } },
    opts = { separator = "gap" },
  }
  v.rail = true
  local rows, r = frame_rows(v)
  for row = 1, v.rows do
    eq(util.width(rows[row]), 5, "rail row " .. row)
  end
  eq(usub(rows[1], 3, 3), v.glyphs.private, "the header keeps its glyph")
  eq(r.hits[v.rows].kind, "footer")
  eq(usub(rows[v.rows], 3, 3), "f", "and the footer keeps its icon")
end)

test("P2 rail: the thumb needs 7 columns", function()
  local many = {}
  for i = 1, 30 do
    many[i] = {
      tab_id = i,
      index = i,
      is_active = i == 1,
      is_pinned = false,
      title = "t" .. i,
      meta = "~/p",
      icon = "t",
      has_unseen = false,
    }
  end
  local narrow = p1_view { rows = 10, cols = 5, items = many }
  narrow.rail = true
  assert(not render.render(narrow).data:find("▐", 1, true), "5 cols has no column to spare")
  local wide = p1_view { rows = 10, cols = 9, items = many }
  wide.rail = true
  assert(render.render(wide).data:find("▐", 1, true), "9 cols draws the thumb")
end)

test("P2 anim: one command per phase, within the backend's bounds", function()
  local frame = render.render(p1_view { rows = 12 })
  local cmd, rows = anim.build("expand_in", frame, { id = 4, anchor = "#1e1e2e" })
  assert(cmd, "built")
  eq(cmd.t, "anim")
  eq(cmd.id, 4)
  eq(cmd.ms, 220)
  eq(cmd.ease, "outCubic")
  eq(cmd.dir, "in")
  eq(cmd.fps, 30)
  eq(cmd.anchor, "#1e1e2e")
  eq(#cmd.rows, #rows, "one entry per selected row")
  eq(cmd.rows[1].delay, 0, "expand staggers top to bottom")
  eq(cmd.rows[2].delay, 12)
  assert(cmd.rows[#cmd.rows].delay <= 120, "capped")
  assert(#cmd.data <= anim.MAX_DATA, "inside the 8 KiB bound")
  assert(cmd.data:find("\27[1;1H", 1, true), "carries its rows with their CUPs")

  local out = anim.build("collapse_out", frame, { id = 5, anchor = "#1e1e2e" })
  eq(out.ms, 160)
  eq(out.ease, "inOutQuad")
  eq(out.dir, "out")
  eq(out.rows[#out.rows].delay, 0, "collapse staggers bottom to top")
  assert(out.rows[1].delay > 0)

  eq(anim.build("hover", frame, { id = 6, anchor = "#1e1e2e", rows = { 3 } }).ms, 60)
  eq(#anim.build("hover", frame, { id = 6, anchor = "#1e1e2e", rows = { 3 } }).rows, 1, "explicit row list wins")
end)

test("P2 anim: refuses what the backend would refuse", function()
  local frame = render.render(p1_view { rows = 12 })
  local _, why = anim.build("nope", frame, { anchor = "#1e1e2e" })
  eq(why, "phase")
  _, why = anim.build("hover", frame, { anchor = "1e1e2e" })
  eq(why, "anchor")
  _, why = anim.build("hover", frame, { anchor = "#1e1e2e", rows = { 999 } })
  eq(why, "empty")
  local wide = { rows = {}, rows_n = 200 }
  for row = 1, 200 do
    wide.rows[row] = "x"
  end
  _, why = anim.build("hover", wide, { anchor = "#1e1e2e" })
  eq(why, "rows")
  local heavy = { rows = { [1] = string.rep("y", anim.MAX_DATA + 1) }, rows_n = 1 }
  _, why = anim.build("hover", heavy, { anchor = "#1e1e2e" })
  eq(why, "size")
end)

test("P3 role: a settings pane is content, never a sidebar", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local settings = fake.pane(tab, { title = "wez-vtabs-settings:ab12" })
  table.insert(tab.pane_list, 2, settings)

  eq(sidebar.is_settings(settings), true)
  eq(sidebar.is_settings(sb), false, "the sidebar's own marker is not a settings marker")
  eq(sidebar.is_backend(settings), false, "rank 0: it is content")
  eq(sidebar.is_ready(settings), false, "and never trusted")

  local sent = #settings.sent
  sidebar.ensure(gui)
  eq(#settings.sent, sent, "never adopted, so never auth'd")
  eq(sidebar.find(tab):pane_id(), sb:pane_id(), "the real sidebar still holds the role")
  eq(state.sidebar_pane_id(tab.id), sb:pane_id(), "the map is untouched")
end)

test("P3 role: a settings pane never closes its tab as an orphan", function()
  local win, gui = setup_window(2)
  attach_all(win, gui)
  local victim = win.tab_list[2]
  mark_ready(victim)
  local settings = fake.pane(victim, { title = "wez-vtabs-settings:ff" })
  victim.pane_list = { sidebar.find(victim), settings }
  victim.active = settings
  sidebar.ensure(gui)
  eq(#win.tab_list, 2, "a settings pane counts as content, so the tab is not orphaned")
end)

test("P3 role: both markers are stripped from the rendered list", function()
  eq(sidebar.marker "wez-vtabs:ab12", true)
  eq(sidebar.marker "wez-vtabs-settings:ab12", true)
  eq(sidebar.marker "wez-vtabs-settings", false)
  eq(sidebar.marker "wez-vtabs-settings:zz", false, "hex only")
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  mark_ready(tab)
  tab:set_title "wez-vtabs-settings:ab12"
  local built = model.build(gui)
  eq(built[1].title:find "wez%-vtabs", nil, "the settings marker never reaches the tab list")
end)

test("P3 role: spawn_args carries a non-default role on every path", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local direct = backend.spawn_args(cfg, "local", nil, "settings")
  eq(direct[1], "/bin/wez-vtabs")
  eq(direct[2], "--role")
  eq(direct[3], "settings")
  eq(#backend.spawn_args(cfg, "local", nil, "sidebar"), 1, "the default role adds nothing")
  eq(#backend.spawn_args(cfg, "local"), 1, "and nil means default")

  local boot = config.setup {}
  local local_boot = backend.spawn_args(boot, "local", nil, "settings")
  eq(local_boot[1], "sh")
  eq(local_boot[#local_boot - 1], "--role")
  eq(local_boot[#local_boot], "settings")

  local remote = backend.spawn_args(boot, "desktop", nil, "settings")
  eq(remote[2], "-c")
  eq(remote[4], "wez-vtabs", "sh -c needs a $0 before the role")
  eq(remote[5], "--role")
  eq(remote[6], "settings")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("P3 lazy attach: expand splits the active tab only, the rest on activation", function()
  local win, gui = setup_window(5)
  win.active_tab_ref = win.tab_list[1]
  local splits = 0
  for _, tab in ipairs(win.tab_list) do
    local pane = tab.pane_list[1]
    local real = pane.split
    pane.split = function(self, args)
      splits = splits + 1
      return real(self, args)
    end
  end

  sidebar.ensure(gui)
  eq(splits, 1, "one split for the active tab, not five")
  eq(sidebars_in(win.tab_list[1]), 1)
  for i = 2, 5 do
    eq(sidebars_in(win.tab_list[i]), 0, "tab " .. i .. " has no sidebar yet")
  end

  sidebar.ensure(gui)
  eq(splits, 1, "idling on the same tab splits nothing more")

  win.active_tab_ref = win.tab_list[3]
  sidebar.ensure(gui)
  eq(splits, 2, "activating tab 3 splits exactly once")
  eq(sidebars_in(win.tab_list[3]), 1)
  eq(sidebars_in(win.tab_list[2]), 0, "its neighbours still wait")
  eq(sidebars_in(win.tab_list[4]), 0)
end)

test("P3 lazy attach: a sidebar-less background tab is not an orphan", function()
  local win, gui = setup_window(3)
  win.active_tab_ref = win.tab_list[1]
  sidebar.ensure(gui)
  sidebar.ensure(gui)
  eq(#win.tab_list, 3, "no tab is closed for lacking a sidebar")
  local listed = model.build(gui)
  eq(#listed, 3, "and every tab is still listed")
end)

test("addendum 5: the bar is gated on hue distance from fg, not on contrast", function()
  local function gutter(title)
    local v = p1_view { opts = { separator = "gap" } }
    v.theme.title_active = title
    local rows = frame_rows(v)
    return usub(rows[4], 3, 3), usub(rows[3], 3, 3)
  end
  local fg = p1_view({}).theme.fg

  local r1, r2 = gutter { 137, 180, 250 }
  eq(r1, " ", "a hue-distinct accent title needs no bar")
  eq(r2, " ", "and a pad row never carries the marker")

  eq(gutter(fg), "▎", "a title that degenerated to fg has no hue left, whatever it scores")
  eq(gutter { fg[1] + 23, fg[2], fg[3] }, "▎", "23 of one channel is still not a difference")
  eq(gutter { fg[1] + 24, fg[2], fg[3] }, " ", "24 is")
  eq(gutter { fg[1] - 26, fg[2] - 26, fg[3] - 26 }, " ", "Nord's 26 keeps its tint")
end)

test("addendum 5: the three degenerate palettes keep the bar, Nord does not", function()
  local palettes = require "palettes"
  local by_name = {}
  for _, p in ipairs(palettes) do
    by_name[p.name] = p
  end
  -- title_active per addendum 5.2; the three that hit the ceiling come back as exactly fg
  local cases = {
    { "Solarized Dark", true },
    { "Solarized Light", true },
    { "One Dark", true },
    { "Nord", false },
  }
  for _, case in ipairs(cases) do
    local p = by_name[case[1]]
    assert(p, case[1] .. " missing from the palette fixture")
    local resolved = theme.resolve({}, p)
    local v = p1_view { opts = { separator = "gap" } }
    v.theme = resolved
    v.theme.title_active = case[2] and resolved.fg or { resolved.fg[1] - 26, resolved.fg[2] - 26, resolved.fg[3] - 26 }
    local rows = frame_rows(v)
    eq(usub(rows[4], 3, 3) == "▎", case[2], case[1] .. " bar")
  end
end)

test("addendum 5: no palette loses both discriminators", function()
  local palettes = require "palettes"
  for _, p in ipairs(palettes) do
    local resolved = theme.resolve({}, p)
    local v = p1_view { opts = { separator = "gap" } }
    v.theme = resolved
    local rows = frame_rows(v)
    local bar = usub(rows[4], 3, 3) == "▎"
    local title = resolved.title_active or resolved.fg
    local delta = 0
    for i = 1, 3 do
      delta = math.max(delta, math.abs(title[i] - resolved.fg[i]))
    end
    assert(bar or delta >= 24, p.name .. " has neither a bar nor a hue-distinct title")
  end
end)

test("addendum 5: the active tab's dot comes from has_unseen, not from being active", function()
  local quiet = p1_items()
  quiet[2].has_unseen = false
  local v = p1_view { items = quiet, opts = { separator = "gap" } }
  v.theme.title_active = { 137, 180, 250 }
  eq(usub(frame_rows(v)[4], 3, 3), " ", "an active tab with nothing unseen keeps a blank gutter")
end)

test("addendum 5: an active tab with unseen output shows the dot once the bar is gone", function()
  local unseen_items = p1_items()
  unseen_items[2].has_unseen = true
  local v = p1_view { items = unseen_items, opts = { separator = "gap" } }
  v.theme.title_active = { 137, 180, 250 }
  local rows = frame_rows(v)
  eq(usub(rows[4], 3, 3), "•", "the freed gutter finally reaches an active tab")
end)

test("addendum 6: tab_height = tall pads the card instead of adding an icon row", function()
  local layout = require "vtabs.layout"
  local v = p1_view { rows = 20, opts = { separator = "gap", tab_height = "tall" } }
  local rows, r = frame_rows(v)
  local first
  for row = 1, v.rows do
    local h = r.hits[row]
    if h and h.kind == "tab" and h.id == 2 then
      first = first or row
    end
  end
  eq(r.hits[first].part, "pad")
  eq(r.hits[first + 1].part, "pad")
  eq(r.hits[first + 2].part, "title", "the title owns the middle row")
  eq(r.hits[first + 3].part, "pad")
  eq(r.hits[first + 4].part, "pad")
  eq(r.hits[first + 2].slot, r.hits[first].slot, "all five rows are one card")
  eq(usub(rows[first + 2], 5, 5), "v", "and the icon rides the title, not a row of its own")
  eq(layout.icon_row(5), 3, "which is where icon_row points")
  for row = 1, v.rows do
    eq(util.width(rows[row]), 28, "tall row " .. row)
  end
end)

test("addendum 2 A3a/A4a: the content block is centred at every tab_height, with and without meta", function()
  local layout = require "vtabs.layout"
  local cases = {
    { height = "row", meta = false, rows = 1, title = 1 },
    { height = "row", meta = "auto", rows = 2, title = 1 },
    { height = "card", meta = false, rows = 3, title = 2 },
    { height = "card", meta = "auto", rows = 4, title = 2 },
    { height = "tall", meta = false, rows = 5, title = 3 },
    { height = "tall", meta = "auto", rows = 6, title = 3 },
  }
  for _, case in ipairs(cases) do
    local v = p1_view { rows = 26, opts = { separator = "gap", tab_height = case.height, meta = case.meta } }
    local painted, r = frame_rows(v)
    local first, count, title
    for row = 1, v.rows do
      local h = r.hits[row]
      if h and h.kind == "tab" and h.id == 2 then
        first = first or row
        count = (count or 0) + 1
        title = h.part == "title" and row or title
      end
    end
    local label = case.height .. " / meta=" .. tostring(case.meta)
    eq(count, case.rows, label .. ": rows per card")
    eq(title - first + 1, case.title, label .. ": the title row is the middle of the block")
    eq(layout.icon_row(case.rows), case.title, label .. ": icon_row agrees")
    eq(usub(painted[title], 5, 5), "v", label .. ": the icon rides the title")
    if case.meta == false then
      for _, item in ipairs(v.items) do
        for row = first, first + count - 1 do
          assert(not painted[row]:find(item.meta, 1, true), label .. ": no cwd, domain or socket path on the card")
        end
      end
    end
  end
end)

test("addendum 2 A3c: show_index rides the title when there is no meta line", function()
  local v = p1_view { rows = 20, opts = { separator = "gap", show_index = true } }
  local rows, r = frame_rows(v)
  local title
  for row = 1, v.rows do
    local h = r.hits[row]
    if h and h.kind == "tab" and h.part == "title" and h.id == 3 then
      title = title or row
    end
  end
  eq(usub(rows[title], 7, 15), "3  claude", "the index is inline with the title")
  local metaed, mr = frame_rows(p1_view { rows = 20, opts = { separator = "gap", show_index = true, meta = "auto" } })
  local meta_row
  for row = 1, 20 do
    local h = mr.hits[row]
    if h and h.kind == "tab" and h.part == "meta" and h.id == 3 then
      meta_row = meta_row or row
    end
  end
  eq(usub(metaed[meta_row - 1], 7, 12), "claude", "with a meta line the title keeps its own row")
  assert(usub(metaed[meta_row], 7, 9):find "3", "and the index goes back to the meta line")
end)

test("addendum 2 A4c: the close glyph is the heavy multiplication x and measures one cell", function()
  local icons_mod = require "vtabs.icons"
  eq(icons_mod.defaults.close, "✖", "not a Nerd Font glyph: those are drawn cell-sized and read thin")
  local resolved = glyphs.resolve(config.setup({ backend = { path = "/bin/wez-vtabs" } }).glyphs, {})
  eq(util.width(resolved.close), 1, "one column, so the ASCII guard never fires")
  eq(resolved.close, "✖")
  local wide = glyphs.resolve(config.setup({ backend = { path = "/bin/wez-vtabs" } }).glyphs, {
    treat_east_asian_ambiguous_width_as_wide = true,
  })
  eq(wide.close, "✖", "U+2716 is Neutral, so ambiguous-as-wide leaves it alone")
  local swapped = glyphs.resolve(
    config.setup({ icon_map = { close = "\u{f0156}" }, backend = { path = "/bin/wez-vtabs" } }).glyphs,
    {}
  )
  eq(swapped.close, "\u{f0156}", "and icon_map still reaches the Nerd Font close for anyone who wants it")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("addendum 4: fit_meta splits on the configured separator", function()
  local sep_items = p1_items()
  sep_items[2].meta = "nvim  ~/work/acme/services/api"
  sep_items[3].meta = "nvim  ~/work/acme/services/web"
  local v = p1_view { rows = 14, items = sep_items, opts = { separator = "gap", meta = "auto" } }
  v.cfg.meta_sep = "  "
  local rows = frame_rows(v)
  assert(usub(rows[5], 6, 25):find("api", 1, true), "two spaces split the composite: " .. usub(rows[5], 6, 25))
  assert(usub(rows[9], 6, 25):find("web", 1, true))
  eq(util.width(rows[5]), 28)
end)

test("addendum 2: frame paints the inner edge and chamfers the page away", function()
  local plain = render.render(p1_view { rows = 20, opts = { separator = "gap" } })
  local v = p1_view { rows = 20, opts = { separator = "gap" } }
  v.cfg.frame = { margin = 1, corners = true, tint = 0.04 }
  v.theme.content_bg = { 0, 0, 0 }
  local rows, r = frame_rows(v)
  for row = 1, v.rows do
    eq(util.width(rows[row]), 28, "framed row " .. row)
  end
  local first, last
  for row = 1, v.rows do
    if r.rows[row] then
      first = first or row
      last = row
    end
  end
  eq(usub(rows[first], 27, 27), "▙", "the page chamfers away from the content at the top")
  eq(usub(rows[last], 27, 27), "▛", "and closes at the bottom")
  assert(r.rows[first]:find(ansi.bg { 0, 0, 0 }, 1, true), "col 28 is painted in the content's colour")

  local card
  for row = 1, v.rows do
    if r.hits[row] and r.hits[row].kind == "tab" and r.hits[row].part == "title" then
      card = card or row
    end
  end
  eq(r.hits[card].x2, 26, "the card grid gives up one column to the frame")
  local plain_card
  for row = 1, 20 do
    if plain.hits[row] and plain.hits[row].kind == "tab" and plain.hits[row].part == "title" then
      plain_card = plain_card or row
    end
  end
  eq(plain.hits[plain_card].x2, 27, "and keeps it when frame is off")
end)

test("addendum 2: frame = false changes nothing", function()
  local off = render.render(p1_view { rows = 20, opts = { separator = "gap" } })
  local explicit = p1_view { rows = 20, opts = { separator = "gap" } }
  explicit.cfg.frame = false
  eq(render.render(explicit).data, off.data, "the default frame is byte-identical to no frame")
end)

test("addendum: the ghost card closes at every width and on both rails", function()
  for _, cols in ipairs { 12, 16, 20, 24, 27, 28, 29, 32, 40, 41 } do
    local v = p1_view { rows = 20, cols = cols, opts = { separator = "gap", width = math.max(cols, 8) } }
    local rows, r = frame_rows(v)
    local top
    for row = 1, v.rows do
      if r.hits[row] and r.hits[row].kind == "new_tab" then
        top = top or row
      end
    end
    assert(top, cols .. " cols has a ghost card")
    local x1, x2 = 3, cols - 1
    for _, row in ipairs { top, top + 2 } do
      local line = rows[row]
      eq(usub(line, x1, x1), row == top and "╭" or "╰", cols .. " cols: corner")
      eq(usub(line, x2, x2), row == top and "╮" or "╯", cols .. " cols: corner")
      assert(usub(line, x1 + 1, x1 + 1) ~= " ", cols .. " cols: gap beside the left corner on row " .. row)
      assert(usub(line, x2 - 1, x2 - 1) ~= " ", cols .. " cols: gap beside the right corner on row " .. row)
    end
    eq(util.width(rows[top]), cols)
  end
  -- the rail grid leaves title_x1 nil, which the ghost label used to index blind
  for _, cols in ipairs { 3, 5, 7, 9, 12 } do
    local v = p1_view { rows = 18, cols = cols, opts = { separator = "gap", width = math.max(cols, 8) } }
    v.rail = true
    local rows, r = frame_rows(v)
    local top
    for row = 1, v.rows do
      if r.hits[row] and r.hits[row].kind == "new_tab" then
        top = top or row
      end
    end
    assert(top, cols .. "-col rail has a ghost")
    local mid = math.ceil(cols / 2)
    eq(usub(rows[top], 1, 1), "╭", cols .. "-col rail: the ghost is the same card")
    eq(usub(rows[top], cols, cols), "╮")
    eq(usub(rows[top + 1], mid, mid), "+", "with the + centred")
    for row = 1, v.rows do
      eq(util.width(rows[row]), cols, cols .. "-col rail row " .. row)
    end
  end
end)

test("layout: hits are computed without painting anything", function()
  local layout = require "vtabs.layout"
  local v = p1_view { rows = 20, hover = { x = 5, y = 6 }, opts = { separator = "gap" } }
  local l = layout.plan(v)
  local painted = render.render(v)
  for row = 1, v.rows do
    local a, b = l.hits[row], painted.hits[row]
    eq(a == nil, b == nil, "row " .. row .. " presence")
    if a then
      eq(a.kind, b.kind, "row " .. row .. " kind")
      eq(a.id, b.id, "row " .. row .. " id")
      eq(a.slot, b.slot, "row " .. row .. " slot")
      eq(a.part, b.part, "row " .. row .. " part")
      eq(a.x1, b.x1)
      eq(a.x2, b.x2)
      eq(hit.span(a, 26), hit.span(b, 26), "row " .. row .. " span")
    end
  end
  eq(l.scroll, painted.scroll)
  eq(l.total, painted.total_rows)
end)

test("layout: the grid, plan and scroll are pure and inspectable", function()
  local layout = require "vtabs.layout"
  local v = p1_view { rows = 20, opts = { separator = "gap" } }
  local l = layout.plan(v)
  eq(l.grid.card_x1, 3)
  eq(l.grid.card_x2, 27)
  eq(l.grid.icon_x, 5)
  eq(l.grid.title_x1, 7)
  eq(l.rail, false)
  eq(l.strip_rows, v.strip.rows)
  eq(l.plan[1].kind, "tab", "the pinned entry leads the plan")
  eq(l.plan[1].part, "title")
  eq(l.rows[l.strip_rows + 1].kind, "card", "and lands on the first list row")

  local rail = p1_view { rows = 16, cols = 5, opts = { separator = "gap" } }
  rail.rail = true
  local rl = layout.plan(rail)
  eq(rl.grid.icon_x, 3)
  eq(rl.grid.close_x, nil, "a rail card has no close column at all")
  eq(layout.has_text(rl.grid), false, "nor any text column, which is the one contract the painters read")
  eq(layout.has_text(l.grid), true)
  eq(rl.rail, true)
end)

test("P1 frames are written for design review", function()
  -- the shared fixture pins row_gap and separator for positional tests; frames want the shipped values
  local design = { row_gap = config.defaults.row_gap, separator = config.defaults.separator }
  local function linux_strip(cfg)
    local geo = require("vtabs.platform").strip_geometry({}, {
      position = cfg.position,
      padding_top = cfg.padding.top,
      toggle_button = cfg.toggle_button,
      card_x1 = cfg.padding.left + 1,
    })
    return {
      rows = geo.rows,
      toggle = cfg.toggle_button and {
        row = geo.toggle_row,
        x = geo.toggle_x,
        x1 = math.max(1, geo.toggle_x - 1),
        x2 = geo.toggle_x + 2,
      } or nil,
    }
  end
  local function dumped(over)
    local v = p1_view(over)
    v.strip = linux_strip(v.cfg)
    return v
  end
  local base = dumped { rows = 20, opts = design }
  local hover_row = base.strip.rows + 6
  dump_frame("tabs", base)
  local pop = dumped { rows = 20, opts = design }
  pop.popover = popover_rect()
  dump_frame("popover-open", pop)
  local tall = dumped { rows = 20, opts = design }
  tall.cfg.tab_height = "tall"
  dump_frame("tall", tall)
  local framed = dumped { rows = 20, opts = design }
  framed.cfg.frame = { margin = 1, corners = true, tint = 0.04 }
  framed.theme.content_bg = framed.theme.bg
  dump_frame("frame", framed)
  local unseen = p1_items()
  unseen[2].has_unseen = true
  local active_unseen = dumped { rows = 20, items = unseen, opts = design }
  active_unseen.theme.title_active = { 137, 180, 250 }
  dump_frame("active-unseen", active_unseen)
  for _, cols in ipairs { 5, 9 } do
    local rail = p1_view { rows = 16, cols = cols, opts = design }
    rail.rail = true
    rail.strip = { rows = 2, toggle = { row = 1, x = math.ceil(cols / 2), x1 = 1, x2 = cols } }
    dump_frame("rail-" .. cols, rail)
  end
  dump_frame("hover", dumped { rows = 20, hover = { x = 5, y = hover_row }, opts = design })
  -- identical to hover.txt on purpose: they differ only in close_hover_fg, which stripping removes
  dump_frame("hover-close", dumped { rows = 20, hover = { x = 26, y = hover_row }, opts = design })
  -- identical to the ghost in tabs.txt on purpose: the hover moves colour only, and stripping removes it
  dump_frame("new-tab-hover", dumped { rows = 20, hover = { x = 5, y = 19 }, opts = design })
  dump_frame("drag", dumped { rows = 20, drag = { tab_id = 3, over_index = 1, active = true }, opts = design })
  dump_frame("private", dumped { rows = 20, private = true, opts = design })
  dump_frame(
    "strip-macos",
    p1_view {
      rows = 20,
      strip = { rows = 4, cols = 9, toggle_row = 1 },
      opts = design,
    }
  )
  local many = {}
  for i = 1, 12 do
    many[i] = {
      tab_id = i,
      index = i,
      is_active = i == 4,
      is_pinned = i == 1,
      title = "tab number " .. i,
      meta = "~/projects/repo" .. i,
      icon = "t",
      has_unseen = i == 6,
    }
  end
  dump_frame(
    "overflow",
    dumped { items = many, rows = 16, scroll = 4, footer = { { icon = "⚑", text = "main · 3 dirty" } }, opts = design }
  )
  if FRAME_DIR and FRAME_DIR ~= "" then
    local f = io.open(FRAME_DIR .. "/collapsed.txt", "w")
    if f then
      f:write "collapsed = today's detach: the sidebar pane is closed, so no frame is rendered.\n"
      f:close()
    end
    local probe = io.open(FRAME_DIR .. "/tabs.txt")
    assert(probe, "frames written")
    probe:close()
  end
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
  local win, gui = setup_window(1)
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

-- ===================== implementer-2: interaction / geometry / focus =====================

local function rgb(c)
  return table.concat(c, ",")
end

test("the page is tinted by default and elevation = 0 makes it seamless", function()
  local t = theme.resolve({}, fake.palette)
  local seamless = theme.resolve({ elevation = 0 }, fake.palette)
  eq(rgb(seamless.bg), "30,30,46", "0 is exactly the terminal background")
  assert(t.bg[1] > seamless.bg[1] and t.bg[3] > seamless.bg[3], "the default tint lifts it toward fg")
  eq(rgb(t.bg), rgb(theme.resolve({ elevation = 0.06 }, fake.palette).bg), "and it is 0.06")
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

test("a divider drag survives a config reload, unless the reload changed width itself", function()
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
  -- Every edit to wezterm.lua reloads, and the plugin watches its own files too, so a reload that
  -- says nothing about the width must not throw the drag away.
  geometry.reset(gui:window_id())
  eq(geometry.desired(gui:window_id()), 34, "an unrelated reload keeps it")
  eq(geometry.correct(gui), false, "and nothing is re-asserted")
  config.setup { width = 30, backend = { path = "/bin/wez-vtabs" } }
  geometry.reset(gui:window_id())
  eq(geometry.desired(gui:window_id()), 30, "changing width itself drops it")
  assert(geometry.correct(gui))
  eq(sb.cols, 30)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("an adjust the mux has not applied yet is issued once, and its landing is never adopted", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local wid = gui:window_id()
  mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline recorded")
  local issued = 0
  -- A remote mux acknowledges the adjust and applies it a poll or more later.
  gui.perform_action = function(_, action)
    if action.action == "AdjustPaneSize" then
      issued = issued + 1
    end
  end
  win:resize(30)
  assert(geometry.correct(gui), "the window resize is corrected")
  for _ = 1, 4 do
    eq(geometry.correct(gui), false, "and not re-issued while the mux has not applied it")
  end
  eq(issued, 1, "one AdjustPaneSize, not one per poll; the duplicates all land and overshoot")

  -- The width it eventually lands on is ours, so it must never read as a divider drag.

  tab:set_split(24)
  eq(geometry.correct(gui), false)
  eq(geometry.desired(wid), 28, "the landing is not adopted")

  -- The sidebar reporting its own size is proof it landed, so the next target goes out at once.
  geometry.landed(wid)
  gui.perform_action = nil
  assert(geometry.correct(gui), "and the retry is not blocked once it has")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a window drag corrects on the first frame of the burst and leaves the rest to the poll", function()
  local win, gui = setup_window(1)
  local wid = gui:window_id()
  geometry.forget_window(wid)
  assert(geometry.on_resize(wid), "the first frame is corrected")
  for _ = 1, 10 do
    eq(geometry.on_resize(wid), false, "every frame after it costs nothing")
  end
  eq(#win.actions, 0, "so a drag issues no adjust per frame at all")
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
  attach_all(win, gui)
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
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  win.active_tab_ref = win.tab_list[1]
  view_mod.sync(gui, { force = true })
  return win, gui
end

---Row a tab's card starts on, read back from the hit map instead of assumed.
local function title_row(sb, tab_id)
  for row, h in pairs(state.session.hits[sb:pane_id()] or {}) do
    if h.kind == "tab" and h.id == tab_id and h.part == "title" then
      return row
    end
  end
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
  local drag = press_row(gui, sb1, title_row(sb1, win.tab_list[3].id))
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
  local from = title_row(sb1, ids[3])
  local onto = title_row(sb1, ids[1])
  press_row(gui, sb1, from)
  local sb3 = sidebar.find(win.tab_list[3])
  mouse(gui, sb3, "drag", "left", 5, from - 1)
  eq(state.session.drag[gui:window_id()].active, false, "one row is jitter")
  mouse(gui, sb3, "up", "left", 5, from - 1)
  eq(win.tab_list[3].id, ids[3], "order untouched")

  press_row(gui, sb1, from)
  mouse(gui, sb3, "drag", "left", 5, onto)
  assert(state.session.drag[gui:window_id()].active, "three rows arms the drag")
  mouse(gui, sb3, "up", "left", 5, onto)
  eq(win.tab_list[1].id, ids[3], "dragged tab took the first slot")
end)

test("a drag that starts before the dwell elapses is jitter", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  press_row(gui, sb1, title_row(sb1, win.tab_list[3].id), "hold")
  local sb3 = sidebar.find(win.tab_list[3])
  mouse(gui, sb3, "drag", "left", 5, title_row(sb1, win.tab_list[1].id))
  eq(state.session.drag[gui:window_id()].active, false)
end)

test("drag events from a pane other than the drag origin are dropped", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  press_row(gui, sb1, title_row(sb1, win.tab_list[3].id))
  local sb2 = sidebar.find(win.tab_list[2])
  mouse(gui, sb2, "drag", "left", 5, title_row(sb1, win.tab_list[1].id))
  eq(state.session.drag[gui:window_id()].active, false)
end)

test("a drag whose pane has no hit map is dropped instead of dropping at slot 1", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local drag = press_row(gui, sb1, 3)
  state.session.hits[sb1:pane_id()] = nil
  mouse(gui, sb1, "drag", "left", 5, 6)
  eq(drag.active, false)
  eq(drag.over_index, nil)
end)

test("right click opens the popover on release, never while the button is held", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local before = #win.actions
  mouse(gui, sb1, "down", "right", 5, 4)
  eq(popover.get(gui:window_id()), nil, "nothing opens under a held button")
  eq(#win.actions, before, "and no overlay action is performed, ever")
  mouse(gui, sb1, "up", "right", 5, 4)
  local pop = popover.get(gui:window_id())
  assert(pop, "the release opens it")
  eq(pop.tab_id, win.tab_list[1].id)
  eq(pop.anchor_row, 4, "anchored on the row the press landed on")
  eq(#win.actions, before, "still no overlay: it is drawn inside the sidebar")
  popover.close(gui)
end)

test("hover=press restores content focus on release, hover=follow keeps the sidebar", function()
  local win, gui = drag_setup()
  local tab = win.tab_list[1]
  local sb1 = sidebar.find(tab)
  press_row(gui, sb1, 3)
  eq(tab.active, sb1)
  mouse(gui, sb1, "up", "left", 5, 3)
  eq(tab.active, sb1, "follow leaves the sidebar active")
  config.setup { hover = "press", backend = { path = "/bin/wez-vtabs" } }
  press_row(gui, sb1, 3)
  mouse(gui, sb1, "up", "left", 5, 3)
  assert(tab.active ~= sb1, "press mode hands focus back")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("mouse move repaints on a row change and stays quiet inside the row", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local sent = #sb1.sent
  -- an inactive card, so hovering it actually changes the frame
  local row = title_row(sb1, win.tab_list[2].id)
  mouse(gui, sb1, "move", "none", 5, row)
  local repainted = #sb1.sent
  assert(repainted > sent, "crossing into a row repaints")
  mouse(gui, sb1, "move", "none", 6, row)
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
  local real_now = util.now_ms
  local frozen = real_now()
  util.now_ms = function()
    return frozen
  end
  for _ = 1, 25 do
    input.handle(gui, sb, "vtabs", '{"t":"key","key":"a","raw":"YQ=="}')
  end
  util.now_ms = real_now
  eq(#content.sent, 20, "20 keys of burst, nothing refills within the same instant")
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
  -- PaneInformation carries only ids, so window_title resolves them through the mux, like wezterm.
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local mux_tab = win.tab_list[1]
  mark_ready(mux_tab)
  local content = sidebar.content_pane(mux_tab)
  content.title = "nvim"
  local sb = { pane_id = sidebar.find(mux_tab):pane_id() }
  local shell = { pane_id = content:pane_id(), title = "nvim" }
  local tab = { tab_id = mux_tab:tab_id(), tab_index = 1, tab_title = "" }
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

  eq(geometry.correct(gui), false, "baseline on the settled window")
  tab:set_split(34)
  eq(geometry.correct(gui), false, "same tab width, same pixels: this one is a drag")
  eq(geometry.desired(gui:window_id()), 34)
end)

test("a paste is charged by its size, so a second large one waits for the budget", function()
  local _, gui, tab, sb, content = key_setup()
  local big = string.rep("eHh4", 21845) .. "eA=="
  -- the bucket refills from the clock, so both pastes must land in the same tick
  local real_now = util.now_ms
  local frozen = real_now()
  util.now_ms = function()
    return frozen
  end
  input.handle(gui, sb, "vtabs", string.format('{"t":"paste","data":"%s"}', big))
  eq(#content.pasted, 1, "the first 64 KiB paste goes through")
  eq(#content.pasted[1], 64 * 1024)
  input.handle(gui, sb, "vtabs", string.format('{"t":"paste","data":"%s"}', big))
  eq(#content.pasted, 1, "the second is over budget")
  util.now_ms = real_now
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
    -- The drag chip paints its meta row in drag_fg, so drag_bg gates that, not meta_fg (r3.1 #7).
    local drag_ceiling = c(t.fg, t.drag_bg)
    assert(c(t.drag_fg, t.drag_bg) >= math.min(3.5, drag_ceiling) - 0.001, "drag_fg vs drag_bg" .. where)
    assert(c(t.close_fg, t.active_bg) >= 3.0 - 0.001, "close_fg vs active_bg" .. where)
    assert(c(t.close_hover_fg, t.active_bg) >= 3.0 - 0.001, "close_hover_fg vs active_bg" .. where)
    assert(c(t.border, t.bg) >= 2.5 - 0.001, "border vs bg" .. where)
    assert(c(t.border_idle, t.bg) >= 2.0 - 0.001, "border_idle vs bg" .. where)
    assert(c(t.ghost_border_hover, t.bg) >= 2.8 - 0.001, "ghost_border_hover vs bg" .. where)
    assert(c(t.ghost_border_hover, t.border_idle) >= 1.3 - 0.001, "the ghost hover is a visible step" .. where)
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

test("a private window renders its header, through the same path the plugin uses", function()
  local win, gui = drag_setup()
  local wid = gui:window_id()
  local seen = nil
  local render_mod = require "vtabs.render"
  local original = render_mod.render
  render_mod.render = function(frame)
    seen = frame
    return original(frame)
  end
  state.set_private(wid, true)
  view_mod.invalidate_theme()
  view_mod.sync(gui, { force = true })
  render_mod.render = original
  state.set_private(wid, false)
  view_mod.invalidate_theme()
  eq(seen.private, true, "view.sync tells the renderer the window is private")
  local sb = sidebar.find(win.tab_list[1])
  local hits = state.session.hits[sb:pane_id()]
  local header = nil
  for row = 1, 12 do
    if hits[row] and hits[row].kind == "space" and row > 1 then
      header = header or row
    end
  end
  assert(header, "the header row is inert")
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
  -- Windows: no "/" to split on, so the old version right-cut and ate the basename.
  eq(sp("C:\\Users\\fredrir\\projects\\app", 20), "C:\\U\\f\\projects\\app")
  eq(sp("C:\\Users\\fredrir\\projects\\vertical-tabs", 20), "…\\vertical-tabs", "left-cut keeps the basename")
  eq(sp("C:\\Users\\x\\app", 20), "C:\\Users\\x\\app", "a path that fits is untouched")
  assert(
    sp("C:\\Users\\x\\projects\\api", 16) ~= sp("C:\\Users\\x\\projects\\web", 16),
    "windows siblings stay distinguishable"
  )
  for _, budget in ipairs { 4, 8, 12, 20, 40 } do
    local out = sp("C:\\Users\\fredrir\\projects\\wezterm-vertical-tabs\\plugin", budget)
    assert(util.width(out) <= budget, "windows budget " .. budget .. " overflowed with " .. out)
  end
  for _, budget in ipairs { 4, 8, 12, 20, 40 } do
    local out = sp("~/projects/wezterm-vertical-tabs/plugin/vtabs", budget)
    assert(util.width(out) <= budget, "budget " .. budget .. " overflowed with " .. out)
  end
end)

test("the P1 defaults and their aliases pass validation without warning", function()
  local before = #wezterm.log
  local cfg = config.setup {}
  eq(cfg.padding.top, 1)
  eq(cfg.row_gap, 0)
  eq(cfg.tab_height, "card")
  eq(cfg.meta, false)
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
    tab_height = "gigantic",
    meta = "path",
    new_tab_button = "button",
    corners = "round",
    scroll_indicator = "sometimes",
    titlebar = "native",
    pinned_style = "tiny",
  } do
    eq(config.setup({ [key] = bad })[key], config.defaults[key], key .. " reset")
  end
  eq(config.setup({ row_gap = -1 }).row_gap, 0)
  eq(config.setup({ row_gap = "two" }).row_gap, 0)
  eq(config.setup({ toggle_button = "yes" }).toggle_button, true)
  eq(config.setup({ row_gap = 3 }).row_gap, 3, "a valid value survives")
end)

test("tab_height and meta are independent, and press mode forces an always-on close button", function()
  eq(config.setup({ tab_height = "row" }).meta, false, "the height decides the pads, not the lines")
  eq(config.setup({ tab_height = "tall" }).meta, false)
  eq(config.setup({ meta = "cwd" }).tab_height, "card", "and the meta line does not rewrite the height")
  eq(config.setup({ meta = false }).tab_height, "card")
  eq(config.setup({ meta = "cwd", tab_height = "tall" }).tab_height, "tall")
  eq(config.setup({ meta = "cwd", tab_height = "tall" }).meta, "cwd")
  eq(config.setup({ hover = "press" }).close_button, "always")
  eq(config.setup({ hover = "press", close_button = "never" }).close_button, "never")
  eq(config.setup({ hover = "follow" }).close_button, "hover")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

local platform = require "vtabs.platform"

-- 8.4 pt cells across, 19 pt down, so 70/8.4 -> 9 cols and 28/19 -> 2 rows. No `dpi`, so 1x.
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

test("the reserve is ceil(70pt / cell width): 9 cols at 8pt, 8 at 9pt, 7 at 10-11pt, 6 at 12pt", function()
  local want = { [8] = 9, [9] = 8, [10] = 7, [11] = 7, [12] = 6 }
  for cell, cols in pairs(want) do
    local g = strip_geom { cols = 28, viewport_rows = 30, pixel_width = 28 * cell, pixel_height = 570 }
    eq(g.cols, cols, cell .. " pt cells")
    eq(g.cols, math.ceil(70 / cell), "and it is the formula, not a table")
    assert(g.cols <= 9, "9 is the widest reserve a readable cell can produce, never 11")
    eq(g.toggle_x, g.cols + 2, "11 is the toggle column at a 9-column reserve, not the reserve")
  end
end)

test("a 2x display doubles the pixels and keeps the points, so the reserve does not move", function()
  local one_x = strip_geom(RETINA)
  local two_x = strip_geom {
    cols = RETINA.cols,
    viewport_rows = RETINA.viewport_rows,
    pixel_width = RETINA.pixel_width * 2,
    pixel_height = RETINA.pixel_height * 2,
    dpi = platform.POINT_DPI * 2,
  }
  eq(two_x.cols, one_x.cols, "the lights are 70 points wide on both")
  eq(two_x.rows_reserved, one_x.rows_reserved)
  eq(two_x.toggle_row, one_x.toggle_row)
  eq(two_x.toggle_x, one_x.toggle_x)
  eq(two_x.cols, 9)
  -- Device pixels alone would halve it, which is the bug this pins.
  eq(math.ceil(70 / (RETINA.pixel_width * 2 / RETINA.cols)), 5)
end)

test("the macOS strip reserves the traffic lights from the pane's own cell size", function()
  local g = strip_geom(RETINA)
  eq(g.cols, math.ceil(70 / (235 / 28)), "70 px of buttons, never a hardcoded column count")
  eq(g.cols, 9)
  eq(g.rows, 3, "max(reserve 2, toggle 1) + padding_top 1")
  -- The reserve is a row COUNT; the toggle must line up with the lights' centre, not sit below it.
  eq(g.toggle_row, 1, "beside the lights at a retina cell height")
  eq(g.toggle_x, 11, "clear of the reserve")
  local small = strip_geom { cols = 28, viewport_rows = 30, pixel_width = 235, pixel_height = 270 }
  eq(small.rows_reserved, 4, "a small font reserves more rows")
  eq(small.toggle_row, 2, "and the centre moves down with them, never past the reserve")
  assert(small.toggle_row <= small.rows_reserved, "always inside the reserve")
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
  eq(strip_geom(RETINA, { is_mac = false, padding_top = 0 }).rows, 1, "no padding, just the toggle row")
  eq(strip_geom(RETINA, { is_mac = false, toggle_button = false, padding_top = 0 }).rows, 0)
  eq(strip_geom(RETINA, linux).toggle_row, 1)
  eq(strip_geom(RETINA, linux).toggle_x, 2, "card_x1")
  eq(strip_geom(RETINA, { is_mac = false, card_x1 = 4 }).toggle_x, 4)
end)

test("the shipped padding gives the toggle a two-row hit span outside macOS", function()
  local g = strip_geom(RETINA, { is_mac = false, padding_top = config.defaults.padding.top })
  eq(g.rows, 2, "toggle row plus the shipped padding.top")
  eq(g.toggle_row, 1)
  eq(math.min(g.toggle_row + 1, g.rows), 2, "the span covers both strip rows")
  assert(g.toggle_row + 1 <= g.rows, "P1-spec §9: toggle_row + 1 <= rows")
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

test("the window padding is zeroed on the sides the sidebar touches, and never when you set it", function()
  local vtabs = dofile(here .. "/../init.lua")
  local function padding(opts, preset)
    local cfg = { keys = {}, window_padding = preset }
    vtabs.apply_to_config(cfg, opts)
    return cfg.window_padding
  end
  local left = padding {}
  eq(left.left, 0, "the sidebar's own edge, so its background reaches it")
  eq(left.top, 0)
  eq(left.bottom, 0)
  eq(left.right, "1cell", "the far side keeps wezterm's own default")
  local right = padding { position = "right" }
  eq(right.right, 0, "mirrored for a right sidebar")
  eq(right.left, "1cell")
  eq(right.top, 0)
  eq(right.bottom, 0)
  local mine = { left = 8, right = 8, top = 8, bottom = 8 }
  eq(padding({}, mine), mine, "a user value is never overwritten")
  eq(padding { edge_to_edge = false }, nil, "and the opt-out never touches it")
  eq(config.defaults.padding.left, 2, "the air the window padding no longer gives, in the page colour")
  eq(config.setup({ position = "right" }).padding.right, 2, "and it mirrors to the side that touches")
  eq(config.setup({ position = "right" }).padding.left, 1)
  eq(config.setup({ position = "right", padding = { right = 4 } }).padding.right, 4, "yours wins")
  eq(config.setup({ position = "right", padding = { right = 4 } }).padding.left, 2, "untouched, unmirrored")
  eq(config.setup({ position = "right", padding = 3 }).padding.right, 2, "a padding that is not a table")
  eq(config.setup({ position = "right", padding = { left = -5 } }).padding.left, 1, "and one out of range")
  eq(config.setup({ position = "right", padding = { left = -5 } }).padding.right, 2, "both come back mirrored")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

local model_mod = require "vtabs.model"

local function meta_of(pane_opts, over)
  local base = { backend = { path = "/bin/wez-vtabs" }, meta = "auto", tab_height = "card" }
  config.setup(legacy(util.merge(base, over or {})))
  local win = fake.window()
  local tab = win:add_tab { title = "t" }
  local pane = tab.pane_list[1]
  for k, v in pairs(pane_opts) do
    pane[k] = v
  end
  model_mod.forget_tab(tab:tab_id())
  return model_mod.build(win.gui)[1].meta
end

test("the meta line names the cwd for shells and the process for anything else", function()
  local home = wezterm.home_dir
  eq(meta_of { process = "/bin/zsh", cwd = { file_path = "/tmp/work" } }, "~/work", "home_dir collapses to ~")
  eq(meta_of { process = "/usr/bin/fish", cwd = { file_path = "/etc" } }, "/etc")
  eq(meta_of { process = "/usr/bin/nvim", cwd = { file_path = "/tmp/work" } }, "nvim  work", "no separator glyph")
  eq(meta_of { process = "/usr/bin/cargo", cwd = { file_path = "/srv/api" } }, "cargo  api")
  if home and home ~= "" then
    eq(meta_of { process = "/bin/bash", cwd = { file_path = home .. "/projects/api" } }, "~/projects/api")
    eq(meta_of { process = "/bin/bash", cwd = { file_path = home } }, "~")
  end
end)

test("ssh names the remote user only when the pane reports one, never the local $USER", function()
  local ssh = meta_of { process = "/usr/bin/ssh", cwd = { file_path = "/home/x", host = "archie" } }
  eq(ssh, "archie", "no authority in the cwd means host alone")
  local named = meta_of {
    process = "/usr/bin/ssh",
    cwd = { file_path = "/home/admin", host = "buildbox", username = "admin" },
  }
  eq(named, "admin@buildbox", "the URL authority is the only source")
  local url = meta_of { process = "/usr/bin/ssh", cwd = "file://admin@buildbox/home/admin" }
  eq(url, "admin@buildbox", "and it is parsed out of the string form too")
  eq(meta_of { process = "/usr/bin/ssh", cwd = false }, "ssh", "nothing resolvable falls back to the process")
  -- get_foreground_process_name is nil off the local domain, so the domain carries the line.
  eq(meta_of { domain = "SSH:archie", cwd = { file_path = "/home/x/api" } }, "SSH:archie  /home/x/api")
  eq(meta_of { domain = "local", process = nil, cwd = { file_path = "/srv" } }, "/srv")
end)

test("meta = cwd, process and false force one column or none", function()
  local pane = { process = "/usr/bin/nvim", cwd = { file_path = "/tmp/work" } }
  eq(meta_of(pane, { meta = "cwd" }), "~/work")
  eq(meta_of(pane, { meta = "process" }), "nvim")
  eq(meta_of(pane, { meta_sep = " · " }), "nvim · work", "the separator is configurable")
  eq(meta_of(pane, { meta = false }), nil)
end)

test("a pane that resolves nothing leaves meta nil rather than an empty row", function()
  eq(meta_of { process = nil, domain = "local", cwd = false }, nil)
end)

test("meta is resolved at most once per poll_ms per tab", function()
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" }, poll_ms = 60000, meta = "auto", tab_height = "card" })
  local win = fake.window()
  local tab = win:add_tab { title = "t" }
  local pane = tab.pane_list[1]
  pane.process, pane.cwd = "/bin/zsh", { file_path = "/tmp/first" }
  model_mod.forget_tab(tab:tab_id())
  local calls = 0
  local original = getmetatable(pane).get_current_working_dir
  getmetatable(pane).get_current_working_dir = function(self)
    calls = calls + 1
    return original(self)
  end
  eq(model_mod.build(win.gui)[1].meta, "~/first")
  pane.cwd = { file_path = "/tmp/second" }
  for _ = 1, 5 do
    model_mod.build(win.gui)
  end
  getmetatable(pane).get_current_working_dir = original
  eq(calls, 1, "five more builds inside one poll cost nothing")
  eq(model_mod.build(win.gui)[1].meta, "~/first", "the cached value is what renders")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("view hands the renderer a strip and a private-aware theme", function()
  config.setup(
    legacy { backend = { path = "/bin/wez-vtabs" }, toggle_button = true, padding = { top = 1, left = 1, right = 1 } }
  )
  local win = fake.window()
  win:add_tab { title = "t1" }
  local gui = win.gui
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  local seen = nil
  local render_mod = require "vtabs.render"
  local original = render_mod.render
  render_mod.render = function(frame)
    seen = frame
    return original(frame)
  end
  view_mod.invalidate_theme()
  view_mod.sync(gui, { force = true })
  assert(seen, "the renderer was called")
  eq(type(seen.strip), "table")
  eq(seen.strip.rows, 2, "no macOS reserve in the fake, so toggle row plus padding")
  eq(seen.strip.cols, 0)
  eq(seen.strip.toggle.row, 1)
  eq(seen.strip.toggle.x1, 1, "the span reaches one column left of the glyph")
  eq(seen.strip.toggle.x2, 4, "four columns wide")
  eq(type(seen.user_scrolled), "boolean")
  eq(rgb(seen.theme.content_bg), "30,30,46", "the fixture palette reaches the renderer")
  assert(rgb(seen.theme.bg) ~= "30,30,46", "and the page carries the default tint")

  state.set_private(gui:window_id(), true)
  view_mod.invalidate_theme()
  view_mod.sync(gui, { force = true })
  render_mod.render = original
  eq(rgb(seen.theme.accent), rgb(seen.theme.private_accent), "a private window recolours")
  state.set_private(gui:window_id(), false)
  view_mod.invalidate_theme()
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test('rail_titlebar = "widen" widens the rail to the reserve and keeps its toggle inside the pane', function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  local wid = gui:window_id()
  sb.get_dimensions = function(self)
    return { cols = self.cols, viewport_rows = 30, pixel_width = self.cols * 10, pixel_height = 570 }
  end
  local was_mac = platform.is_mac
  platform.is_mac = false
  local seen = nil
  local render_mod = require "vtabs.render"
  local original = render_mod.render
  render_mod.render = function(frame)
    seen = frame
    return original(frame)
  end
  local function railed(opts)
    config.setup(util.merge({ meta = "auto", titlebar = "macos", backend = { path = "/bin/wez-vtabs" } }, opts))
    view_mod.invalidate_theme()
    view_mod.sync(gui, { force = true })
    return seen.strip
  end

  sidebar.set_collapsed(gui, true)
  local geom = railed {}
  eq(geom.cols, 7, "the lights need 7 columns at a 10 pt cell")
  eq(geometry.desired(wid), 7, "so the rail is widened to them, not left at rail_width 5")
  assert(geometry.correct(gui), "and corrected to it")
  eq(sb.cols, 7)

  geom = railed {}
  assert(geom.toggle.x2 <= sb.cols, "the toggle span ends inside the rail, at " .. geom.toggle.x2)
  local toggle_row = nil
  for row, h in pairs(state.session.hits[sb:pane_id()]) do
    if h.kind == "action" or h.kind == "toggle" then
      toggle_row = row
      for _, span in ipairs(h.spans or { h }) do
        assert(span.x2 <= sb.cols, "and so does its hit record, at " .. tostring(span.x2))
        assert(span.x1 >= 1, "which is what makes it clickable at all")
      end
    end
  end
  assert(toggle_row, "the rail still records the strip's own target")

  railed { rail_titlebar = "none" }
  eq(geometry.desired(wid), 5, "opting out leaves rail_width alone")
  railed { rail_titlebar = "band" }
  eq(geometry.desired(wid), 5, "and so does banding instead")

  render_mod.render = original
  platform.is_mac = was_mac
  sidebar.set_collapsed(gui, false)
  view_mod.invalidate_theme()
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test('titlebar = "macos" previews the light reserve on a machine that has none', function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  -- The fake reports no pixel height, and the reserve is derived from the cell box.
  sb.get_dimensions = function(self)
    return { cols = self.cols, viewport_rows = 30, pixel_width = self.cols * 10, pixel_height = 570 }
  end
  local was_mac = platform.is_mac
  platform.is_mac = false
  local seen = nil
  local render_mod = require "vtabs.render"
  local original = render_mod.render
  render_mod.render = function(frame)
    seen = frame
    return original(frame)
  end
  local function strip_of(opts)
    config.setup(util.merge({ meta = "auto", backend = { path = "/bin/wez-vtabs" } }, opts))
    view_mod.invalidate_theme()
    view_mod.sync(gui, { force = true })
    return seen.strip
  end
  eq(strip_of({}).cols, 0, "off macOS there is nothing to reserve")
  local preview = strip_of { titlebar = "macos" }
  eq(preview.cols, 7, "70 px of buttons over 10 px cells")
  eq(preview.rows, 3, "two reserved rows plus padding.top")
  eq(strip_of({ titlebar = "macos", position = "right" }).cols, 0, "the lights are on the left only")
  render_mod.render = original
  platform.is_mac = was_mac
  view_mod.invalidate_theme()
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("every row of a card activates its tab, pads included, but only inside the card surface", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local third = win.tab_list[3]
  local title = title_row(sb1, third.id)
  assert(title, "the third card has a title row")
  for _, row in ipairs { title - 1, title, title + 1, title + 2 } do
    win.active_tab_ref = win.tab_list[1]
    mouse(gui, sb1, "down", "left", 5, row)
    eq(win.active_tab_ref, third, "row " .. row .. " belongs to the third card")
    mouse(gui, sb1, "up", "left", 5, row)
  end
  local wid = gui:window_id()
  for _, col in ipairs { 1, 28 } do
    win.active_tab_ref = win.tab_list[1]
    state.session.last_click[wid] = nil
    mouse(gui, sb1, "down", "left", col, title)
    eq(win.active_tab_ref, win.tab_list[1], "col " .. col .. " carries no card surface")
  end
end)

test("the close span closes and the toggle span collapses the sidebar", function()
  local win, gui = drag_setup()
  local sb1 = sidebar.find(win.tab_list[1])
  local hits = state.session.hits[sb1:pane_id()]
  local first = title_row(sb1, win.tab_list[1].id)
  eq(hit.span(hits[first], 25), "close")
  eq(hit.span(hits[first], 27), "close")
  eq(hit.span(hits[first], 24), nil)
  eq(hit.span(hits[first + 1], 26), "close", "the meta row carries the same span")
  eq(hit.span(hits[first - 1], 26), nil, "the pad row does not")
  eq(hits[1].kind, "action")

  eq(#win.tab_list, 3)
  mouse(gui, sb1, "down", "left", 26, first)
  eq(#win.tab_list, 3, "the ✕ arms on the press")
  mouse(gui, sb1, "up", "left", 26, first)
  eq(#win.tab_list, 2, "and closes the card's tab on the release")

  mouse(gui, sb1, "down", "left", 26, first)
  mouse(gui, sb1, "up", "left", 5, 3)
  eq(#win.tab_list, 2, "a release that slid off the ✕ closes nothing")

  assert(not state.is_collapsed(gui:window_id()))
  mouse(gui, sb1, "down", "left", 2, 1)
  assert(state.is_collapsed(gui:window_id()), "the toggle hides the sidebar")
  sidebar.set_collapsed(gui, false)
end)

test("an armed ✕ closes only where it was pressed, and any drag cancels it", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  local a, b = win.tab_list[1], win.tab_list[3]
  -- Only the active or hovered card offers a ✕, so hover the second one to arm its span.
  mouse(gui, sb, "move", "none", 26, title_row(sb, b.id))
  local row_a, row_b = title_row(sb, a.id), title_row(sb, b.id)
  eq(hit.span(state.session.hits[sb:pane_id()][row_b], 26), "close", "both cards show one")
  local before = #win.tab_list

  mouse(gui, sb, "down", "left", 26, row_a)
  mouse(gui, sb, "up", "left", 26, row_b)
  eq(#win.tab_list, before, "a release over another card's ✕ closes neither of them")

  mouse(gui, sb, "down", "left", 26, row_a)
  mouse(gui, sb, "up", "left", 26, row_a)
  eq(#win.tab_list, before - 1, "the same ✕ pressed and released closes exactly one tab")

  -- WezTerm drops the pointer capture on release, so a release outside still arrives here with
  -- translated coordinates; motion is the only signal that the gesture stopped being a click.
  local c = win.tab_list[#win.tab_list]
  mouse(gui, sb, "move", "none", 26, title_row(sb, c.id))
  local row_c = title_row(sb, c.id)
  local kept = #win.tab_list
  mouse(gui, sb, "down", "left", 26, row_c)
  mouse(gui, sb, "drag", "left", 26, row_c)
  mouse(gui, sb, "up", "left", 26, row_c)
  eq(#win.tab_list, kept, "a flick between press and release cancels the close")
end)

test("a click in a pinned entry's pin span toggles the pin instead of activating", function()
  local win, gui = drag_setup()
  local first = win.tab_list[1]
  state.set_pinned(first.id, true)
  view_mod.sync(gui, { force = true })
  local sb = sidebar.find(win.tab_list[2])
  win.active_tab_ref = win.tab_list[2]
  local row
  for y = 1, 12 do
    local h = state.session.hits[sb:pane_id()][y]
    if h and h.id == first.id then
      row = y
      break
    end
  end
  assert(row, "the dense pinned entry has a row")
  -- The pin glyph replaces the close button on hover, so hover it before asking for the span.
  mouse(gui, sb, "move", "none", 26, row)
  eq(hit.span(state.session.hits[sb:pane_id()][row], 26), "pin")
  mouse(gui, sb, "down", "left", 26, row)
  eq(state.is_pinned(first.id), false, "the pin was toggled")
  eq(win.active_tab_ref, win.tab_list[2], "and the tab was not activated")
end)

test("a drag onto the neighbouring card reorders, at every card height", function()
  local layout = require "vtabs.layout"
  for _, shape in ipairs { { "card", false }, { "card", "auto" }, { "row", false }, { "tall", false } } do
    local height, meta = shape[1], shape[2]
    config.setup {
      backend = { path = "/bin/wez-vtabs" },
      tab_height = height,
      meta = meta,
      row_gap = 0,
    }
    local label = height .. "/" .. tostring(meta)
    local win, gui = drag_setup()
    local sb = sidebar.find(win.tab_list[1])
    local first, second = win.tab_list[1].id, win.tab_list[2].id
    local from, onto = title_row(sb, first), title_row(sb, second)
    assert(from and onto, label .. ": both cards are on screen")
    eq(onto - from, layout.slot_rows(config.get()), label .. ": the neighbour is exactly one slot away")

    press_row(gui, sb, from)
    mouse(gui, sb, "drag", "left", 5, onto)
    local drag = state.session.drag[gui:window_id()]
    assert(drag and drag.active, label .. ": one slot of travel starts the drag")
    mouse(gui, sb, "up", "left", 5, onto)
    view_mod.sync(gui, { force = true })
    assert(
      title_row(sb, second) < title_row(sb, first),
      label .. ": and the dragged tab lands below the one it was dropped on"
    )
  end
  config.setup { meta = "auto", backend = { path = "/bin/wez-vtabs" } }
end)

test("a drop on a gap row lands below its card, a drop on the title row lands on it", function()
  local win = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  local hits = state.session.hits[sb:pane_id()]
  local dims = state.session.dims[sb:pane_id()]
  local second = title_row(sb, win.tab_list[2].id)
  eq(hit.drop_slot(hits, second - 1, dims.rows), 2, "pad row")
  eq(hit.drop_slot(hits, second, dims.rows), 2, "title row")
  eq(hit.drop_slot(hits, second + 1, dims.rows, dims.strip_rows), 2, "meta row")
  eq(hit.drop_slot(hits, second + 3, dims.rows, dims.strip_rows), 3, "gap row drops below")
  eq(hit.drop_slot(hits, 1, dims.rows, dims.strip_rows), 1, "inside the strip")
end)

-- P1-spec §7, verbatim. Injected values in other tests cannot keep a wrong default green.
local P1_DEFAULTS = {
  width = 28,
  padding = { top = 1, left = 2, right = 1 },
  edge_to_edge = true,
  row_gap = 0,
  tab_height = "card",
  meta = false,
  separator = "gap",
  pinned_style = "dense",
  new_tab_button = "ghost",
  new_tab_label = "New tab",
  corners = "chamfer",
  scroll_indicator = "auto",
  titlebar = "auto",
  toggle_button = true,
  close_button = "hover",
  show_index = false,
}

test("the shipped defaults are the §7 table", function()
  local shipped = config.defaults
  for key, want in pairs(P1_DEFAULTS) do
    if type(want) == "table" then
      for field, value in pairs(want) do
        eq(shipped[key][field], value, key .. "." .. field)
      end
    else
      eq(shipped[key], want, key)
    end
  end
  eq(shipped.theme.use_scheme_tab_bar, nil, "the deprecated key is gone from the defaults")
  local resolved = config.setup {}
  for key, want in pairs(P1_DEFAULTS) do
    if type(want) ~= "table" then
      eq(resolved[key], want, "setup keeps " .. key)
    end
  end
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("tab_height accepts the row counts as well as the names", function()
  eq(config.setup({ tab_height = 2 }).tab_height, "card")
  eq(config.setup({ tab_height = 1 }).tab_height, "row")
  eq(config.setup({ tab_height = 3 }).tab_height, "tall")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("a dragged card paints its meta row in drag_fg, the loudest colour on that surface", function()
  local win = drag_setup()
  local wid = win.gui:window_id()
  local cards = model.ordered(model.build(win.gui))
  for _, item in ipairs(cards) do
    item.meta = "~/projects/api"
  end
  -- An unmistakable drag_fg, so "which colour painted this row" needs no inference.
  local resolved = theme.resolve({ drag_fg = "#ff00ff" }, fake.palette)
  local cfg = config.get()
  local function frame(drag)
    return render.render {
      cols = 28,
      rows = 14,
      items = cards,
      theme = resolved,
      cfg = cfg,
      glyphs = cfg.glyphs,
      scroll = 0,
      strip = { rows = 1 },
      drag = drag,
    }
  end
  local idle = frame(nil)
  local dragged = frame { tab_id = cards[1].tab_id, over_index = 1, active = true }
  local function meta_row(r)
    for row = 1, 14 do
      if r.hits[row] and r.hits[row].part == "meta" and r.hits[row].id == cards[1].tab_id then
        return row
      end
    end
  end
  local row = meta_row(dragged)
  assert(row, "the drag chip still has a meta row")
  local function row_has(data, y, colour)
    local seg = data:match("\27%[" .. y .. ";1H(.-)\27%[" .. (y + 1) .. ";1H") or ""
    return seg:find("38;2;" .. table.concat(colour, ";"), 1, true) ~= nil
  end
  assert(row_has(dragged.data, row, resolved.drag_fg), "drag colours, not meta_fg")
  assert(not row_has(idle.data, meta_row(idle) or 1, resolved.drag_fg), "an idle card is unaffected")
  local shipped = theme.resolve({}, fake.palette)
  assert(
    theme.contrast(shipped.drag_fg, shipped.drag_bg) >= math.min(3.5, theme.contrast(shipped.fg, shipped.drag_bg)),
    "and by default it is the best the palette can do on drag_bg"
  )
  state.session.drag[wid] = nil
end)

test("a footer row is a target in its own right, never empty space", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local clicked = 0
  local cfg = config.get()
  cfg.hooks.footer = function()
    return {
      { text = "inert" },
      {
        text = "live",
        id = "x",
        on_click = function()
          clicked = clicked + 1
        end,
      },
    }
  end
  view_mod.sync(gui, { force = true })
  local hits = state.session.hits[sb:pane_id()]
  local inert, live
  for row = 1, 24 do
    if hits[row] and hits[row].kind == "footer" then
      if hits[row].entry.on_click then
        live = row
      else
        inert = inert or row
      end
    end
  end
  assert(inert and live, "both footer rows are hit records")
  local before = #win.tab_list
  for _ = 1, 2 do
    mouse(gui, sb, "down", "left", 5, inert)
  end
  eq(#win.tab_list, before, "double-clicking a footer row without on_click opens nothing")
  mouse(gui, sb, "down", "left", 5, live)
  eq(clicked, 1, "and a row with on_click still fires it")
  cfg.hooks.footer = nil
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a hover repaint tracks any span, not just the close button", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  eq(hit.in_close, nil, "the shim is gone")
  local sent = #sb.sent
  local row = title_row(sb, win.tab_list[2].id)
  mouse(gui, sb, "move", "none", 5, row)
  local painted = #sb.sent
  assert(painted > sent, "entering a card repaints")
  mouse(gui, sb, "move", "none", 26, row)
  assert(#sb.sent > painted, "crossing into the close span repaints again")
end)

local schema = require "vtabs.schema"

test("the schema describes every option the defaults expose, and nothing it does not", function()
  local function walk(tbl, prefix, seen)
    for key, value in pairs(tbl) do
      local path = prefix and (prefix .. "." .. key) or key
      local option = schema.by_key[path]
      assert(option, "no descriptor for " .. path)
      seen[path] = true
      -- A list's entries are values, not options; only containers hold more descriptors.
      if type(value) == "table" and not option.open and option.type ~= "list" then
        walk(value, path, seen)
      end
    end
    return seen
  end
  local seen = walk(config.defaults, nil, {})
  for _, option in ipairs(schema.options) do
    if option.default ~= nil and not schema.is_open(option.key) then
      assert(seen[option.key], option.key .. " has a default the config never grows")
    end
    assert(option.label and option.group, option.key .. " needs a label and a group")
    if option.type == "enum" then
      assert(option.enum and #option.enum > 0, option.key .. " is an enum with no values")
      local ok = false
      for _, allowed in ipairs(option.enum) do
        ok = ok or allowed == option.default
      end
      assert(ok, option.key .. " default is not one of its own enum values")
    end
  end
end)

test("schema.defaults is a fresh deep copy each time, so setup cannot poison it", function()
  local a, b = schema.defaults(), schema.defaults()
  assert(a.padding ~= b.padding, "nested tables are not shared")
  a.padding.top = 99
  eq(schema.defaults().padding.top, 1)
  eq(config.defaults.padding.top, 1)
end)

test("a key the schema does not know warns, including inside a closed container", function()
  local function warns_for(opts)
    local before = #wezterm.log
    config.setup(opts)
    for i = before + 1, #wezterm.log do
      if wezterm.log[i]:find("unknown option", 1, true) then
        return wezterm.log[i]
      end
    end
    return nil
  end
  assert(warns_for({ widht = 30 }):find "widht", "top-level typo")
  assert(warns_for({ backend = { pth = "/x" } }):find "backend.pth", "nested typo")
  assert(warns_for({ hooks = { fliter = print } }):find "hooks.fliter")
  eq(warns_for { theme = { accent = "#ff0000" } }, nil, "theme is an open container")
  eq(warns_for { icon_map = { nvim = "x" } }, nil, "so is icon_map")
  eq(warns_for { keys = { new_tab = false } }, nil, "and keys")
  eq(warns_for { private = { env = { FOO = "1" } } }, nil, "and private.env")
  eq(warns_for { backend = { path = "/bin/wez-vtabs" } }, nil, "a known nested key is quiet")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("every enum rejects a value outside it and every range rejects the wrong side", function()
  for _, option in ipairs(schema.options) do
    if option.type == "enum" and not option.key:find "%." then
      local cfg = config.setup { [option.key] = "definitely-not-valid" }
      eq(cfg[option.key], option.default, option.key .. " reset to its default")
    end
  end
  eq(config.setup({ width = 4 }).width, 28, "below min")
  eq(config.setup({ width = "wide" }).width, 28, "wrong type")
  eq(config.setup({ row_gap = -1 }).row_gap, 0)
  eq(config.setup({ theme = { elevation = 2 } }).theme.elevation, 0.06, "above max")
  eq(config.setup({ toggle_button = "yes" }).toggle_button, true)
  eq(config.setup({ padding = { top = -1 } }).padding.top, 1, "nested keys validate too")
  eq(config.setup({ width = 40 }).width, 40, "a valid value survives")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the committed options table is what the schema generates", function()
  local ok, how, code = os.execute "lua ../scripts/gen-docs.lua --check >/dev/null 2>&1"
  local status = code or (ok and 0 or 1)
  eq(status, 0, "docs/configuration.md is stale; run `just docs`")
  assert(how == nil or how == "exit", "generator exited normally")
end)

test("the popover surface is never harder to read than the sidebar body", function()
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local ceiling = math.min(4.5, 0.95 * theme.contrast(t.fg, t.bg))
    local where = " on " .. p.name
    assert(theme.contrast(t.fg, t.surface_raised) >= ceiling - 0.001, "fg vs surface_raised" .. where)
    assert(theme.contrast(t.meta_fg, t.surface_raised) >= 3.5 - 0.001, "meta_fg vs surface_raised" .. where)
    assert(theme.contrast(t.fg, t.surface_raised) < theme.contrast(t.fg, t.bg), "raised is a surface" .. where)
    -- disabled is quiet by design; it only has to stay a colour, not a gate
    assert(theme.contrast(t.disabled_fg, t.surface_raised) > 1.5, "disabled_fg still visible" .. where)
  end
  -- Only the two low-contrast schemes need the lift lowered below 0.09.
  local lowered = {}
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local dark = theme.luminance(t.bg) < 0.5
    local full = theme.mix(t.bg, t.fg, 0.09 * (dark and 1.0 or 0.6))
    if rgb(t.surface_raised) ~= rgb(full) then
      lowered[p.name] = true
    end
  end
  eq(rgb(util.sorted_keys(lowered)), rgb { "Solarized Dark", "Solarized Light" })
end)

test("the scrim is a contrast target: every palette lands in the same narrow band", function()
  local lo, hi = 99, 0
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local where = " on " .. p.name
    assert(t.scrim >= 0.30 and t.scrim <= 0.70, "scrim inside its range" .. where)
    local fg = theme.contrast(theme.mix(t.fg, t.bg, t.scrim), t.bg)
    local card = theme.contrast(theme.mix(t.active_bg, t.bg, t.scrim), t.bg)
    assert(fg >= 2.0 and fg <= 3.0, "scrimmed text stays legible but recedes" .. where .. ": " .. fg)
    assert(card < 1.3, "the scrimmed active card stops reading as a block" .. where .. ": " .. card)
    lo, hi = math.min(lo, fg), math.max(hi, fg)
  end
  assert(hi / lo < 1.2, "a fixed fade would spread 2.5x; the target keeps it under 1.2x")
end)

test("the new surfaces are overridable like every other theme key", function()
  local over = theme.resolve({ surface_raised = "#010203", disabled_fg = "#040506", scrim = 0.5 }, palettes[1])
  eq(rgb(over.surface_raised), "1,2,3")
  eq(rgb(over.disabled_fg), "4,5,6")
  eq(over.scrim, 0.5)
end)

test("the sidebar page is tinted by default; the frame gutter keeps the terminal background", function()
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local where = " on " .. p.name
    assert(rgb(t.bg) ~= hex(p.background), "the default tint is visible" .. where)
    eq(rgb(t.content_bg), hex(p.background), "content_bg is never tinted" .. where)
    eq(rgb(t.bg), rgb(theme.mix(t.content_bg, t.fg, 0.06 * (theme.luminance(t.bg) < 0.5 and 1 or 1))), where)
  end
  eq(config.setup({}).theme.elevation, 0.06, "the shipped default")
  local seamless = theme.resolve({ elevation = 0 }, palettes[1])
  eq(rgb(seamless.bg), hex(palettes[1].background), "0 is still the seamless option")
  -- Out of range resets to the default rather than painting the sidebar in the foreground.
  eq(config.setup({ theme = { elevation = 1 } }).theme.elevation, 0.06)
  eq(config.setup({ theme = { elevation = 0 } }).theme.elevation, 0)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("wezterm is told not to dim the idle pane, unless the user asked for dimming", function()
  local vtabs = dofile(here .. "/../init.lua")
  local function hsb(opts, preset)
    local cfg = { keys = {} }
    cfg.inactive_pane_hsb = preset
    vtabs.apply_to_config(cfg, opts)
    return cfg.inactive_pane_hsb
  end
  local off = hsb {}
  eq(off.brightness, 1.0, "the sidebar is chrome; wezterm would dim it whenever the shell has focus")
  eq(off.saturation, 1.0)
  eq(hsb { dim_inactive_panes = true }, nil, "opting in leaves wezterm's default alone")
  local mine = { brightness = 0.5, saturation = 0.5 }
  eq(hsb({}, mine), mine, "a user value is never overwritten")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("cell counts and durations must be whole numbers", function()
  eq(config.setup({ width = 28.7 }).width, 28, "a fractional width would reach AdjustPaneSize")
  eq(config.setup({ row_gap = 1.5 }).row_gap, 0)
  eq(config.setup({ rail_width = 5.5 }).rail_width, 5)
  eq(config.setup({ poll_ms = 500.5 }).poll_ms, 500)
  eq(config.setup({ padding = { top = 1.2 } }).padding.top, 1)
  eq(config.setup({ tooltip_delay_ms = 600.5 }).tooltip_delay_ms, 600)
  eq(config.setup({ animation = { fps = 30.5 } }).animation.fps, 30)
  eq(config.setup({ width = 32 }).width, 32, "a whole number survives")
  eq(config.setup({ theme = { elevation = 0.06 } }).theme.elevation, 0.06, "ratios still take fractions")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

local function open_popover(row)
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  mouse(gui, sb, "down", "right", 5, row or 3)
  mouse(gui, sb, "up", "right", 5, row or 3)
  view_mod.sync(gui, { force = true })
  return win, gui, sb, popover.get(gui:window_id())
end

---A foreground process the skip list does not name is what makes a close want confirming.
local function make_busy(tab)
  for _, p in ipairs(tab.pane_list) do
    if not sidebar.is_backend(p) then
      p.process = "/usr/bin/sleep"
    end
  end
end

local function popover_row(sb, id)
  for row, h in pairs(state.session.hits[sb:pane_id()] or {}) do
    if h.kind == "popover" and h.id == id then
      return row
    end
  end
end

test("the ✕ on a busy tab asks in the sidebar, and closes only when Close is chosen", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  make_busy(win.tab_list[1])
  local close_row = title_row(sb, win.tab_list[1].id)
  local acted = #win.actions
  mouse(gui, sb, "down", "left", 26, close_row)
  mouse(gui, sb, "up", "left", 26, close_row)
  eq(#win.tab_list, 3, "nothing closes while the question is open")
  eq(#win.actions, acted, "and CloseCurrentTab never ran, so no overlay to dismiss")
  local pop = popover.get(gui:window_id())
  eq(pop.level, "confirm")
  eq(pop.index, 2, "Cancel is selected, so a stray Enter is harmless")

  input.handle(gui, sb, "vtabs", '{"t":"key","key":"escape"}')
  eq(popover.get(gui:window_id()), nil, "escape cancels the question outright")
  eq(#win.tab_list, 3)

  mouse(gui, sb, "down", "left", 26, close_row)
  mouse(gui, sb, "up", "left", 26, close_row)
  view_mod.sync(gui, { force = true })
  local row = popover_row(sb, "confirm_close")
  assert(row, "the confirm level offers Close")
  local col = state.session.hits[sb:pane_id()][row].x1 + 1
  mouse(gui, sb, "down", "left", col, row)
  eq(#win.tab_list, 3, "a destructive item arms on the press, like the ✕ it came from")
  mouse(gui, sb, "up", "left", col, row)
  eq(#win.tab_list, 2, "and choosing it closes the tab")
  eq(popover.get(gui:window_id()), nil)
end)

test("the menu's close items raise the same confirm level, and Cancel leaves the tabs alone", function()
  local win, gui = open_popover(3)
  make_busy(win.tab_list[1])
  popover.run(gui, "close")
  eq(popover.get(gui:window_id()).level, "confirm", "the menu asks instead of closing")
  eq(#win.tab_list, 3)
  popover.run(gui, "confirm_cancel")
  eq(popover.get(gui:window_id()), nil)
  eq(#win.tab_list, 3, "Cancel closed nothing")

  local others, others_gui = open_popover(3)
  for _, tab in ipairs(others.tab_list) do
    make_busy(tab)
  end
  popover.run(others_gui, "close_others")
  local pop = popover.get(others_gui:window_id())
  eq(pop.confirm, "close_others")
  eq(pop.count, 2, "the tabs that are not the anchor")
  local rect = popover.rect(others_gui, 30, 28, theme.resolve({}, fake.palette), config.get())
  local asked = false
  for _, r in ipairs(rect.rows) do
    for _, span in ipairs(r.spans or {}) do
      asked = asked or span.text:find("and 1 other", 1, true) ~= nil
    end
  end
  assert(asked, "the question names the first victim, then how many more follow it")
  popover.run(others_gui, "confirm_close")
  eq(#others.tab_list, 1, "and Close takes them all")
end)

test("a destructive menu item runs on the release, and only over the item it was pressed on", function()
  local win, gui, sb = open_popover(3)
  local before = #win.tab_list
  local rows = {}
  for row, h in pairs(state.session.hits[sb:pane_id()]) do
    if h.kind == "popover" and h.id then
      rows[h.id] = row
    end
  end
  assert(rows.close and rows.activate, "the menu offers a destructive item and a plain one")
  local col = state.session.hits[sb:pane_id()][rows.close].x1 + 2
  mouse(gui, sb, "down", "left", col, rows.close)
  eq(#win.tab_list, before, "the press alone closes nothing")
  assert(popover.get(gui:window_id()), "and leaves the menu up")
  mouse(gui, sb, "up", "left", col, rows.activate)
  eq(#win.tab_list, before, "a release over a different item runs neither")
  mouse(gui, sb, "down", "left", col, rows.close)
  mouse(gui, sb, "up", "left", col, rows.close)
  eq(#win.tab_list, before - 1, "pressed and released on the same item, it runs")
end)

test("a tab the skip list names closes without a question, and so does confirm_close = false", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  local close_row = title_row(sb, win.tab_list[1].id)
  mouse(gui, sb, "down", "left", 26, close_row)
  mouse(gui, sb, "up", "left", 26, close_row)
  eq(popover.get(gui:window_id()), nil, "zsh is on the skip list")
  eq(#win.tab_list, 2)

  local opted, opted_gui = drag_setup()
  local sb2 = sidebar.find(opted.tab_list[1])
  make_busy(opted.tab_list[1])
  config.setup { meta = "auto", confirm_close = false, backend = { path = "/bin/wez-vtabs" } }
  view_mod.sync(opted_gui, { force = true })
  local opted_row = title_row(sb2, opted.tab_list[1].id)
  mouse(opted_gui, sb2, "down", "left", 26, opted_row)
  mouse(opted_gui, sb2, "up", "left", 26, opted_row)
  eq(popover.get(opted_gui:window_id()), nil, "opting out never asks")
  eq(#opted.tab_list, 2)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the menu opens at the column that asked for it, and its hits are pane columns", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  local cfg = config.get()
  local resolved = theme.resolve({}, fake.palette)
  local function rect_at(col)
    popover.close(gui)
    popover.open(gui, win.tab_list[1].id, 9, col)
    return popover.rect(gui, 24, 28, resolved, cfg)
  end
  local narrow = rect_at(3)
  eq(narrow.x, 3, "a click on the card's own gutter opens flush with it")
  eq(rect_at(4).x, 4, "one column right of it opens one column right: the menu tracks the click")
  local mid = rect_at(12)
  local first_col = cfg.padding.left + 1
  local last_col = 28 - cfg.padding.right - mid.w + 1
  eq(mid.x, math.min(12, last_col), "and one further right tracks the pointer to where it still fits")
  eq(rect_at(28).x, last_col, "a click at the right edge slides the whole menu back inside")
  eq(rect_at(1).x, first_col, "and one left of the padding is pushed off it")
  assert(mid.x + mid.w - 1 <= 28 - cfg.padding.right, "the right border never leaves the sidebar")

  -- The hit map is read against pane columns, so a rect that no longer starts at the padding must
  -- still map a click to the row it painted.
  local placed = rect_at(12)
  local rows = {}
  for i, row in ipairs(placed.rows) do
    if row.hit and row.hit.id then
      rows[#rows + 1] = { y = placed.y + i - 1, hit = row.hit }
    end
  end
  assert(#rows >= 2, "the menu painted items")
  local second = rows[2]
  eq(second.hit.x1, placed.x, "the span is the rect's own columns, absolute")
  eq(second.hit.x2, placed.x + placed.w - 1)
  view_mod.sync(gui, { force = true })
  local live = state.session.hits[sb:pane_id()]
  local found = nil
  for row, h in pairs(live) do
    if h.kind == "popover" and h.id and not found then
      found = { row = row, hit = h }
    end
  end
  assert(found, "and the live frame carries them too")
  assert(found.hit.x1 > 1, "a click at column 1 is scrim, not the first item")
  popover.close(gui)
end)

test("a click level with an item but beside the menu dismisses it, and never runs it", function()
  local win, gui, sb = open_popover(3)
  local before = #win.tab_list
  local hits = state.session.hits[sb:pane_id()]
  local close_row, spare = nil, nil
  for row, h in pairs(hits) do
    if h.kind == "popover" and h.id == "close" then
      close_row = row
    end
  end
  assert(close_row, "the menu offers Close tab")
  local rect = hits[close_row]
  for col = 1, 28 do
    if col < rect.x1 or col > rect.x2 then
      spare = spare or col
    end
  end
  assert(spare, "and the sidebar has columns the menu does not own on that row")
  mouse(gui, sb, "down", "left", spare, close_row)
  eq(#win.tab_list, before, "the tab survives a click that only shares the row")
  eq(popover.get(gui:window_id()), nil, "and the menu is dismissed like any click away")
end)

test("a menu is painted at every width the schema allows, and never opens without being painted", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  local resolved = theme.resolve({}, fake.palette)
  for _, cols in ipairs { 8, 9, 10, 14, 20, 28 } do
    config.setup { meta = "auto", width = cols, backend = { path = "/bin/wez-vtabs" } }
    popover.close(gui)
    popover.open(gui, win.tab_list[1].id, 3, 2)
    local rect = popover.rect(gui, 24, cols, resolved, config.get())
    assert(rect, "a menu at " .. cols .. " columns, however cramped")
    assert(rect.x >= 1 and rect.x + rect.w - 1 <= cols, "inside the pane at " .. cols)
    for _, row in ipairs(rect.rows) do
      eq(row.hit.x1, rect.x, "every row carries the rect's columns at " .. cols)
    end
  end
  popover.close(gui)

  -- Belt and braces for the same failure: a level that is open but unpainted must not eat clicks.
  config.setup { meta = "auto", backend = { path = "/bin/wez-vtabs" } }
  view_mod.sync(gui, { force = true })
  local active = win.active_tab_ref
  popover.open(gui, win.tab_list[3].id, 3, 2)
  eq(popover.get(gui:window_id()) ~= nil, true)
  mouse(gui, sb, "down", "left", 5, title_row(sb, win.tab_list[3].id))
  eq(popover.get(gui:window_id()), nil, "the first click dismisses a menu nothing painted")
  eq(win.active_tab_ref, win.tab_list[3], "and is handled as the click it was")
  win.active_tab_ref = active
end)

test("a busy tab in a sidebar too narrow to ask falls back to wezterm's own confirmation", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  make_busy(win.tab_list[1])
  local narrow = sb.cols
  sb.cols = 10
  local acted = #win.actions
  actions.request_close(gui, win.tab_list[1].id, 3, 2)
  eq(popover.get(gui:window_id()), nil, "no unreadable question is opened")
  assert(#win.actions > acted, "wezterm is asked instead")
  eq(win.actions[#win.actions].action.arg.confirm, true, "with its own overlay, which a key can use")
  sb.cols = narrow
end)

test("the selected menu row is an accent fill that clears 4.5 on all ten palettes", function()
  local schemes = require "palettes"
  for _, p in ipairs(schemes) do
    local t = theme.resolve({}, p)
    local where = " on " .. p.name
    assert(theme.contrast(t.popover_sel_fg, t.popover_sel_bg) >= 4.5, "selected text" .. where)
    assert(theme.contrast(t.popover_sel_bg, t.surface_raised) >= 2.5, "fill against the panel" .. where)
    assert(theme.contrast(t.popover_sel_hint, t.popover_sel_bg) >= 3.0, "hint" .. where)
    local ink = rgb(t.popover_sel_fg)
    assert(ink == "0,0,0" or ink == "255,255,255", "the ink is one of the two absolutes" .. where)
    -- The construction this replaced: hover_bg on surface_raised was the same colour in practice.
    assert(theme.contrast(t.hover_bg, t.surface_raised) < 1.2, "which the old one never was" .. where)
  end
end)

test("the pointer selects the row under it inside the menu, and leaves it alone outside", function()
  local win, gui, sb = open_popover(3)
  local pop = popover.get(gui:window_id())
  pop.index = 1
  local hits = state.session.hits[sb:pane_id()]
  local item_row, scrim_row, disabled_row
  for row, h in pairs(hits) do
    if h.kind == "popover" and h.id then
      if h.disabled then
        disabled_row = row
      elseif not item_row and popover.items(gui, pop.tab_id)[1].id ~= h.id then
        item_row = row
      end
    elseif h.kind == "scrim" then
      scrim_row = scrim_row or row
    end
  end
  assert(item_row and scrim_row and disabled_row, "an enabled row, a disabled one and the scrim")
  mouse(gui, sb, "move", "none", hits[item_row].x1 + 1, item_row)
  eq(popover.selected(gui).id, hits[item_row].id, "the pointer picked the row it is over")
  local picked = popover.get(gui:window_id()).index
  mouse(gui, sb, "move", "none", 1, scrim_row)
  eq(popover.get(gui:window_id()).index, picked, "the scrim never erases the selection")
  mouse(gui, sb, "move", "none", hits[disabled_row].x1 + 1, disabled_row)
  eq(popover.get(gui:window_id()).index, picked, "and a disabled row is not selectable")
  config.setup { meta = "auto", popover = { follow_pointer = false }, backend = { path = "/bin/wez-vtabs" } }
  mouse(gui, sb, "move", "none", hits[item_row].x1 + 1, item_row)
  eq(popover.get(gui:window_id()).index, picked, "opting out pins the selection to the keyboard")
  config.setup { meta = "auto", backend = { path = "/bin/wez-vtabs" } }
  popover.close(gui)
  eq(#win.tab_list, 3)
end)

test("opening the menu fades its own rows once, for the configured time, and closing does not", function()
  local win, gui = drag_setup()
  local sb = sidebar.find(win.tab_list[1])
  local function anims_since(n)
    local out = {}
    for i = n + 1, #sb.sent do
      if sb.sent[i]:find('"anim"', 1, true) then
        out[#out + 1] = sb.sent[i]
      end
    end
    return out
  end
  local before = #sb.sent
  mouse(gui, sb, "down", "right", 5, 3)
  mouse(gui, sb, "up", "right", 5, 3)
  local sent = anims_since(before)
  eq(#sent, 1, "one fade, on the frame that first painted the menu")
  eq(sent[1]:match '"ms":(%d+)', "90", "the configured duration reaches the backend")
  local rect = popover.rect(gui, 24, 28, theme.resolve({}, fake.palette), config.get())
  local rows = 0
  for _ in sent[1]:gmatch '"y":%d+' do
    rows = rows + 1
  end
  eq(rows, rect.h, "over the menu's own rows, not the whole sidebar")

  before = #sb.sent
  popover.close(gui)
  view_mod.sync(gui, { force = true })
  eq(#anims_since(before), 0, "a dismissed menu vanishes at once")

  config.setup { meta = "auto", popover = { fade_ms = 0 }, backend = { path = "/bin/wez-vtabs" } }
  before = #sb.sent
  mouse(gui, sb, "down", "right", 5, 3)
  mouse(gui, sb, "up", "right", 5, 3)
  eq(#anims_since(before), 0, "and 0 turns it off")
  popover.close(gui)
  config.setup { meta = "auto", backend = { path = "/bin/wez-vtabs" } }
end)

test("the menu is as wide as its widest row wants, clamped to the sidebar", function()
  local cfg = config.get()
  local menu = popover.items(fake.window().gui, 1)
  local natural = popover.width_for(cfg, 80, menu, nil)
  eq(natural, 23, "the widest label is 18 cells and carries no hint")
  eq(popover.width_for(cfg, 28, menu, nil), 23, "the shipped 28-column sidebar still fits it whole")
  eq(popover.width_for(cfg, 22, menu, nil), 19, "a narrower one caps it at the columns it can spare")
  local fixed = util.merge(cfg, { popover = { width = 18 } })
  eq(popover.width_for(fixed, 80, menu, nil), 18, "a number is taken verbatim")
  eq(popover.width_for(util.merge(cfg, { popover = { width = 4 } }), 80, menu, nil), 16, "with a floor")
  eq(popover.width_for(cfg, 80, menu, { string.rep("x", 40) }), 45, "a header can widen it too")
  eq(config.setup({ popover = { width = "wide" } }).popover.width, "auto", "a bad value resets")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the popover title wraps on a space, then a slash, then hard", function()
  eq(rgb(popover.wrap("nvim plugin/vtabs/render.lua", 22, 3)), rgb { "nvim", "plugin/vtabs/", "render.lua" })
  eq(rgb(popover.wrap("short", 22, 3)), rgb { "short" })
  local hard = popover.wrap(string.rep("x", 50), 10, 3)
  eq(#hard, 3)
  for _, line in ipairs(hard) do
    assert(util.width(line) <= 10, "each wrapped line fits")
  end
end)

test("every popover row is exactly the rect width, header shrunk or not", function()
  local _, gui = open_popover(3)
  local cfg = config.get()
  local resolved = theme.resolve({}, fake.palette)
  for _, rows in ipairs { 30, 14, 10, 8, 6, 5 } do
    local rect = popover.rect(gui, rows, 28, resolved, cfg)
    assert(rect, "a rect at " .. rows .. " rows")
    eq(rect.h, #rect.rows)
    assert(rect.y >= 1 and rect.y + rect.h - 1 <= math.max(rows, rect.h), "on screen at " .. rows)
    for _, row in ipairs(rect.rows) do
      for _, span in ipairs(row.spans or {}) do
        assert(span.x >= 1 and span.x + util.width(span.text) - 1 <= rect.w, "span inside the rect")
      end
    end
  end
  popover.close(gui)
end)

test("items are never dropped: the header shrinks and the list scrolls instead", function()
  local _, gui = open_popover(3)
  local pop = popover.get(gui:window_id())
  local count = #popover.items(gui, pop.tab_id)
  for _, rows in ipairs { 30, 16, 12, 10 } do
    local placed = popover.layout(gui, pop, rows, 28)
    eq(#placed.items, count, "all items still there at " .. rows .. " rows")
  end
  local tall = popover.layout(gui, pop, 30, 28)
  local short = popover.layout(gui, pop, 12, 28)
  assert(#short.lines < #tall.lines, "the header is what gives way")
  popover.close(gui)
end)

test("selection skips disabled items and first-letter jump lands on an enabled one", function()
  local _, gui = open_popover(3)
  local pop = popover.get(gui:window_id())
  local entries = popover.items(gui, pop.tab_id)
  local space_at
  for i, item in ipairs(entries) do
    if item.id == "space" then
      space_at = i
    end
  end
  assert(space_at and entries[space_at].disabled, "Move to space is disabled until P4")
  pop.index = space_at - 1
  popover.move(gui, 1)
  assert(pop.index ~= space_at, "the disabled item is stepped over")
  pop.index = 1
  assert(popover.jump(gui, "c"), "jumps to Close")
  assert(not popover.items(gui, pop.tab_id)[pop.index].disabled)
  pop.index = 1
  assert(popover.jump(gui, "m"), "jumps past the disabled Move to space to Move to new window")
  eq(popover.items(gui, pop.tab_id)[pop.index].id, "tear_off")
  pop.index = 1
  eq(popover.jump(gui, "q"), false, "no item starts with q, so nothing moves")
  eq(pop.index, 1)
  popover.close(gui)
end)

test("click-away closes without switching tabs; the frame and disabled items are inert", function()
  local win, gui, sb = open_popover(3)
  local active = win.active_tab_ref
  local hits = state.session.hits[sb:pane_id()]
  local scrim_row, frame_row, item_row
  for row = 1, 20 do
    local h = hits[row]
    if h and h.kind == "scrim" then
      scrim_row = scrim_row or row
    elseif h and h.kind == "popover" then
      if h.id then
        item_row = item_row or row
      else
        frame_row = frame_row or row
      end
    end
  end
  assert(scrim_row and frame_row and item_row, "scrim, frame and item rows all present")
  mouse(gui, sb, "down", "left", 5, frame_row)
  assert(popover.get(gui:window_id()), "the frame does not close it")
  mouse(gui, sb, "down", "left", 5, scrim_row)
  eq(popover.get(gui:window_id()), nil, "a click away closes it")
  eq(win.active_tab_ref, active, "and does not switch tabs")
end)

test("no tab hit records survive while the popover is open", function()
  local _, gui, sb = open_popover(3)
  for _, h in pairs(state.session.hits[sb:pane_id()]) do
    assert(h.kind == "scrim" or h.kind == "popover", "unexpected " .. tostring(h.kind))
  end
  popover.close(gui)
end)

test("a middle click is ignored while the popover is open, a right click retargets it", function()
  local win, gui, sb = open_popover(3)
  local before = #win.tab_list
  -- Row 3 is the anchor card itself: scrimmed now, a tab row again once the popover closes.
  local scrim_row = 3
  eq(state.session.hits[sb:pane_id()][scrim_row].kind, "scrim")
  mouse(gui, sb, "down", "middle", 5, scrim_row)
  eq(#win.tab_list, before, "middle does not close a tab through the scrim")
  assert(popover.get(gui:window_id()), "and does not dismiss")
  mouse(gui, sb, "down", "right", 5, scrim_row)
  eq(popover.get(gui:window_id()), nil, "right on the scrim closes")
  mouse(gui, sb, "up", "right", 5, scrim_row)
  assert(popover.get(gui:window_id()), "and the release retargets rather than dismissing")
  popover.close(gui)
end)

test("raw key forwarding is suppressed while the popover is open", function()
  local _, gui, sb = open_popover(3)
  local tab = util.active_tab(gui)
  local content = sidebar.content_pane(tab)
  local before = #content.sent
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"l","raw":"bA=="}')
  eq(#content.sent, before, "nothing reaches the shell")
  assert(popover.get(gui:window_id()), "and the popover is still open")
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"escape"}')
  eq(popover.get(gui:window_id()), nil, "escape closes it")
end)

test("rename commits on enter, cancels on escape, and handles the terminal chords", function()
  local win, gui, sb = open_popover(3)
  local tab = win.tab_list[1]
  tab:set_title "render.lua"
  popover.run(gui, "rename")
  local pop = popover.get(gui:window_id())
  eq(pop.level, "rename")
  eq(pop.buffer, "render.lua")
  popover.edit(pop, "backspace", {})
  eq(pop.buffer, "render.lu")
  popover.edit(pop, "x", {})
  eq(pop.buffer, "render.lux")
  popover.edit(pop, "a", { "ctrl" })
  eq(pop.cursor, 1)
  popover.edit(pop, "e", { "ctrl" })
  eq(pop.cursor, 11)
  popover.edit(pop, "u", { "ctrl" })
  eq(pop.buffer, "")
  for ch in ("hello world"):gmatch "." do
    popover.edit(pop, ch, {})
  end
  popover.edit(pop, "w", { "ctrl" })
  eq(pop.buffer, "hello ")
  eq(popover.edit(pop, "enter", {}), "commit")
  popover.commit_rename(gui)
  eq(tab:get_title(), "hello ")
  eq(popover.get(gui:window_id()), nil)

  view_mod.sync(gui, { force = true })
  mouse(gui, sb, "down", "right", 5, 3)
  mouse(gui, sb, "up", "right", 5, 3)
  popover.run(gui, "rename")
  local second = popover.get(gui:window_id())
  popover.edit(second, "z", {})
  eq(popover.edit(second, "escape", {}), "cancel")
  popover.back(gui)
  eq(tab:get_title(), "hello ", "cancel leaves the title untouched")
  eq(popover.get(gui:window_id()).level, "root", "escape steps back a level before closing")
  popover.close(gui)
end)

test("context = false removes the mouse trigger but not the keyboard one", function()
  local win, gui = drag_setup()
  config.setup { backend = { path = "/bin/wez-vtabs" }, context = false }
  local sb = sidebar.find(win.tab_list[1])
  mouse(gui, sb, "down", "right", 5, 3)
  mouse(gui, sb, "up", "right", 5, 3)
  eq(popover.get(gui:window_id()), nil, "right click does nothing")
  state.set_focus(gui:window_id(), true)
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"m"}')
  assert(popover.get(gui:window_id()), "m in keyboard mode still opens it")
  popover.close(gui)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("collapsed = rail keeps the pane and narrows it to rail_width", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(sb.cols, 28)
  sidebar.set_collapsed(gui, true)
  eq(#tab:panes(), 2, "the rail keeps the pane")
  eq(geometry.desired(gui:window_id()), 5)
  local before = #win.actions
  assert(geometry.correct(gui), "one correction")
  eq(#win.actions - before, 1, "exactly one AdjustPaneSize")
  eq(sb.cols, 5)
  sidebar.set_collapsed(gui, false)
  eq(geometry.desired(gui:window_id()), 28)
  assert(geometry.correct(gui))
  eq(sb.cols, 28)
end)

test("the rail flag reaches strip_geometry, so the toggle lands inside the rail", function()
  local win, gui = setup_window(1)
  config.setup { collapsed = "rail", rail_width = 5, titlebar = "macos", backend = { path = "/bin/wez-vtabs" } }
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  -- the reserve only exists when the pane reports a cell box, and only then can the toggle escape it
  sb.get_dimensions = function(self)
    return { cols = self.cols, viewport_rows = 24, pixel_width = self.cols * 10, pixel_height = 456 }
  end
  local was_mac = platform.is_mac
  platform.is_mac = false
  sidebar.set_collapsed(gui, true)
  geometry.correct(gui)
  local seen
  local render_mod = require "vtabs.render"
  local original = render_mod.render
  render_mod.render = function(frame)
    seen = frame
    return original(frame)
  end
  view_mod.invalidate_theme()
  view_mod.sync(gui, { force = true })
  render_mod.render = original
  platform.is_mac = was_mac
  assert(seen, "the rail rendered")
  eq(seen.rail, true, "collapsed to the rail")
  assert(seen.strip.cols > 0, "and the preview reserved columns for the lights")
  assert(
    seen.strip.toggle.x <= seen.cols,
    "the toggle is inside the rail, not at reserve + 2 past its end: " .. tostring(seen.strip.toggle.x)
  )
  eq(seen.strip.toggle_row, seen.strip.rows - config.get().padding.top, "the toggle row sits below the reserve")
  local rows = {}
  local painted = render.render(seen)
  for row = 1, seen.rows do
    rows[row] = row_text(painted.data, row)
  end
  local x = math.ceil(seen.cols / 2)
  eq(usub(rows[seen.strip.toggle_row], x, x), "«", "and §8 centres the one glyph that fits in the rail")
  sidebar.set_collapsed(gui, false)
  view_mod.invalidate_theme()
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("the rail toggle centres below the macOS reserve instead of beside it", function()
  local mac = {
    is_mac = true,
    integrated_buttons = true,
    native_button_style = true,
    position = "left",
    padding_top = 1,
    toggle_button = true,
    card_x1 = 2,
    rail = true,
    rail_width = 5,
  }
  local g = platform.strip_geometry(RETINA, mac)
  eq(g.cols, 9)
  eq(g.rows_reserved, 2, "the raw reserve, before padding")
  eq(g.toggle_row, 3, "below the lights, not beside them")
  eq(g.width, 9, "the rail widens to the reserve")
  eq(g.toggle_x, 5, "centred in the widened rail")
  eq(g.rows, 4)
  local wide = platform.strip_geometry(
    RETINA,
    { is_mac = false, rail = true, rail_width = 5, toggle_button = true, padding_top = 1 }
  )
  eq(wide.cols, 0)
  eq(wide.toggle_row, 1)
  eq(wide.toggle_x, 3, "centre of a 5-column rail")
end)

test("the active title is accent-tinted where the scheme can carry it, else it keeps fg", function()
  local barred = {}
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    local where = " on " .. p.name
    eq(rgb(t.active_title_fg), rgb(t.title_active), "both names, one colour" .. where)
    eq(t.title_active_contrast, theme.contrast(t.title_active, t.active_bg), "exposed, not recomputed" .. where)
    assert(t.title_active_contrast >= math.min(4.5, theme.contrast(t.fg, t.active_bg)) - 0.001, "gate" .. where)
    if t.title_active_contrast < 4.0 then
      barred[p.name] = true
    end
  end
  -- The schemes whose own fg cannot reach 4.0 on the tinted card keep the accent bar instead.
  eq(rgb(util.sorted_keys(barred)), rgb { "One Dark", "Solarized Dark", "Solarized Light" })
end)

test("content_bg is the untinted terminal background, whatever elevation does to the page", function()
  for _, p in ipairs(palettes) do
    local t = theme.resolve({}, p)
    eq(rgb(t.content_bg), hex(p.background), "on " .. p.name)
    assert(rgb(t.bg) ~= rgb(t.content_bg), "the page is tinted, the gutter is not")
  end
  local seamless = theme.resolve({ elevation = 0 }, palettes[1])
  eq(rgb(seamless.content_bg), rgb(seamless.bg), "they coincide only at elevation 0")
end)

test("tall cards and the frame are configurable, and false is the frame default", function()
  eq(config.setup({ tab_height = "tall" }).tab_height, "tall")
  eq(config.setup({ tab_height = 3 }).tab_height, "tall")
  eq(config.setup({ tab_height = "gigantic" }).tab_height, "card", "an unknown height resets")
  eq(config.setup({}).frame, false)
  local framed = config.setup { frame = { margin = 1, corners = "chamfer" } }
  eq(framed.frame.margin, 1)
  eq(framed.frame.corners, "chamfer")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a rail is corrected to exactly rail_width, below the 8-column sidebar floor", function()
  for _, width in ipairs { 3, 5, 7 } do
    local win, gui = setup_window(1)
    config.setup { backend = { path = "/bin/wez-vtabs" }, rail_width = width }
    sidebar.ensure(gui)
    local sb = mark_ready(win.tab_list[1])
    sidebar.set_collapsed(gui, true)
    eq(geometry.desired(gui:window_id()), width)
    assert(geometry.correct(gui), "one correction at rail_width " .. width)
    eq(sb.cols, width, "the sidebar floor must not raise a deliberate rail")
    sidebar.set_collapsed(gui, false)
    assert(geometry.correct(gui))
    eq(sb.cols, 28, "and expanding still lands on cfg.width")
  end
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("every vtabs module is on the config-reload watch list", function()
  local vtabs = dofile(here .. "/../init.lua")
  local watched = {}
  for _, name in ipairs(vtabs.module_names()) do
    watched[name] = true
  end
  local listing = io.popen('ls "' .. here .. '/../vtabs"')
  local missing = {}
  local seen = 0
  for line in listing:lines() do
    local name = line:match "^(.+)%.lua$"
    if name then
      seen = seen + 1
      if not watched[name] then
        missing[#missing + 1] = name
      end
    end
  end
  listing:close()
  assert(seen > 10, "the listing found the modules")
  eq(table.concat(missing, ","), "", "modules not watched, so edits to them would not reload")
end)

test("a toggle sends the fade around its single resize, and only on a local domain", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  view_mod.sync(gui, { force = true })
  local function anims_since(n)
    local out = {}
    for i = n + 1, #sb.sent do
      local phase = sb.sent[i]:match '"t":"anim".-"phase":"([%a_]+)"' or sb.sent[i]:match '"phase":"([%a_]+)"'
      if sb.sent[i]:find('"anim"', 1, true) then
        out[#out + 1] = phase or "?"
      end
    end
    return out
  end
  local before = #sb.sent
  local actions_before = #win.actions
  actions.toggle_sidebar(gui)
  local sent = anims_since(before)
  assert(#sent >= 1, "the collapse is animated")
  local resizes = 0
  for i = actions_before + 1, #win.actions do
    if win.actions[i].action.action == "AdjustPaneSize" then
      resizes = resizes + 1
    end
  end
  eq(resizes, 1, "one pane resize per toggle, whatever the fade does")

  before = #sb.sent
  sb.domain = "SSH:archie"
  actions.toggle_sidebar(gui)
  eq(#anims_since(before), 0, "animations = auto is off for a remote domain")
  sb.domain = "local"

  config.setup { backend = { path = "/bin/wez-vtabs" }, animations = false }
  before = #sb.sent
  actions.toggle_sidebar(gui)
  eq(#anims_since(before), 0, "and off entirely when asked")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
  sidebar.set_collapsed(gui, false)
end)

test("collapsed = hidden bands the window so the macOS lights clear the shell", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  mark_ready(win.tab_list[1])
  gui.decorations = "INTEGRATED_BUTTONS|RESIZE"
  gui.window_padding = { left = 4, right = 4, top = 0, bottom = 2 }
  local was_mac = platform.is_mac
  platform.is_mac = true
  view_mod.invalidate_theme()
  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden" }

  eq(gui:get_config_overrides().window_padding, nil, "nothing is overridden while expanded")
  state.set_collapsed(gui:window_id(), true)
  assert(view_mod.apply_titlebar_band(gui), "collapsing applies the band")
  local padded = gui:get_config_overrides().window_padding
  eq(padded.top, platform.TITLEBAR_PAD, "the band is the light reserve, in dpi-scaled points")
  eq(padded.left, 4, "the user's other sides are kept")
  eq(padded.bottom, 2)
  eq(view_mod.apply_titlebar_band(gui), false, "and it is idempotent")
  assert(state.applying_recently(gui:window_id()), "the reload it triggers is marked as ours")

  state.set_collapsed(gui:window_id(), false)
  assert(view_mod.apply_titlebar_band(gui), "expanding clears it")
  eq(gui:get_config_overrides().window_padding, nil, "back to the user's own padding")

  -- The rail never needs the band: its own pane still owns the window's top-left.
  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "rail" }
  state.set_collapsed(gui:window_id(), true)
  eq(view_mod.apply_titlebar_band(gui), false, "a rail is not banded")
  eq(gui:get_config_overrides().window_padding, nil)

  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden", rail_titlebar = "none" }
  eq(view_mod.apply_titlebar_band(gui), false, "rail_titlebar = none declines")
  eq(gui:get_config_overrides().window_padding, nil, "and leaves window_padding alone")

  config.setup { backend = { path = "/bin/wez-vtabs" }, collapsed = "hidden" }
  gui.full_screen = true
  eq(view_mod.apply_titlebar_band(gui), false, "fullscreen has no titlebar to clear")
  gui.full_screen = false
  gui.decorations = "RESIZE"
  view_mod.invalidate_theme()
  eq(view_mod.apply_titlebar_band(gui), false, "and neither does a window without the buttons")

  platform.is_mac = was_mac
  gui.decorations, gui.window_padding = nil, nil
  state.set_collapsed(gui:window_id(), false)
  view_mod.invalidate_theme()
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

os.remove(state.file)

-- ===================== lead: post-review fixes (P2) =====================

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

test("composite ignores a rect that lies entirely outside the pane", function()
  local plain = render.render(p1_view { rows = 12 })
  local v = p1_view { rows = 12 }
  v.popover = popover_rect { y = 40, h = 5 }
  local out = render.render(v)
  eq(out.data, plain.data)
  for row, h in pairs(out.hits) do
    assert(h.kind ~= "scrim", "row " .. row .. " must not be scrimmed by an invisible popover")
  end
end)

test("P2 anim: a 60-row frame fits the 24 KiB bound", function()
  local frame = render.render(p1_view { rows = 60 })
  local cmd, reason = anim.build("expand_in", frame, { id = 9, anchor = "#1e1e2e" })
  assert(cmd, "60 rows must animate, got " .. tostring(reason))
  assert(#cmd.data <= anim.MAX_DATA)
end)

test("the rail's narrow width is never adopted as the user's desired width", function()
  local win, gui = setup_window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  local wid = gui:window_id()
  eq(geometry.correct(gui), false, "baseline recorded")
  state.set_collapsed(wid, true)
  geometry.correct(gui)
  eq(sb.cols, config.get().rail_width, "collapsed to the rail")
  geometry.correct(gui)
  state.set_collapsed(wid, false)
  assert(geometry.correct(gui), "expanding corrects back")
  eq(sb.cols, 28)
  eq(geometry.desired(wid), 28, "the rail width was not adopted as a divider drag")
end)

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

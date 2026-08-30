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

local function row_text(data, row)
  local seg = data:match("\27%[" .. row .. ";1H(.-)\27%[" .. (row + 1) .. ";1H")
    or data:match("\27%[" .. row .. ";1H(.*)$")
  return strip(seg)
end

local util = require "vtabs.util"
local config = require "vtabs.config"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local state = require "vtabs.state"

test("merge nests tables and replaces lists", function()
  local out = util.merge({ a = { b = 1, c = 2 }, l = { 1, 2 } }, { a = { c = 3 }, l = { 9 } })
  eq(out.a.b, 1)
  eq(out.a.c, 3)
  eq(#out.l, 1)
  eq(out.l[1], 9)
end)

test("truncate keeps width budget", function()
  eq(util.truncate("hello world", 5, "…"), "hell…")
  eq(util.truncate("hi", 5, "…"), "hi")
  eq(util.width(util.truncate("ünïcödé text", 6, "…")), 6)
end)

test("config validates enums and width", function()
  local cfg = config.setup { position = "top", width = 2 }
  eq(cfg.position, "left")
  eq(cfg.width, 28)
  eq(config.setup({ width = 20 }).width, 20)
end)

test("theme resolves from palette with user overrides", function()
  local t = theme.resolve(
    { accent = "#ff0000" },
    { background = "#101010", foreground = "#f0f0f0", ansi = {}, brights = {} }
  )
  eq(t.accent[1], 255)
  eq(t.fg[1], 240)
  assert(t.bg[1] < 16)
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
  local cfg = config.setup {}
  local v = {
    cols = 28,
    rows = 10,
    items = items(),
    theme = theme.resolve({}, {}),
    cfg = cfg,
    icons = { close = "x", new_tab = "+", unseen = "*" },
    scroll = 0,
  }
  for k, val in pairs(over or {}) do
    v[k] = val
  end
  return v
end

test("every rendered row is exactly cols wide", function()
  local long = items()
  long[3].title = "a very long title that has unseen output and overflows"
  local narrow = view { cols = 12, items = items(), hover = { x = 12, y = 4 } }
  narrow.cfg = config.setup { width = 12 }
  local variants =
    { view(), view { hover = { x = 27, y = 4 } }, view { hover = { x = 3, y = 5 } }, view { items = long } }
  for _, v in ipairs(variants) do
    for row = 1, 10 do
      eq(util.width(row_text(render.render(v).data, row)), 28, "row " .. row)
    end
  end
  for row = 1, 10 do
    eq(util.width(row_text(render.render(narrow).data, row)), 12, "narrow row " .. row)
  end
end)

test("layout: padding, pinned, separator, tabs, new tab", function()
  local r = render.render(view())
  eq(r.hits[1].kind, "space")
  eq(r.hits[2].kind, "tab")
  eq(r.hits[2].tab_id, 1)
  eq(r.hits[3].kind, "separator")
  eq(r.hits[4].tab_id, 2)
  eq(r.hits[5].tab_id, 3)
  eq(r.hits[6].kind, "new_tab")
  eq(r.hits[7].kind, "space")
  eq(r.total_rows, 6)
  assert(strip(r.data):find "New Tab")
  assert(strip(r.data):find "…", "long title truncated")
end)

test("hover shows close button with a hit span", function()
  local plain = render.render(view())
  eq(plain.hits[5].close, nil)
  local r = render.render(view { hover = { x = 5, y = 5 } })
  assert(r.hits[5].close, "close span present")
  eq(r.hits[5].close.to, 27)
  local text = row_text(r.data, 5)
  eq(text:sub(r.hits[5].close.from, r.hits[5].close.to), "x")
end)

test("pinned compact rows never show close", function()
  local r = render.render(view { hover = { x = 27, y = 2 } })
  eq(r.hits[2].close, nil)
end)

test("unseen marker rendered for inactive tabs", function()
  local r = render.render(view())
  local text = row_text(r.data, 5)
  assert(text:find "%* $", "unseen glyph near right edge: " .. text)
end)

test("drag ghost reorders and slots renumber", function()
  local r = render.render(view { drag = { tab_id = 3, over_index = 2, active = true } })
  eq(r.hits[4].tab_id, 3)
  eq(r.hits[5].tab_id, 2)
  eq(r.hits[4].slot, 2)
end)

test("scroll clamps and ensure_visible follows active", function()
  local many = {}
  for i = 1, 30 do
    many[i] =
      { tab_id = i, index = i, is_active = i == 25, is_pinned = false, title = "t" .. i, icon = "", has_unseen = false }
  end
  local r = render.render(view { items = many, rows = 10, scroll = 999 })
  eq(r.scroll, 22)
  r = render.render(view { items = many, rows = 10, scroll = 0, ensure_visible = 25 })
  local found = false
  for row = 1, 10 do
    if r.hits[row].tab_id == 25 then
      found = true
    end
  end
  assert(found, "active row visible after ensure_visible")
end)

test("keys build honours overrides and disables", function()
  local all = keys.build {}
  assert(#all >= 20)
  eq(#keys.build(false), 0)
  local custom = keys.build { new_tab = false, toggle_sidebar = { key = "s", mods = "ALT" } }
  local found_toggle, found_new = false, false
  for _, k in ipairs(custom) do
    if k.key == "s" and k.mods == "ALT" then
      found_toggle = true
    end
    if k.key == "t" and k.mods == "CMD" then
      found_new = true
    end
  end
  assert(found_toggle)
  assert(not found_new)
end)

test("state persists to wezterm.GLOBAL", function()
  local wezterm = require "wezterm"
  state.set_pinned(7, true)
  assert(state.is_pinned(7))
  assert(wezterm.GLOBAL.vtabs.pinned["7"])
  state.set_pinned(7, false)
  assert(not state.is_pinned(7))
  state.push_closed { cwd = "/tmp" }
  eq(state.pop_closed().cwd, "/tmp")
  eq(state.pop_closed(), nil)
end)

print(string.format("%d passed, %d failed", passed, failed))
os.exit(failed == 0 and 0 or 1)

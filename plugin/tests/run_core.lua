local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local model = require "vtabs.model"

local test, eq = H.test, H.eq
local palette = H.palette

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
  local ordered = model.ordered {
    { tab_id = 1, index = 1, is_pinned = true },
    { tab_id = 2, index = 2, is_pinned = false },
    { tab_id = 3, index = 3, is_pinned = false },
  }
  eq(ordered[1].tab_id, 1)
  eq(ordered[2].tab_id, 2)
  eq(ordered[3].tab_id, 3)
end)

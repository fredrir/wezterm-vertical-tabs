local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local hit = require "vtabs.hit"
local model = require "vtabs.model"

local test, eq, usub, strip, row_text = H.test, H.eq, H.usub, H.strip, H.row_text
local palette, items, view = H.palette, H.items, H.view

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
  eq(r.hits[8].kind, "space", "a page row above the ghost, so it is as far from the last card as cards are")
  for row = 9, 11 do
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
  eq(usub(b, 26, 26), "x", "glyph sits one col inside the card edge")
  eq(hit.span(plain.hits[4], 26), "close", "active card shows close")
  eq(hit.span(plain.hits[7], 26), nil, "idle card does not")
  local meta = render.render(view { opts = { meta = "auto" }, hover = { x = 5, y = 4 } })
  eq(meta.hits[5].part, "meta")
  eq(hit.span(meta.hits[5], 26), "close", "and on the meta row when there is one")
end)

test("unseen marker survives hover and always-close", function()
  local r = render.render(view { hover = { x = 5, y = 7 }, opts = { close_button = "always" } })
  eq(usub(row_text(r.data, 7), 2, 2), "•", "unseen dot survives in the gutter")
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
  eq(r.scroll, 85, "clamped to max_scroll")
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

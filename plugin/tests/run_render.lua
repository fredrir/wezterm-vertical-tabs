local H = require "support.helpers"
local util = require "vtabs.util"
local config = require "vtabs.config"
local ansi = require "vtabs.ansi"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local state = require "vtabs.state"
local hit = require "vtabs.hit"
local glyphs = require "vtabs.glyphs"
local sidebar = require "vtabs.sidebar"

local test, eq, usub, strip, row_text = H.test, H.eq, H.usub, H.strip, H.row_text
local frame_rows, p1_view, attach_all, mark_ready = H.frame_rows, H.p1_view, H.attach_all, H.mark_ready
local window = H.window

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
  eq(usub(rows[1], 4, 4), "~", "icon at icon_x")
  eq(usub(rows[1], 6, 13), "dotfiles", "title at title_x1")
  eq(usub(rows[4], 4, 4), "v", "the icon rides the centred title row")
  eq(usub(rows[4], 2, 2), " ", "theme.title_active is hue-distinct here, so no bar")
  eq(usub(rows[3], 2, 2), " ", "and a pad row carries nothing at all")
  local metaed = frame_rows(p1_view { opts = { separator = "gap", meta = "auto" } })
  eq(usub(metaed[5], 6, 25), "~/p/wez-plugins     ", "meta at meta_x1, elided in the middle")
  local wide = frame_rows(p1_view { opts = { width = 40, separator = "gap" }, cols = 40 })
  eq(usub(wide[1], 6, 13), "dotfiles", "title column does not move with width")
end)

test("P1 chamfer: the card's own first and last row, right side only", function()
  local rows = frame_rows(p1_view { rows = 13, opts = { separator = "gap" } })
  eq(usub(rows[4], 27, 27), " ", "the active card is square")
  local hovered_rows = frame_rows(p1_view { rows = 13, hover = { x = 5, y = 7 }, opts = { separator = "gap" } })
  eq(usub(hovered_rows[6], 27, 27), "▙", "a hovered card chamfers on its first row")
  eq(usub(hovered_rows[7], 27, 27), " ", "never on the title row, which is no longer an edge")
  eq(usub(hovered_rows[8], 27, 27), "▛", "and closes on its last")
  assert(usub(rows[4], 2, 2) ~= "▙", "col 2 is the gutter, never a chamfer")
  local one_row =
    frame_rows(p1_view { rows = 12, hover = { x = 5, y = 3 }, opts = { tab_height = "row", separator = "gap" } })
  for _, line in ipairs(one_row) do
    assert(not line:find("▙", 1, true) and not line:find("▛", 1, true), "1-row cards are square")
  end
  local dense = frame_rows(p1_view { rows = 13, hover = { x = 5, y = 1 }, opts = { separator = "gap" } })
  assert(not dense[1]:find("▙", 1, true), "a hovered dense pinned row stays square")
end)

test("P1 hits: one record per row across the whole card", function()
  local r = render.render(p1_view { hover = { x = 5, y = 4 }, opts = { separator = "gap" } })
  for _, row in ipairs { 3, 4, 5 } do
    eq(r.hits[row].kind, "tab", "row " .. row)
    eq(r.hits[row].id, 2)
    eq(r.hits[row].slot, 2)
    eq(r.hits[row].x1, 2)
    eq(r.hits[row].x2, 27)
  end
  eq(r.hits[3].part, "pad")
  eq(r.hits[4].part, "title")
  eq(r.hits[5].part, "pad")
  eq(hit.in_card(r.hits[3], 1), false, "col 1 is page, not card")
  eq(hit.in_card(r.hits[3], 28), false, "col 28 is the thumb channel")
  eq(hit.in_card(r.hits[3], 2), true)
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
  eq(usub(idle[9], 2, 2), "╭")
  eq(usub(idle[9], 27, 27), "╮")
  eq(usub(idle[10], 4, 4), "+")
  eq(usub(idle[10], 6, 13), "New tab ", "label at title_x1")
  eq(usub(idle[11], 2, 2), "╰")
  eq(usub(idle[11], 27, 27), "╯")
  for row = 9, 11 do
    eq(r.hits[row].kind, "new_tab")
    eq(r.hits[row].x1, 2)
    eq(r.hits[row].x2, 27)
  end
  eq(r.hits[8].kind, "space", "a page row above it, matching the gap between cards")
  local hover = frame_rows(p1_view { hover = { x = 5, y = 10 }, opts = { separator = "gap" } })
  for row = 9, 11 do
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
  eq(usub(idle_rows[top], 2, 2), "╭", "and the corners stay closed")
  eq(usub(idle_rows[top], 27, 27), "╮")
  eq(usub(idle_rows[top + 2], 2, 2), "╰")
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
  eq(usub(rows[2], 20, 20), "❮", "with no lights the cluster right-aligns on card_x2")
  eq(usub(rows[2], 23, 23), "+")
  eq(usub(rows[2], 26, 26), "⚙", "with the last glyph over the close column")
  eq(r.hits[1].kind, "strip")
  eq(r.hits[1].x1, nil, "strip reserve is not clickable")
  eq(r.hits[2].kind, "action")
  eq(r.hits[2].x1, 19)
  eq(r.hits[2].x2, 27)
  eq(hit.span(r.hits[2], 19), "toggle", "the cluster keeps its order, so the toggle is still first")
  eq(hit.span(r.hits[2], 21), "toggle")
  eq(hit.span(r.hits[2], 22), "new_tab", "the spans are contiguous, with no dead cell between")
  eq(hit.span(r.hits[2], 25), "settings")
  eq(hit.span(r.hits[2], 27), "settings")
  eq(hit.span(r.hits[2], 18), nil)
  local two = frame_rows(p1_view {
    strip = { rows = 3, cols = 0, toggle_row = 2 },
    opts = { separator = "gap", strip_actions = { "toggle", "new_tab" } },
  })
  eq(usub(two[2], 23, 23), "❮", "a shorter cluster still ends over the close column")
  eq(usub(two[2], 26, 26), "+")
  eq(r.hits[3].kind, "action", "the band is 2 rows and stays inside the strip")
  eq(r.hits[4].kind, "tab", "the list starts below the strip")
  local right = frame_rows(p1_view {
    strip = { rows = 2, cols = 0, toggle_row = 1 },
    opts = { position = "right", separator = "gap" },
  })
  eq(usub(right[1], 2, 2), "❯", "position=right mirrors the cluster to card_x1 and flips the glyph")
  eq(usub(right[1], 5, 5), "+")
end)

test("addendum 2 A8a: every action column derives from the reserve, whatever it measures", function()
  -- 70 px of buttons is 9 cells at the default macOS font and 10 at the next cell width up
  for _, reserve in ipairs { 9, 10 } do
    local v = p1_view {
      strip = { rows = 4, cols = reserve, toggle_row = 1 },
      opts = {
        separator = "gap",
        strip_actions = { "toggle", "new_tab", "settings", "search" },
        hooks = { search = function() end },
      },
    }
    local rows, r = frame_rows(v)
    local base = reserve + 2
    eq(usub(rows[1], base, base), "❮", reserve .. ": first glyph two columns clear of the last light")
    eq(usub(rows[1], base + 3, base + 3), "+")
    eq(usub(rows[1], base + 6, base + 6), "⚙")
    local spans = r.hits[1].spans
    eq(#spans, 4)
    eq(spans[1].x1, base - 1, reserve .. ": the first span opens on the reserve's last column")
    eq(spans[1].x2, base + 1)
    eq(spans[2].x1, base + 2, reserve .. ": contiguous and non-overlapping")
    eq(spans[4].x2, base + 10)
    for _, row in ipairs { 1, 2 } do
      eq(r.hits[row].kind, "action", reserve .. ": both reserved rows take the click")
    end
    eq(r.hits[3].kind, "strip", reserve .. ": the alignment row does not")

    local bare = p1_view {
      strip = { rows = 4, cols = reserve, toggle_row = 1 },
      opts = { separator = "gap", strip_actions = { "toggle", "new_tab", "settings", "search" } },
    }
    local _, dropped = frame_rows(bare)
    eq(#dropped.hits[1].spans, 3, reserve .. ": search is not drawn without a hook to answer it")
    eq(dropped.hits[1].spans[3].id, "settings", reserve .. ": but settings opens the page on its own")
  end
end)

test("addendum 2 A8d: hovering one action lights only its own three columns", function()
  local base = p1_view { strip = { rows = 3, cols = 0, toggle_row = 2 }, opts = { separator = "gap" } }
  local idle = render.render(base)
  local lit = render.render(p1_view {
    strip = { rows = 3, cols = 0, toggle_row = 2 },
    hover = { x = 26, y = 2 },
    opts = { separator = "gap" },
  })
  assert(not idle.rows[2]:find(ansi.bg(base.theme.hover_bg), 1, true), "nothing is lit while the pointer is away")
  local body = strip(lit.rows[2])
  eq(usub(body, 26, 26), "⚙", "the hovered glyph is still its own")
  local plan = require("vtabs.layout").plan(p1_view {
    strip = { rows = 3, cols = 0, toggle_row = 2 },
    hover = { x = 26, y = 2 },
    opts = { separator = "gap" },
  })
  eq(plan.rows[2].lit_id, "settings", "and only that action is lit")
  eq(plan.rows[3].lit_id, "settings", "on both rows of the band")
  local off = require("vtabs.layout").plan(p1_view {
    strip = { rows = 3, cols = 0, toggle_row = 2 },
    hover = { x = 18, y = 2 },
    opts = { separator = "gap" },
  })
  eq(off.rows[2].lit_id, nil, "a column between the cluster and the list lights nothing")
end)

test("addendum 2 §8: the rail keeps only what fits, centred", function()
  local narrow = p1_view { rows = 16, cols = 5, opts = { separator = "gap", width = 8 } }
  narrow.rail = true
  narrow.strip = { rows = 2, cols = 0, toggle_row = 1 }
  local rows, r = frame_rows(narrow)
  eq(usub(rows[1], 3, 3), "❮", "one action, centred at ceil(width / 2)")
  eq(#r.hits[1].spans, 1)
  local wide = p1_view {
    rows = 16,
    cols = 9,
    opts = { separator = "gap", width = 9, strip_actions = { "toggle", "new_tab" } },
  }
  wide.rail = true
  wide.strip = { rows = 2, cols = 0, toggle_row = 1 }
  local wide_rows, wr = frame_rows(wide)
  eq(#wr.hits[1].spans, 2, "nine columns hold the pair")
  eq(usub(wide_rows[1], 2, 2), "❮")
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
  eq(r.hits[12].kind, "footer", "the footer keeps the pane's last row")
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
  eq(wide.toggle_left, "❮", "the ornament is EAW Neutral, so ambiguous-as-wide leaves it alone")
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
  eq(usub(rows[1], 6, 12), "Private")
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
  eq(usub(rows[2], 2, 2), "-", "ascii rule when the box glyph is unsafe")
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
  local win, gui = window(2)
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
    hover = { x = 23, y = 1 },
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
  eq(usub(rows[ghost_top], 3, 4), "╌╌", "every cell between the corners is dashed")
  eq(usub(rows[ghost_top], 25, 26), "╌╌", "at both ends")
  eq(usub(rows[ghost_top], 5, 6), "╌╌", "and in between: the dash lives inside the glyph")
  eq(usub(rows[ghost_top + 1], 2, 2), "╎", "the sides are dashed too")
  eq(usub(rows[ghost_top + 1], 27, 27), "╎")
  eq(usub(rows[ghost_top + 2], 3, 4), "╌╌", "the bottom rail closes the same way")
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
  eq(usub(over_rows[top], 3, 4), "╌╌", "and the hovered ghost stays dashed")
end)

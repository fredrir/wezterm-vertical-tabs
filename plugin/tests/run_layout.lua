local H = require "support.helpers"
local util = require "vtabs.util"
local config = require "vtabs.config"
local anim = require "vtabs.anim"
local ansi = require "vtabs.ansi"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local state = require "vtabs.state"
local hit = require "vtabs.hit"
local glyphs = require "vtabs.glyphs"
local model = require "vtabs.model"
local sidebar = require "vtabs.sidebar"
local fake = require "fake_mux"
local backend = require "vtabs.backend"

local test, eq, usub, strip, row_text = H.test, H.eq, H.usub, H.strip, H.row_text
local frame_rows, dump_frame = H.frame_rows, H.dump_frame
local p1_items, p1_view, sidebars_in, FRAME_DIR, attach_all =
  H.p1_items, H.p1_view, H.sidebars_in, H.FRAME_DIR, H.attach_all
local mark_ready, window = H.mark_ready, H.window

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
  local footer_row = v.rows - config.get().padding.bottom
  eq(r.hits[footer_row].kind, "footer")
  eq(usub(rows[footer_row], 3, 3), "f", "and the footer keeps its icon")
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
  local win, gui = window(1)
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
  local win, gui = window(2)
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
  local win, gui = window(1)
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
  local win, gui = window(5)
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
  local win, gui = window(3)
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
    return usub(rows[4], 2, 2), usub(rows[3], 2, 2)
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
    eq(usub(rows[4], 2, 2) == "▎", case[2], case[1] .. " bar")
  end
end)

test("addendum 5: no palette loses both discriminators", function()
  local palettes = require "palettes"
  for _, p in ipairs(palettes) do
    local resolved = theme.resolve({}, p)
    local v = p1_view { opts = { separator = "gap" } }
    v.theme = resolved
    local rows = frame_rows(v)
    local bar = usub(rows[4], 2, 2) == "▎"
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
  eq(usub(frame_rows(v)[4], 2, 2), " ", "an active tab with nothing unseen keeps a blank gutter")
end)

test("addendum 5: an active tab with unseen output shows the dot once the bar is gone", function()
  local unseen_items = p1_items()
  unseen_items[2].has_unseen = true
  local v = p1_view { items = unseen_items, opts = { separator = "gap" } }
  v.theme.title_active = { 137, 180, 250 }
  local rows = frame_rows(v)
  eq(usub(rows[4], 2, 2), "•", "the freed gutter finally reaches an active tab")
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
  eq(usub(rows[first + 2], 4, 4), "v", "and the icon rides the title, not a row of its own")
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
    eq(usub(painted[title], 4, 4), "v", label .. ": the icon rides the title")
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
  eq(usub(rows[title], 6, 14), "3  claude", "the index is inline with the title")
  local metaed, mr = frame_rows(p1_view { rows = 20, opts = { separator = "gap", show_index = true, meta = "auto" } })
  local meta_row
  for row = 1, 20 do
    local h = mr.hits[row]
    if h and h.kind == "tab" and h.part == "meta" and h.id == 3 then
      meta_row = meta_row or row
    end
  end
  eq(usub(metaed[meta_row - 1], 6, 11), "claude", "with a meta line the title keeps its own row")
  assert(usub(metaed[meta_row], 6, 8):find "3", "and the index goes back to the meta line")
end)

test("addendum 2 A4c/§1.6d: the close glyph is in-font and measures one cell", function()
  local icons_mod = require "vtabs.icons"
  -- the stub has no nerdfonts table, so the default resolves to its ASCII fallback; on a real
  -- install it is nf-md-close_thick, which is in the cmap where U+2716 never was
  eq(icons_mod.defaults.close, "x", "the fallback is ASCII, never another font's glyph")
  local resolved = glyphs.resolve(config.setup({ backend = { path = "/bin/wez-vtabs" } }).glyphs, {})
  eq(util.width(resolved.close), 1, "one column, so the ASCII guard never fires")
  local swapped =
    glyphs.resolve(config.setup({ icon_map = { close = "Z" }, backend = { path = "/bin/wez-vtabs" } }).glyphs, {})
  eq(swapped.close, "Z", "and icon_map still reaches it")
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
    local x1, x2 = 2, cols - 1
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
  eq(l.grid.card_x1, 2)
  eq(l.grid.card_x2, 27)
  eq(l.grid.icon_x, 4)
  eq(l.grid.title_x1, 6)
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
      strip = { rows = 2, cols = 9, toggle_row = 1 },
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

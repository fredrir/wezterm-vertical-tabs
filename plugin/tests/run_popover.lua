local H = require "support.helpers"
local util = require "vtabs.util"
local config = require "vtabs.config"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local input = require "vtabs.input"
local popover = require "vtabs.popover"
local fake = require "fake_mux"
local view_mod = require "vtabs.view"
local palettes = require "palettes"

local test, eq, rgb, hex, title_row = H.test, H.eq, H.rgb, H.hex, H.title_row
local popover_row, mouse, window = H.popover_row, H.mouse, H.window
local ready_window, open_popover, here = H.ready_window, H.open_popover, H.here

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
  -- The frame is no excuse: wezterm skips only the pane's default fill under a background layer and
  -- goes on dimming explicit-bg cells, which is every cell the sidebar paints. Without this the
  -- sidebar rendered (38,38,52) against a frame tint of (41,41,58).
  local zen = hsb { frame = "zen", backend = { path = "/bin/wez-vtabs" } }
  eq(zen.brightness, 1.0, "a background layer does not spare the sidebar's own cells")
  eq(zen.saturation, 1.0)
  local mine = { brightness = 0.5, saturation = 0.5 }
  eq(hsb({}, mine), mine, "a user value is never overwritten")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("cell counts and durations must be whole numbers", function()
  eq(config.setup({ width = 28.7 }).width, 28, "a fractional width would reach AdjustPaneSize")
  eq(config.setup({ row_gap = 1.5 }).row_gap, 0)
  eq(config.setup({ rail_width = 5.5 }).rail_width, 5)
  eq(config.setup({ poll_ms = 500.5 }).poll_ms, 500)
  eq(config.setup({ padding = { top = 1.2 } }).padding.top, 0)
  eq(config.setup({ tooltip_delay_ms = 600.5 }).tooltip_delay_ms, 600)
  eq(config.setup({ animation = { fps = 30.5 } }).animation.fps, 30)
  eq(config.setup({ width = 32 }).width, 32, "a whole number survives")
  eq(config.setup({ theme = { elevation = 0.06 } }).theme.elevation, 0.06, "ratios still take fractions")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

---A foreground process the skip list does not name is what makes a close want confirming.
local function make_busy(tab)
  for _, p in ipairs(tab.pane_list) do
    if not sidebar.is_backend(p) then
      p.process = "/usr/bin/sleep"
    end
  end
end

test("the ✕ on a busy tab asks in the sidebar, and closes only when Close is chosen", function()
  local win, gui = ready_window()
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
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  local close_row = title_row(sb, win.tab_list[1].id)
  mouse(gui, sb, "down", "left", 26, close_row)
  mouse(gui, sb, "up", "left", 26, close_row)
  eq(popover.get(gui:window_id()), nil, "zsh is on the skip list")
  eq(#win.tab_list, 2)

  local opted, opted_gui = ready_window()
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
  local win, gui = ready_window()
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
  local win, gui = ready_window()
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
  local win, gui = ready_window()
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
  local win, gui = ready_window()
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
  eq(popover.width_for(cfg, 22, menu, nil), 20, "a narrower one caps it at the columns it can spare")
  local fixed = util.merge(cfg, { popover = { width = 18 } })
  eq(popover.width_for(fixed, 80, menu, nil), 18, "a number is taken verbatim")
  eq(popover.width_for(util.merge(cfg, { popover = { width = 4 } }), 80, menu, nil), 16, "with a floor")
  eq(popover.width_for(cfg, 80, menu, { string.rep("x", 40) }), 45, "a header can widen it too")
  eq(config.setup({ popover = { width = "wide" } }).popover.width, "auto", "a bad value resets")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the menu shows the key bound to an item, which no binding ever carried before", function()
  local bindings = keys.build {}
  local named = 0
  for _, b in ipairs(bindings) do
    if b.vtabs then
      named = named + 1
    end
  end
  eq(named, #bindings, "every binding names itself, or the hints have nothing to match on")

  local win, gui = window(1)
  local menu = popover.items(gui, win.tab_list[1].id)
  local close = nil
  for _, item in ipairs(menu) do
    if item.id == "close" then
      close = item
    end
  end
  assert(close, "the menu offers Close tab")
  assert(close.hint and close.hint ~= "", "and shows the key bound to it, got " .. tostring(close.hint))
  assert(close.hint:find "W", "which is the close_tab binding: " .. close.hint)

  -- wezterm rejects a key entry carrying a field it does not know, so the name never reaches it.
  local cfg = { keys = {} }
  keys.apply(cfg, config.setup {})
  assert(#cfg.keys > 0)
  for _, b in ipairs(cfg.keys) do
    eq(b.vtabs, nil, "no binding handed to wezterm carries the name")
  end
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the pointer never moves the confirm level's selection away from Cancel", function()
  local win, gui, sb = open_popover(3)
  make_busy(win.tab_list[1])
  popover.run(gui, "close")
  local pop = popover.get(gui:window_id())
  eq(pop.level, "confirm")
  eq(pop.index, 2, "Cancel")
  view_mod.sync(gui, { force = true })
  local before = #win.tab_list
  for row, h in pairs(state.session.hits[sb:pane_id()]) do
    if h.kind == "popover" and h.id then
      mouse(gui, sb, "move", "none", h.x1 + 2, row)
    end
  end
  eq(popover.get(gui:window_id()).index, 2, "a pointer resting on Close never arms it")
  eq(#win.tab_list, before)
  popover.close(gui)
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
  local win, gui = ready_window()
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

local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
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
local view_mod = require "vtabs.view"
local palettes = require "palettes"
local platform = require "vtabs.platform"
local page = require "vtabs.page"

local test, eq, usub, row_text, rgb = H.test, H.eq, H.usub, H.row_text, H.rgb
local hex, page_rows, dump_lines, legacy, p1_view = H.hex, H.page_rows, H.dump_lines, H.legacy, H.p1_view
local page_view, attach_all, mark_ready, window, RETINA = H.page_view, H.attach_all, H.mark_ready, H.window, H.RETINA
local here, mouse = H.here, H.mouse

test("P3 A1: the settings page carries the marker and is never taken for a sidebar", function()
  local settings = require "vtabs.settings"
  local win, gui = window(1)
  attach_all(win, gui)
  mark_ready(win.tab_list[1])
  local tab = settings.open(gui)
  assert(tab, "the page opened")
  local found, pane = settings.find(win)
  eq(found, tab, "and find() reaches the same tab")
  assert(pane:get_title():match "^wez%-vtabs%-settings:%x+$", "A1a: the settings marker")
  eq(sidebar.is_settings(pane), true, "A1b")
  eq(sidebar.is_backend(pane), false, "A1b: is_backend is false for the very same pane")
  eq(sidebar.marker(pane:get_title()), true, "A1d: so the marker never leaks into a tab title")
  eq(sidebar.has_marker(pane), false, "and the adoption path will not take it")

  -- A1c: a tab holding only the page has no sidebar at all, and the page is its content
  local bare = win:add_tab { process = "/bin/zsh" }
  bare.pane_list[1].title = "wez-vtabs-settings:deadbeef"
  local content, sb = sidebar.classify(bare)
  eq(sb, nil, "A1c: no pane wins the sidebar contest")
  eq(#content, 1, "A1c: the page is content")
  eq(sidebar.find(bare), nil)
  eq(sidebar.is_backend(bare.pane_list[1]), false, "A1c: rank none")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 A1d/A2a: the page is one card called Settings, and one tab per window", function()
  local settings = require "vtabs.settings"
  local win, gui = window(1)
  attach_all(win, gui)
  mark_ready(win.tab_list[1])
  local first = settings.open(gui)
  local count = #win.tab_list
  local again = settings.open(gui)
  eq(again, first, "A2a: the second call activates the page instead of spawning another")
  eq(#win.tab_list, count, "and no tab was added")

  local listed = model.build(gui)
  local card
  for _, item in ipairs(listed) do
    card = item.tab_id == first:tab_id() and item or card
  end
  assert(card, "A1d: the page has a card")
  eq(card.title, "Settings")
  eq(card.icon, config.get().glyphs.settings, "A1d: with the cog")
  eq(card.meta, nil, "and no meta line to probe for")
  assert(not card.title:find("wez-vtabs", 1, true), "A1d: never the raw marker")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 A2b: escape closes the settings tab, not whichever tab is active", function()
  local settings = require "vtabs.settings"
  local win, gui = window(2)
  attach_all(win, gui)
  for _, tab in ipairs(win.tab_list) do
    mark_ready(tab)
  end
  local page_tab = settings.open(gui)
  local page_id = page_tab:tab_id()
  local pane = select(2, settings.find(win))
  -- the page authenticates over the same bridge, so it echoes the token like any other backend
  pane.vars.vtabs_token = state.token_for(pane:pane_id())
  local other = win.tab_list[1]
  win.active_tab_ref = other
  local before = #win.tab_list
  input.handle(gui, pane, "vtabs", '{"t":"key","key":"escape"}')
  eq(#win.tab_list, before - 1, "one tab closed")
  eq(settings.find(win), nil, "and it was the page")
  for _, tab in ipairs(win.tab_list) do
    assert(tab:tab_id() ~= page_id, "the page is gone")
  end
  assert(actions.tab_by_id(gui, other:tab_id()), "A2b: the tab that was active survived")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 A2c: every trigger reaches actions.open_settings, and only one place spawns the page", function()
  local settings = require "vtabs.settings"
  local win, gui = window(1)
  attach_all(win, gui)
  mark_ready(win.tab_list[1])
  local calls = 0
  local original = settings.open
  settings.open = function(gui_window)
    calls = calls + 1
    return original(gui_window)
  end

  actions.open_settings(gui)
  eq(calls, 1, "the action itself")
  local binding
  for _, entry in ipairs(keys.build(config.get().keys)) do
    binding = entry.key == "," and entry or binding
  end
  assert(binding, "there is a settings binding")
  eq(binding.mods, platform.SUPER, "CMD+, on macOS, CTRL+SHIFT+, everywhere else")
  binding.action.callback(gui)
  eq(calls, 2, "the key binding")
  popover.open(gui, win.tab_list[1]:tab_id(), 1)
  popover.run(gui, "settings")
  eq(calls, 3, "and the popover item")

  -- The strip paints a ⚙ by default, so clicking it has to reach the page like everything else.
  local sb = sidebar.find(win.tab_list[1])
  local function click_settings()
    view_mod.sync(gui, { force = true })
    for y, h in pairs(state.session.hits[sb:pane_id()] or {}) do
      for _, span in ipairs(h.spans or {}) do
        if span.id == "settings" then
          mouse(gui, sb, "down", "left", span.x1, y)
          return true
        end
      end
    end
    return false
  end
  assert(click_settings(), "the strip paints a settings button by default")
  eq(calls, 4, "and the strip button")

  -- A hook is what points it somewhere else; the built-in is only the default destination.
  local hooked = 0
  config.setup {
    meta = "auto",
    backend = { path = "/bin/wez-vtabs" },
    hooks = {
      settings = function()
        hooked = hooked + 1
      end,
    },
  }
  assert(click_settings(), "still painted with a hook set")
  eq(hooked, 1, "the hook ran")
  eq(calls, 4, "and the page was not opened behind it")

  settings.open = original
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 frames are written for design review", function()
  for _, size in ipairs { { 100, 21 }, { 60, 18 } } do
    local pv = page_view { cols = size[1], rows = size[2], st = { group = 2, focus = 4 } }
    local lines = page_rows(pv)
    dump_lines("settings-" .. size[1], lines, size[1])
    for row = 1, size[2] do
      eq(util.width(lines[row]), size[1], size[1] .. "-col row " .. row)
    end
  end
end)

test("P3 A3a/A3b: the nav is the descriptors' own groups and every option gets a widget", function()
  local fields = page.fields(config.setup { backend = { path = "/bin/wez-vtabs" } })
  local groups = page.groups(fields)
  assert(#groups >= 8, "one nav entry per declared group, got " .. #groups)
  local seen = {}
  for _, g in ipairs(groups) do
    assert(not seen[g], "no group twice")
    seen[g] = true
  end
  eq(groups[1], "layout", "A3a: reading order, not alphabetical")

  local by_key = {}
  for _, row in ipairs(fields) do
    by_key[row.key] = row
    assert(row.widget ~= nil, row.key .. " has a widget")
    assert(row.widget ~= "text" or type(row.value) == "string", row.key .. " renders as text only if it is one")
  end
  eq(by_key.icons.widget, "toggle", "A3b: boolean")
  eq(by_key.corners.widget, "picker", "enum")
  eq(by_key.width.widget, "stepper", "number")
  eq(by_key.new_tab_label.widget, "text", "string")
  eq(by_key.icon_map.widget, "entries", "an open container is counted, not typed into")
  assert(by_key["padding.left"], "container children get their own rows")

  -- A3a: a descriptor with a new group grows the nav with no edit here
  local schema_mod = require "vtabs.schema"
  local added_option = { key = "made_up_probe", type = "boolean", default = false, group = "probe", label = "P" }
  schema_mod.options[#schema_mod.options + 1] = added_option
  schema_mod.by_key[added_option.key] = added_option
  local grown = page.groups(page.fields(config.setup { backend = { path = "/bin/wez-vtabs" } }))
  eq(grown[#grown], "probe", "A3a: the nav grew on its own")
  schema_mod.options[#schema_mod.options] = nil
  schema_mod.by_key[added_option.key] = nil
end)

test("P3 A3c: a stepper and a picker cannot leave the bounds the descriptor declares", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local by_key = {}
  for _, row in ipairs(page.fields(cfg)) do
    by_key[row.key] = row
  end
  local width = by_key.width
  local floor = width.option.min
  width.value = floor
  eq(page.step(width, -1), floor, "A3c: a stepper stops at min")
  local capped = by_key["theme.elevation"] or by_key.width
  if capped.option and capped.option.max then
    capped.value = capped.option.max
    eq(page.step(capped, 1), capped.option.max, "and at max")
  end
  width.value = 28
  eq(page.step(width, 1), 29)
  eq(page.step(width, -1), 27)

  local corners = by_key.corners
  local enum = corners.option.enum
  corners.value = enum[#enum]
  eq(page.step(corners, 1), enum[1], "A3c: a picker wraps inside its own enum")
  corners.value = enum[1]
  eq(page.step(corners, -1), enum[#enum])
  for _, value in ipairs { page.step(corners, 1), page.step(corners, -1) } do
    assert(util.contains(enum, value), "never a value outside the enum")
  end
  eq(page.step(by_key.icons, 1), not by_key.icons.value, "a toggle flips")
end)

test("P3 A4a: a key set in wezterm.lua is locked with its source, and marked changed otherwise", function()
  local cfg = config.setup { width = 32, backend = { path = "/bin/wez-vtabs" } }
  local by_key = {}
  for _, row in ipairs(page.fields(cfg)) do
    by_key[row.key] = row
  end
  eq(by_key.width.locked, "wezterm.lua", "A4a")
  eq(by_key.width.changed, true, "and it does differ from the default")
  eq(by_key.corners.locked, nil, "an untouched key is editable")
  eq(by_key.corners.changed, false)
  eq(by_key["hooks.footer"] and by_key["hooks.footer"].locked, nil, "no hook is set here")

  -- A4b: a wezterm key the host set outright
  config.host_config = { window_padding = { left = 8 } }
  local hosted = {}
  for _, row in ipairs(page.fields(config.setup { backend = { path = "/bin/wez-vtabs" } })) do
    hosted[row.key] = row
  end
  eq(hosted.edge_to_edge.locked, "wezterm.lua (host)", "A4b: named as the host's")
  eq(page.apply_mode(hosted.edge_to_edge), "locked", "so the page cannot write it")
  config.host_config = {}
  local free = {}
  for _, row in ipairs(page.fields(config.setup { backend = { path = "/bin/wez-vtabs" } })) do
    free[row.key] = row
  end
  eq(free.edge_to_edge.locked, nil)
  eq(page.apply_mode(free.corners), "instant", "most keys are ours to swap")
  eq(page.apply_mode(free.edge_to_edge), "reload", "and a few only exist while apply_to_config runs")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("P3 A7: copy as Lua reproduces exactly the non-default set", function()
  local cfg = config.setup { width = 32, meta = "cwd", theme = { accent = "#f5c2e7" } }
  local text = page.as_lua(cfg)
  assert(text:find("vtabs.apply_to_config(config, {", 1, true), "a paste-ready call")
  assert(text:find("width = 32", 1, true), text)
  assert(text:find('meta = "cwd"', 1, true), text)
  assert(text:find('accent = "#f5c2e7"', 1, true), "nested tables inline: " .. text)
  assert(not text:find("row_gap", 1, true), "and nothing still at its default")
  eq(page.as_lua(config.setup { backend = { path = "/bin/wez-vtabs" } }):find "backend" ~= nil, true, "opts show up")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("P3 A2/§2: the page has two breakpoints and nothing else", function()
  local narrow = page_rows(page_view { cols = 40, rows = 12 })
  local said = false
  for _, line in ipairs(narrow) do
    said = said or line:find("Settings needs 48 columns", 1, true) ~= nil
  end
  assert(said, "under 48 columns the page says so and draws nothing else")

  local mid, mid_out = page_rows(page_view { cols = 60, rows = 18 })
  eq(page.grid(60).preview, false, "48 to 89 is nav plus form")
  assert(mid[1]:find("Settings", 1, true), "with a header")
  for row = 1, 18 do
    eq(util.width(mid[row]), 60, "60-col row " .. row)
  end
  eq(mid_out.hits[4].kind, "body", "and rows carry hit records")
  assert(mid_out.hits[4].nav and mid_out.hits[4].id, "with both columns on them")

  local wide, out = page_rows(page_view { cols = 100, rows = 21 })
  eq(page.grid(100).preview, true, "90 and up adds the preview box")
  for row = 1, 21 do
    eq(util.width(wide[row]), 100, "100-col row " .. row)
  end
  local g = page.grid(100)
  eq(g.nav_x1, 2)
  eq(g.nav_x2, 19)
  eq(g.divider, 20)
  eq(g.caret_x, 21)
  eq(g.label_x, 23)
  eq(g.value_x2, 64)
  eq(g.marker_x, 66)
  eq(g.preview_x1, 69)
  eq(g.preview_x2, 100)
  eq(usub(wide[4], g.divider, g.divider), "│", "the divider runs down the body")
  eq(usub(wide[4], g.preview_x1, g.preview_x1), "╭", "and the preview box is drawn")
  eq(out.hits[1].kind, "chrome", "the header is not a target")
end)

test("P3 §2: the nav selects, the caret follows the focus, and hits name the field", function()
  local v = page_view { cols = 100, rows = 21, st = { group = 2, focus = 4 } }
  local rows, out = page_rows(v)
  local nav_rows, field_rows = 0, 0
  for row = 1, v.rows do
    local h = out.hits[row]
    nav_rows = h.nav ~= nil and nav_rows + 1 or nav_rows
    field_rows = h.id ~= nil and field_rows + 1 or field_rows
  end
  assert(nav_rows > 0 and field_rows > 0, "the body carries both kinds of target")
  local g = page.grid(100)
  local caret_row
  for row = 1, v.rows do
    if usub(rows[row], g.caret_x, g.caret_x) == v.glyphs.focus then
      caret_row = caret_row or row
    end
  end
  eq(caret_row, 4 + 3, "the caret marks the focused field")
  local focused = out.hits[caret_row]
  eq(focused.kind, "body")
  eq(hit.span(focused, g.value_x2), "inc", "the value column carries its own sub-targets")
  eq(hit.span(focused, g.caret_x), "field", "the label is the row itself")
  eq(hit.span(focused, g.nav_x1), "nav", "and the nav column is the nav, on the very same row")

  local selected
  for row = 1, v.rows do
    local h = out.hits[row]
    if h.nav and usub(rows[row], g.nav_x1, g.nav_x1) == v.glyphs.active then
      selected = h.nav
    end
  end
  eq(selected, page.groups()[2], "the marker is on the selected group")
end)

test("P3 §6: the preview renders the merged table and never touches the live config", function()
  local v = page_view { cols = 100, rows = 21, pending = { new_tab_label = "PENDING" } }
  local before = config.get().new_tab_label
  local rows = page_rows(v)
  local found = false
  for _, line in ipairs(rows) do
    found = found or line:find("PENDING", 1, true) ~= nil
  end
  assert(found, "the pending edit shows in the preview")
  eq(config.get().new_tab_label, before, "A6b: and the live config is untouched")

  local plain = page_rows(page_view { cols = 100, rows = 21 })
  local same = true
  for i = 1, #rows do
    same = same and rows[i] == plain[i]
  end
  assert(not same, "so the preview really did change")
end)

test("P3 A3d: the recorder arms, shows the caveat only on macOS, and takes the next key", function()
  local platform_mod = require "vtabs.platform"
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local keys_group
  for i, group in ipairs(page.groups(page.fields(cfg))) do
    keys_group = group == "behaviour" and i or keys_group
  end
  local st = { group = keys_group, focus = 1, scroll = 0, filter = "keys." }
  local v = page_view { cols = 100, rows = 21, cfg = cfg, st = st }

  local idle = page.plan(v)
  local first = idle.fields[1]
  assert(first and first.widget == "recorder", "the filter found a keys.* row: " .. tostring(first and first.key))
  assert(page.value_text(first):find("[ record ]", 1, true), "idle: " .. page.value_text(first))
  local rows = page_rows(v)
  for _, line in ipairs(rows) do
    assert(not line:find("does not deliver CMD", 1, true), "A3d: no caveat while nothing is armed")
  end

  -- arming is what Enter does to a recorder
  page.key(nil, v, { key = "enter" })
  eq(st.armed, first.key, "Enter arms the recorder")
  page_rows(v)
  local armed_text = page.value_text(page.plan(v).fields[1])
  assert(armed_text:find("ARMED", 1, true), "armed: " .. armed_text)

  local was_mac = platform_mod.is_mac
  platform_mod.is_mac = true
  st.armed = first.key
  local mac_rows = page_rows(v)
  local said = false
  for _, line in ipairs(mac_rows) do
    said = said or line:find("does not deliver CMD", 1, true) ~= nil
  end
  assert(said, "A3d: on macOS the caveat is shown while armed")
  local named = false
  for _, line in ipairs(mac_rows) do
    named = named or line:find("enable_kitty_keyboard", 1, true) ~= nil
  end
  assert(named, "and enable_kitty_keyboard is named, not offered")

  platform_mod.is_mac = false
  st.armed = first.key
  local linux_rows = page_rows(v)
  for _, line in ipairs(linux_rows) do
    assert(not line:find("does not deliver CMD", 1, true), "A3d: and never where it is not true")
  end
  platform_mod.is_mac = was_mac

  -- the recorder records what the pty delivered, not what the user meant
  st.armed = first.key
  page.key(nil, v, { key = "z", mods = { "CTRL", "SHIFT" } })
  eq(st.armed, nil, "the next key disarms it")
  eq(config.get().keys[first.key:match "^keys%.(.+)$"].key, "z", "and is taken as the binding")
  eq(config.get().keys[first.key:match "^keys%.(.+)$"].mods, "CTRL|SHIFT", "mods come off the array the wire sends")
  st.armed = first.key
  page.key(nil, v, { key = "escape" })
  eq(st.armed, nil, "escape disarms without recording")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 §3/§4: the badge names the source, and r resets exactly the focused field", function()
  local cfg = config.setup { width = 32, backend = { path = "/bin/wez-vtabs" } }
  local v = page_view { cols = 100, rows = 21, cfg = cfg, st = { group = 1, focus = 1, scroll = 0 } }
  local rows, out = page_rows(v)
  local function row_for(key, painted, plan_out)
    for row = 1, 21 do
      if plan_out.hits[row].id == key then
        return painted[row]
      end
    end
  end
  local focused = row_for("width", rows, out)
  assert(focused, "the width row is drawn")
  assert(focused:find("LOCKED wezterm.lua", 1, true), "§4: the focused row names the source: " .. focused)
  assert(not focused:find("(host)", 1, true), "and not as the host's")

  -- every other locked row says LOCKED and shows its value; only one row is ever being asked about
  local elsewhere = page_view { cols = 100, rows = 21, cfg = cfg, st = { group = 1, focus = 3, scroll = 0 } }
  local other_rows, other_out = page_rows(elsewhere)
  local unfocused = row_for("width", other_rows, other_out)
  assert(unfocused:find("LOCKED", 1, true), "still badged: " .. unfocused)
  assert(not unfocused:find("wezterm.lua", 1, true), "but the reason is the focused row's job: " .. unfocused)
  assert(unfocused:find("32", 1, true), "and the value is shown instead: " .. unfocused)
  local help = other_rows[19]
  assert(help:find("—", 1, true), "the descriptor's label and help sit in the hint area: " .. help)

  config.host_config = { window_padding = { left = 8 } }
  local host_cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local host_st = { group = 1, focus = 1, scroll = 0 }
  for i, row in ipairs(page.plan(page_view { cols = 100, rows = 21, cfg = host_cfg, st = host_st }).fields) do
    host_st.focus = row.key == "edge_to_edge" and i or host_st.focus
  end
  local hosted, hosted_out = page_rows(page_view { cols = 100, rows = 21, cfg = host_cfg, st = host_st })
  local host_line = row_for("edge_to_edge", hosted, hosted_out)
  assert(
    host_line and host_line:find("wezterm.lua (host)", 1, true),
    "§4: named as the host's: " .. tostring(host_line)
  )
  config.host_config = {}

  -- r resets the focused field and nothing else, and the changed marker goes with it
  local edited = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local st = { group = 2, focus = 1, scroll = 0 }
  local ev = page_view { cols = 100, rows = 21, cfg = edited, st = st }
  local plan = page.plan(ev)
  local target, other
  for i, row in ipairs(plan.fields) do
    if row.widget == "picker" and not row.locked and not target then
      target, st.focus = row, i
    elseif row.widget == "picker" and not row.locked and not other then
      other = row
    end
  end
  assert(target and other, "two pickers to tell apart")
  page.key(nil, ev, { key = "rightarrow" })
  page.key(nil, ev, { key = "rightarrow" })
  local moved = page.plan(page_view { cols = 100, rows = 21, cfg = config.get(), st = st })
  eq(moved.fields[st.focus].changed, true, "the focused field moved off its default")
  local marker_col = page.grid(100).marker_x
  local after = page_rows(page_view { cols = 100, rows = 21, cfg = config.get(), st = st })
  local marked = 0
  for _, line in ipairs(after) do
    marked = usub(line, marker_col, marker_col) == ev.glyphs.unseen and marked + 1 or marked
  end
  eq(marked, 1, "exactly one row carries the changed marker")

  page.key(nil, ev, { key = "r" })
  local reset = page.plan(page_view { cols = 100, rows = 21, cfg = config.get(), st = st })
  eq(reset.fields[st.focus].changed, false, "r puts it back on the schema default")
  eq(reset.fields[st.focus].value, target.default)
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 §3: a type = any key fronts an enum, and custom is shown but not stepped", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local by_key = {}
  for _, row in ipairs(page.fields(cfg)) do
    by_key[row.key] = row
  end
  local frame = by_key.frame
  assert(frame, "frame has a row")
  eq(frame.widget, "variant", "a boolean toggle could never express its table form")
  eq(page.variant_name(frame), "off")
  eq(page.value_text(frame), "‹ off ›")
  eq(page.step(frame, 1), true, "and it steps through its presets")

  local tabled = config.setup { frame = { margin = 1, corners = true }, backend = { path = "/bin/wez-vtabs" } }
  local rows = {}
  for _, row in ipairs(page.fields(tabled)) do
    rows[row.key] = row
  end
  eq(page.variant_name(rows.frame), "custom", "a table no preset describes reads as custom")
  eq(page.value_text(rows.frame), "‹ custom ›")
  eq(page.step(rows.frame, 1), rows.frame.value, "and an arrow key cannot flip it away")
  eq(page.WIDGETS.variant.activate(rows.frame), nil, "nor can Enter")
  assert(rows["frame.margin"], "its keys are listed below it instead")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("P3 §2: the hint bar and the preview icons go through the glyph guard", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local safe = glyphs.resolve(cfg.glyphs, {})
  local wide = glyphs.resolve(cfg.glyphs, { treat_east_asian_ambiguous_width_as_wide = true })
  for _, key in ipairs { "hint_up", "hint_down", "hint_left", "hint_right" } do
    eq(util.width(safe[key]), 1, key .. " is one column")
    eq(util.width(wide[key]), 1, key .. " stays one column under ambiguous-as-wide")
  end
  eq(wide.hint_up, "^", "the arrows are East Asian Ambiguous, so the flag substitutes them")
  eq(wide.hint_left, "<")

  local v = page_view { cols = 100, rows = 21 }
  local bar = page_rows(v)[21]
  assert(bar:find("Enter edit", 1, true), "Enter is spelled out, not U+23CE: " .. bar)
  assert(not bar:find("⏎", 1, true), "which is in barely any monospace font")
  local narrow = page_rows(page_view { cols = 60, rows = 18 })[18]
  assert(narrow:find("Enter", 1, true), "narrow too: " .. narrow)

  local ascii = page_view { cols = 100, rows = 21 }
  ascii.glyphs = wide
  local ascii_bar = page_rows(ascii)[21]
  assert(ascii_bar:find("^v field", 1, true), "and the bar is composed from the guarded glyphs: " .. ascii_bar)

  -- A6a: the preview's icons come from the merged map, so an icon_map edit shows there
  local edited = page_rows(page_view { cols = 100, rows = 21, pending = { icon_map = { zsh = "Z" } } })
  local found = false
  for _, line in ipairs(edited) do
    found = found or line:find("Z zsh", 1, true) ~= nil
  end
  assert(found, "A6a: the sample icons are resolved, never literals")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("P3 §2: the 60-column layout is nav plus form, and says so under 48", function()
  local narrow = page_rows(page_view { cols = 60, rows = 18, st = { group = 1, focus = 1, scroll = 0 } })
  local g = page.grid(60)
  eq(g.preview, false, "no preview box below 90 columns")
  eq(g.nav_x1, 2)
  eq(g.nav_x2, 15)
  eq(g.divider, 16)
  eq(g.caret_x, 17)
  eq(g.label_x, 19)
  eq(g.form_x2, 58)
  assert(narrow[1]:find("Settings", 1, true), "the header")
  eq(usub(narrow[4], g.divider, g.divider), "│", "the divider runs down the body")
  eq(usub(narrow[4], g.nav_x1 + 2, g.nav_x1 + 7), "Layout", "nav on the left")
  assert(narrow[18]:find("esc", 1, true), "and the narrow hint bar is last")
  assert(not narrow[18]:find("copy as Lua", 1, true), "which is the short copy, not the wide one")
  for row = 1, 18 do
    eq(util.width(narrow[row]), 60, "60-col row " .. row)
    assert(not narrow[row]:find("╭", 1, true), "row " .. row .. " draws no preview box")
  end

  local under = page_rows(page_view { cols = 47, rows = 12 })
  local said, other = false, 0
  for _, line in ipairs(under) do
    if line:find("Settings needs 48 columns", 1, true) then
      said = true
    elseif line:match "%S" then
      other = other + 1
    end
  end
  assert(said, "under 48 it says so")
  eq(other, 0, "and draws nothing else")
  eq(page.grid(47), nil, "there is no grid to draw with")
  assert(page.grid(48), "48 is the floor, not the first refusal")
end)

test("refactor 9: the hit kinds, the changed-set walk and the path check each have one home", function()
  local settings = require "vtabs.settings"
  -- the constants are the strings, so producers and consumers cannot drift apart on a typo
  eq(hit.KIND.TAB, "tab")
  eq(hit.KIND.ACTION, "action")
  eq(hit.KIND.BODY, "body")
  local v = p1_view { rows = 20, opts = { separator = "gap" } }
  v.strip = { rows = 2, cols = 0, toggle_row = 1 }
  local r = render.render(v)
  local kinds = {}
  for row = 1, v.rows do
    kinds[r.hits[row].kind] = true
  end
  for _, kind in ipairs { hit.KIND.ACTION, hit.KIND.TAB, hit.KIND.NEW_TAB, hit.KIND.SPACE } do
    assert(kinds[kind], "the sidebar still produces " .. kind)
  end

  -- one walk answers both "what goes in the file" and "what goes on the clipboard"
  local cfg = config.setup { width = 32, theme = { accent = "#f5c2e7" }, backend = { path = "/bin/wez-vtabs" } }
  local changed = settings.changed(cfg)
  eq(changed.width, 32)
  eq(changed.theme.accent, "#f5c2e7", "nested through the same descriptor walk")
  eq(changed.row_gap, nil, "and nothing still at its default")
  eq(changed.theme.elevation, nil)
  local text = page.as_lua(cfg)
  assert(text:find("width = 32", 1, true) and text:find('accent = "#f5c2e7"', 1, true), text)

  -- the path check names a traversal segment, not any two dots anywhere in the string
  eq(settings.safe_path "/home/me/.config/wez-vtabs/settings.json", true)
  eq(settings.safe_path "/home/me/my..notes/settings.json", true, "two dots inside a name are not a traversal")
  eq(settings.safe_path "/home/me/../etc/settings.json", false, "a whole `..` segment is")
  eq(settings.safe_path "settings.json", false, "and a relative path is refused outright")
  eq(settings.safe_path "", false)
  eq(settings.safe_path(nil), false)

  -- a container with a non-table default has no default for its children, and asking cannot throw
  local boxed = config.setup { settings = { path = "/tmp/x.json" }, backend = { path = "/bin/wez-vtabs" } }
  local rows = {}
  for _, row in ipairs(page.fields(boxed)) do
    rows[row.key] = row
  end
  assert(rows["settings.path"], "settings.path is listed")
  eq(rows["settings.path"].default, nil, "with no default of its own to compare against")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 A5: settings.json holds only what differs, versioned, and never a symlink", function()
  local settings = require "vtabs.settings"
  local schema_mod = require "vtabs.schema"
  local dir = "/tmp/vtabs-settings-" .. tostring(os.time()) .. tostring(math.random(1e6))
  os.execute("mkdir -p " .. dir)
  local path = dir .. "/settings.json"
  local function with_path(over)
    local opts = { backend = { path = "/bin/wez-vtabs" }, settings = { path = path } }
    for k, v in pairs(over or {}) do
      opts[k] = v
    end
    return config.setup(opts)
  end

  -- A5a: only the non-default keys, and the file names its version
  local cfg = with_path { width = 32, meta = "cwd" }
  assert(settings.save(cfg), "the file was written")
  local f = assert(io.open(path, "r"))
  local written = wezterm.json_parse(f:read "a")
  f:close()
  eq(written.version, 1)
  eq(written.options.width, 32)
  eq(written.options.meta, "cwd")
  eq(written.options.row_gap, nil, "a key still at its default is not written")
  eq(written.options.position, nil)
  eq(schema_mod.get(written.options, "settings.path"), path, "but a key the user moved is")

  -- A4c: the file moves a default, opts still wins over it
  local stored = settings.load(with_path {})
  eq(stored.width, 32, "the file is read back")
  eq(config.setup({ backend = { path = "/bin/wez-vtabs" } }, stored).width, 32, "and layers under opts")
  eq(config.setup({ width = 40, backend = { path = "/bin/wez-vtabs" } }, stored).width, 40, "which always wins")

  -- A5c: unknown keys go, but not the children of an open container
  local body = wezterm.json_encode {
    version = 1,
    options = {
      width = 30,
      nonsense = 1,
      icon_map = { totallymade_up = "x" },
      theme = { made_up = "#ffffff" },
      keys = { made_up = { key = "z" } },
    },
  }
  local out = assert(io.open(path, "w"))
  out:write(body)
  out:close()
  local before = #wezterm.log
  local kept = settings.load(with_path {})
  eq(kept.width, 30)
  eq(kept.nonsense, nil, "A5c: an unknown key is dropped")
  eq(kept.icon_map.totallymade_up, "x", "A5c: but icon_map is open")
  eq(kept.theme.made_up, "#ffffff", "and so are theme")
  eq(kept.keys.made_up.key, "z", "and keys")
  assert(#wezterm.log > before, "with one warning")

  -- A5d: a version we do not know is ignored outright
  out = assert(io.open(path, "w"))
  out:write(wezterm.json_encode { version = 99, options = { width = 30 } })
  out:close()
  eq(settings.load(with_path {}), nil, "A5d")
  out = assert(io.open(path, "w"))
  out:write "{ not json at all"
  out:close()
  eq(settings.load(with_path {}), nil, "corrupt is ignored too")

  -- A5f: the descriptor is `any`, so a table survives validation
  local table_cfg = config.setup { settings = { persist = false }, backend = { path = "/bin/wez-vtabs" } }
  eq(table_cfg.settings.persist, false, "A5f: the table is kept, not reset to the default")
  eq(settings.persists(table_cfg), false, "and persist = false never writes")
  eq(settings.save(table_cfg), false)

  os.remove(path)
  os.execute("rmdir " .. dir)
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 S9: config.replace runs the same cross-key rules setup does", function()
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  eq(cfg.close_button, "hover", "the shipped pair")
  local edited = util.merge(cfg, { hover = "press" })
  eq(config.replace(edited).close_button, "always", "press mode never hovers a background row")
  eq(config.get().close_button, "always", "and the live config carries the rule, not just setup's copy")

  local wide = util.merge(config.setup { backend = { path = "/bin/wez-vtabs" } }, { popover = { width = "wide" } })
  eq(config.replace(wide).popover.width, "auto", "and the popover width is coerced the same way")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("P3 S10: the persistence guards refuse, each for its own reason", function()
  local settings = require "vtabs.settings"
  local dir = "/tmp/vtabs-guards-" .. tostring(os.time()) .. tostring(math.random(1e6))
  os.execute("mkdir -p " .. dir)
  local path = dir .. "/settings.json"
  local function with(over)
    local opts = { backend = { path = "/bin/wez-vtabs" }, settings = over }
    return config.setup(opts)
  end

  local cfg = with { path = path }
  assert(settings.save(cfg), "a plain file is written")
  assert(settings.load(cfg), "and read back")

  -- the symlink refusal
  wezterm.symlinks[path] = true
  local before = #wezterm.log
  eq(settings.load(cfg), nil, "S10: a symlinked settings file is refused")
  assert(#wezterm.log > before, "with one warning")
  wezterm.symlinks[path] = nil
  assert(settings.load(cfg), "and read again once it is not one")

  -- the absolute-path refusal
  local relative = with { path = "settings.json" }
  eq(settings.path(relative), settings.dir .. "/settings.json", "S10: a relative path falls back to the default")
  local traversing = with { path = "/tmp/../etc/settings.json" }
  eq(settings.path(traversing), settings.dir .. "/settings.json", "and so does a traversing one")
  eq(settings.path(with { path = path }), path, "an absolute one is taken as given")

  -- persist
  eq(settings.persists(with { path = path, persist = false }), false, "S10: persist = false never writes")
  eq(settings.save(with { path = path, persist = false }), false)
  eq(settings.persists(config.setup { settings = false, backend = { path = "/bin/wez-vtabs" } }), false)
  eq(settings.persists(with { path = path }), true)

  os.remove(path)
  os.execute("rmdir " .. dir)
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 S11: escape and a bare q close the page, a chord does not", function()
  local settings = require "vtabs.settings"
  local function closes(ev)
    local win, gui = window(2)
    attach_all(win, gui)
    for _, tab in ipairs(win.tab_list) do
      mark_ready(tab)
    end
    settings.open(gui)
    local before = #win.tab_list
    local answered = settings.key(gui, ev)
    return answered, #win.tab_list < before
  end
  local answered, closed = closes { key = "escape" }
  assert(answered and closed, "escape closes it")
  answered, closed = closes { key = "q" }
  assert(answered and closed, "so does a bare q")
  answered, closed = closes { key = "q", mods = {} }
  assert(answered and closed, "S11: an empty mods array is bare, and the wire sends an array")
  answered = closes { key = "q", mods = { "CTRL" } }
  eq(answered, false, "S11: but CTRL+q is the shell's, not ours")
  answered = closes { key = "q", mods = "CTRL" }
  eq(answered, false, "and a string still works, for anything that sends one")
  answered = closes { key = "j" }
  eq(answered, false, "and every other key is the page's own")
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 A4a/A6a: opts keys are recorded as locked, and replace re-derives the glyphs", function()
  local settings = require "vtabs.settings"
  local cfg = config.setup {
    width = 30,
    theme = { accent = "#f5c2e7" },
    backend = { path = "/bin/wez-vtabs" },
  }
  assert(config.explicit.width, "A4a: a key the user wrote is explicit")
  assert(config.explicit["theme.accent"], "A4a: nested too")
  assert(not config.explicit.row_gap, "and one they left alone is not")
  eq(config.explicit["backend.path"], true)

  -- A6a: glyphs are derived from icon_map, so replace must not carry a stale table over
  local edited = util.merge(cfg, { icon_map = { close = "Z" } })
  edited.glyphs = cfg.glyphs
  eq(config.replace(edited).glyphs.close, "Z", "A6a: config.replace re-derives cfg.glyphs")
  eq(config.get().glyphs.close, "Z")

  -- A5e: nothing here arms a timer
  local before = #wezterm.log
  settings.persists(config.get())
  eq(#wezterm.log, before)
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("P3 A4b: the host's own wezterm keys are recorded before the plugin writes any", function()
  local vtabs = dofile(here .. "/../init.lua")
  local hosted =
    { keys = {}, window_padding = { left = 8, right = 8, top = 8, bottom = 8 }, window_decorations = "TITLE" }
  vtabs.apply_to_config(hosted, { backend = { path = "/bin/wez-vtabs" } })
  eq(config.host_config.window_padding.left, 8, "A4b: the host's value, not the one we would write")
  eq(config.host_config.window_decorations, "TITLE")
  local bare = { keys = {} }
  vtabs.apply_to_config(bare, { backend = { path = "/bin/wez-vtabs" } })
  eq(config.host_config.window_padding, nil, "and nil where the host left it alone")
  eq(config.host_config.colors_split, nil)
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("collapsed = rail keeps the pane and narrows it to rail_width", function()
  local win, gui = window(1)
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
  local win, gui = window(1)
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
  eq(usub(rows[seen.strip.toggle_row], x, x), "❮", "and §8 centres the one glyph that fits in the rail")
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
    local win, gui = window(1)
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

test("P3 §4: options the host set in wezterm.lua are LOCKED on the Behaviour group", function()
  -- Exactly what the screenshot harness passes, so this is the precedence a shot would show.
  local cfg = config.setup {
    backend = { path = "/bin/wez-vtabs" },
    poll_ms = 200,
    confirm_close = false,
    debug = true,
  }
  local fields = page.fields(cfg)
  local by_key = {}
  for _, row in ipairs(fields) do
    by_key[row.key] = row
  end
  for _, key in ipairs { "poll_ms", "confirm_close", "debug" } do
    local row = by_key[key]
    assert(row, key .. " has a form row")
    eq(row.locked, "wezterm.lua", key .. " is locked by the host config")
  end
  eq(by_key.poll_ms.group, "behaviour", "poll_ms lives on the Behaviour group")

  -- An option the host left alone stays editable, so LOCKED names the host, not every row.
  eq(by_key.width.locked, nil, "width was not set in wezterm.lua")

  local groups = page.groups(fields)
  local behaviour = nil
  for i, g in ipairs(groups) do
    if g == "behaviour" then
      behaviour = i
    end
  end
  assert(behaviour, "the nav carries a Behaviour group")

  local v = page_view { cfg = cfg, st = { group = behaviour, focus = 1 } }
  local lines = page_rows(v)
  local locked_row = nil
  for _, line in ipairs(lines) do
    if line:find("poll_ms", 1, true) then
      locked_row = line
    end
  end
  assert(locked_row, "the Behaviour group renders the poll_ms row")
  assert(locked_row:find("LOCKED", 1, true), "the poll_ms row carries the LOCKED badge: " .. locked_row)
end)

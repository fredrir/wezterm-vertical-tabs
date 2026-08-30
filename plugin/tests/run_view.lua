local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local state = require "vtabs.state"
local hit = require "vtabs.hit"
local sidebar = require "vtabs.sidebar"
local geometry = require "vtabs.geometry"
local fake = require "fake_mux"
local view_mod = require "vtabs.view"
local platform = require "vtabs.platform"
local model_mod = require "vtabs.model"

local test, eq, rgb, title_row = H.test, H.eq, H.rgb, H.title_row
local legacy, mark_ready, mouse = H.legacy, H.mark_ready, H.mouse
local press_row, window, ready_window = H.press_row, H.window, H.ready_window

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
  eq(seen.strip.rows, 2, "no macOS reserve in the fake, so this padding.top plus the toggle row")
  eq(seen.strip.cols, 0)
  eq(seen.strip.toggle.row, 2, "and the padding is above the glyph, not below it")
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
  local win, gui = window(1)
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
  local win, gui = window(1)
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
  eq(preview.rows, 2, "the two reserved rows, and padding.top is 0")
  eq(strip_of({ titlebar = "macos", position = "right" }).cols, 0, "the lights are on the left only")
  render_mod.render = original
  platform.is_mac = was_mac
  view_mod.invalidate_theme()
  config.setup(legacy { backend = { path = "/bin/wez-vtabs" } })
end)

test("every row of a card activates its tab, pads included, but only inside the card surface", function()
  local win, gui = ready_window()
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
  local win, gui = ready_window()
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
  local strip_hit = state.session.hits[sb1:pane_id()][1]
  local toggle_x
  for _, span in ipairs(strip_hit.spans or {}) do
    toggle_x = span.id == "toggle" and span.x1 or toggle_x
  end
  assert(toggle_x, "the strip band carries a toggle span")
  mouse(gui, sb1, "down", "left", toggle_x, 1)
  assert(state.is_collapsed(gui:window_id()), "the toggle hides the sidebar")
  sidebar.set_collapsed(gui, false)
end)

test("an armed ✕ closes only where it was pressed, and any drag cancels it", function()
  local win, gui = ready_window()
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
  local win, gui = ready_window()
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
    local win, gui = ready_window()
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
  local win = ready_window()
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

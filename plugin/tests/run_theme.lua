local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local theme = require "vtabs.theme"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local view_mod = require "vtabs.view"
local palettes = require "palettes"

local test, eq, rgb, hex, palette = H.test, H.eq, H.rgb, H.hex, H.palette
local legacy, ready_window = H.legacy, H.ready_window

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
  local win, gui = ready_window()
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
  eq(cfg.padding.top, 0)
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

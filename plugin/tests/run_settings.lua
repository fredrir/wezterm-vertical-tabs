local H = require "support.helpers"
local wezterm = require "wezterm"
local util = require "vtabs.util"
local config = require "vtabs.config"
local theme = require "vtabs.theme"
local keys = require "vtabs.keys"
local state = require "vtabs.state"
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

local test, eq, rgb = H.test, H.eq, H.rgb
local hex, legacy = H.hex, H.legacy
local attach_all, mark_ready, window = H.attach_all, H.mark_ready, H.window
local here = H.here

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
  -- the painting page owns its keys and reports Escape as the verb
  input.handle(gui, pane, "vtabs", '{"t":"do","a":"close_settings"}')
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
    view_mod.sync(gui)
    for _, button in ipairs(require("vtabs.actions").resolved_strip(config.get())) do
      if button.id == "settings" then
        require("vtabs.input").handle(gui, sb, "vtabs", '{"t":"do","a":"strip","id":"settings"}')
        return true
      end
    end
    return false
  end
  assert(click_settings(), "the strip carries a settings button by default")
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

test("settings_model: the body carries every form row pre-rendered, its nav and its preview", function()
  local settings_model = require "vtabs.settings_model"
  local cfg = config.setup { backend = { path = "/bin/wez-vtabs" } }
  local body = settings_model.body(cfg, { group = 1, focus = 1 }, nil)
  eq(body.screen, "settings")
  eq(type(body.version), "string")
  assert(#body.fields > 20, "the whole form crossed")
  assert(#body.groups >= 5, "the nav groups crossed")
  eq(body.groups[1].id, "layout")
  eq(body.groups[1].label, "Layout")
  local width
  for _, f in ipairs(body.fields) do
    if f.key == "width" then
      width = f
    end
  end
  assert(width, "the width row is there")
  eq(width.widget, "stepper")
  eq(width.value_text, "‹ 28 ›")
  eq(width.changed, false)
  eq(#body.preview.tabs, 3)
  eq(body.preview.render.padding.left, cfg.padding.left)
  local pending = settings_model.body(cfg, {}, { icons = false })
  eq(pending.preview.icons, false, "the preview reflects a pending edit")
end)

test("settings_model: the verbs commit through page.commit like the v1 keys do", function()
  local settings_model = require "vtabs.settings_model"
  local win = require("fake_mux").window()
  win:add_tab { title = "t1" }
  local gui = win.gui
  config.setup { backend = { path = "/bin/wez-vtabs" } }
  local st = {}
  local before = config.get().width
  settings_model.act(gui, st, "nudge_option", { key = "width", delta = 1 })
  eq(config.get().width, before + 1, "nudge steps and commits")
  settings_model.act(gui, st, "reset_option", { key = "width" })
  eq(config.get().width, config.defaults.width, "reset restores the default")
  settings_model.act(gui, st, "activate_option", { key = "new_tab_label" })
  assert(st.editing and st.editing.key == "new_tab_label", "a text field opens an edit buffer")
  for _, ch in ipairs { "backspace", "backspace", "backspace", "X" } do
    settings_model.act(gui, st, "edit_key", { key = ch })
  end
  settings_model.act(gui, st, "edit_key", { key = "enter" })
  eq(st.editing, nil, "enter commits and closes the buffer")
  assert(config.get().new_tab_label:sub(-1) == "X", "the typed text landed")
  settings_model.act(gui, st, "activate_option", { key = "keys.close_tab" })
  eq(st.armed, "keys.close_tab", "a recorder arms")
  settings_model.act(gui, st, "record_chord", { key = "t", mods = { "CTRL" } })
  eq(st.armed, nil)
  eq(config.get().keys.close_tab.key, "t", "the chord committed")
  eq(settings_model.act(gui, st, "nope", {}), false, "unknown verbs are not ours")
end)

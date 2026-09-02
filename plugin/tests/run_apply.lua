local H = require "support.helpers"
local config = require "vtabs.config"
local host_config = require "vtabs.host_config"
local keys = require "vtabs.keys"
local page = require "vtabs.page"
local platform = require "vtabs.platform"
local protocol = require "vtabs.gen.protocol"
local schema = require "vtabs.schema"
local settings = require "vtabs.settings"
local settings_model = require "vtabs.settings_model"
local util = require "vtabs.util"
local version = require "vtabs.version"

local test, eq = H.test, H.eq

local function normalize_reply(values, warnings)
  return {
    normalizer_v = 1,
    plugin_version = version,
    schema_id = schema.schema_id,
    values = values,
    warnings = warnings or require("vtabs.wire").array(),
  }
end

local function commit(gui, path, value, mode, body)
  return page.commit_effect(gui, {
    settings_rev = 1,
    path = path,
    change = { op = "set", value = value },
    mode = mode or "instant",
    persistence_json = body or '{"version":1,"options":{}}',
  })
end

---The same chord in wezterm's other spelling: aliases swapped, order reversed, spaces around `|`.
local function respelled(mods)
  local parts = {}
  for part in mods:gmatch "[^|]+" do
    table.insert(parts, 1, ({ CMD = "SUPER", ALT = "OPT" })[part] or part)
  end
  return table.concat(parts, " | ")
end

local function bound_to(entries, key)
  local out = {}
  for _, entry in ipairs(entries) do
    if entry.key == key then
      out[#out + 1] = entry
    end
  end
  return out
end

test("host projection keys and apply policies come from generated descriptors", function()
  local descriptors = {}
  for _, option in ipairs(require "vtabs.gen.schema") do
    descriptors[option.key] = option
  end
  eq(descriptors.frame.host_key, "window_padding")
  eq(descriptors.frame.apply_mode, "override")
  eq(descriptors.edge_to_edge.apply_mode, "reload")
  eq(descriptors["backend.uservar"].apply_mode, "reload")

  local expected = {
    edge_to_edge = "window_padding",
    frame = "window_padding",
    ["frame.zen"] = "window_padding",
    ["frame.margin"] = "window_padding",
    ["frame.inset"] = "window_padding",
    titlebar = "window_decorations",
    dim_inactive_panes = "inactive_pane_hsb",
    hover = "pane_focus_follows_mouse",
    ["theme.split"] = "colors_split",
  }
  for key, host_key in pairs(expected) do
    eq(host_config.HOST_KEYS[key], host_key, key)
  end
  for key in pairs(host_config.HOST_KEYS) do
    eq(expected[key], host_config.HOST_KEYS[key], "unexpected host projection " .. key)
  end
  eq(host_config.HOST_KEYS["frame.border"], nil, "unrelated frame changes stay instant")

  local owned = host_config.owned_keys { colors_split = "red", window_padding = {} }
  eq(table.concat(owned, ","), "window_padding,colors_split", "host inventory uses descriptor order")
end)

test("a chord the user bound under another spelling of its modifiers is left alone", function()
  local mine = function() end
  local host = {
    keys = {
      { key = "t", mods = respelled(platform.SUPER), action = mine },
      { key = "b", mods = respelled(platform.SUPER2), action = mine },
    },
  }
  keys.apply(host, { keys = {} })
  -- `t` and `b` each carry one more default on the other tier; the user's own chord is never doubled
  for _, key in ipairs { "t", "b" } do
    local entries = bound_to(host.keys, key)
    eq(#entries, 2, key .. ": the user's chord plus the default on the other tier")
    local own = 0
    for _, entry in ipairs(entries) do
      own = own + (entry.action == mine and 1 or 0)
    end
    eq(own, 1, key .. ": exactly one entry is the user's")
  end
  eq(#bound_to(host.keys, "w"), 1, "an unbound default is still appended")
end)

test("a bare RESIZE on macOS gets the window buttons back; plain and a right sidebar decline", function()
  local init = require "init"
  local was = platform.is_mac
  platform.is_mac = true
  local function applied(decorations, opts)
    local host = { window_decorations = decorations }
    opts = opts or {}
    opts.settings = { path = "/nonexistent/vtabs-test-settings.json" }
    init.apply_to_config(host, opts)
    return host.window_decorations, config.host_config.window_decorations
  end
  eq(applied(nil), "INTEGRATED_BUTTONS|RESIZE", "unset opts in")
  local got, host = applied "RESIZE"
  eq(got, "INTEGRATED_BUTTONS|RESIZE", "RESIZE is upgraded rather than warned about")
  eq(host, nil, "and the key is the plugin's, so the page offers titlebar")
  got, host = applied("RESIZE", { titlebar = "plain" })
  eq(got, "RESIZE", "plain keeps the user's RESIZE")
  eq(host, "RESIZE", "which stays the host's")
  eq(applied "TITLE|RESIZE", "TITLE|RESIZE", "any other value is left alone")
  eq(applied("RESIZE", { position = "right" }), "RESIZE", "a right-hand sidebar reserves nothing")
  platform.is_mac = was
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("plugin reload watches include both generated Rust mirrors", function()
  local wezterm = require "wezterm"
  local watched = table.concat(wezterm.reload_watch, "\n")
  assert(watched:find("/vtabs/gen/protocol.lua", 1, true), "protocol mirror is watched")
  assert(watched:find("/vtabs/gen/schema.lua", 1, true), "schema mirror is watched")
end)

test("a Rust-normalized poll interval remains an integer at the WezTerm boundary", function()
  local boot_normalize = require "vtabs.boot_normalize"
  local init = require "init"
  local real_try = boot_normalize.try
  boot_normalize.try = function()
    local cfg = config.setup { settings = false, backend = { path = "/bin/wez-vtabs" } }
    cfg.poll_ms = 500.0
    return cfg
  end
  local host = {}
  local ok, err = pcall(init.apply_to_config, host, {})
  boot_normalize.try = real_try
  if not ok then
    error(err, 0)
  end
  eq(host.status_update_interval, 500)
  eq(math.type(host.status_update_interval), "integer")
end)

test("hover_highlight off makes a hover-only close button permanent, as press mode does", function()
  local cfg = config.setup { hover_highlight = false, close_button = "hover", backend = { path = "/bin/wez-vtabs" } }
  eq(cfg.close_button, "always")
  eq(config.setup({ close_button = "hover", backend = { path = "/bin/wez-vtabs" } }).close_button, "hover")
end)

test("live settings applies Rust's derived cross-field patches without a second Lua policy pass", function()
  config.setup { hover = "follow", close_button = "hover", settings = false, backend = { path = "/bin/wez-vtabs" } }
  page.commit_effect(nil, {
    settings_rev = 1,
    path = { "hover" },
    change = { op = "set", value = "press" },
    derived = {},
    mode = "instant",
    persistence_json = '{"version":1,"options":{}}',
  })
  eq(config.get().close_button, "hover", "Lua does not invent a derived change")
  page.commit_effect(nil, {
    settings_rev = 1,
    path = { "hover" },
    change = { op = "set", value = "press" },
    derived = { { path = { "close_button" }, change = { op = "set", value = "always" } } },
    mode = "instant",
    persistence_json = '{"version":1,"options":{"hover":"press","close_button":"always"}}',
  })
  eq(config.get().close_button, "always", "the Rust-derived host value is applied")
end)

test("derived paths receive host projection and escalate reload once", function()
  local wezterm = require "wezterm"
  local _, gui = H.window(1)
  wezterm.reloads = 0
  config.host_config = {}
  config.setup { hover = "follow", settings = false, backend = { path = "/bin/wez-vtabs" } }
  local mode = page.commit_effect(gui, {
    settings_rev = 1,
    path = { "width" },
    change = { op = "set", value = 29 },
    derived = { { path = { "hover" }, change = { op = "set", value = "press" } } },
    mode = "instant",
    persistence_json = '{"version":1,"options":{"width":29,"hover":"press"}}',
  })
  eq(mode, "reload")
  eq(wezterm.reloads, 1, "all derived projections aggregate into one reload")
  eq(config.get().hover, "press")
end)

test("a settings commit is saved and becomes the next resolved configuration", function()
  local path = os.tmpname()
  os.remove(path)
  config.host_config = {}
  config.setup { settings = { path = path }, backend = { path = "/bin/wez-vtabs" } }
  eq(commit(nil, { "width" }, 37, "instant", '{"version":1,"options":{"width":37}}'), "instant")
  local stored = assert(settings.load { settings = { path = path } })
  eq(stored.width, 37, "the accepted edit was written")
  eq(config.setup({}, stored).width, 37, "the next config evaluation restores it")
  os.remove(path)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("a settings commit never copies wezterm.lua-owned values into settings.json", function()
  local path = os.tmpname()
  os.remove(path)
  config.host_config = {}
  config.setup({ width = 42, settings = { path = path } }, nil)
  eq(commit(nil, { "poll_ms" }, 700, "instant", '{"version":1,"options":{"poll_ms":700}}'), "instant")
  local stored = assert(settings.load { settings = { path = path } })
  eq(stored.width, nil, "an unrelated explicit value is not persisted")
  eq(stored.poll_ms, 700, "the page-owned edit is persisted")
  eq(
    config.setup({ settings = { path = path } }, stored).width,
    config.defaults.width,
    "removing it from wezterm.lua does not leave a stale copy"
  )
  os.remove(path)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the final persistence-disable and path-change commits use old permission and the new destination", function()
  local write_private = util.write_private
  local directory = settings.dir
  local writes = {}
  util.write_private = function(path, body)
    writes[#writes + 1] = { path = path, body = body }
    return true
  end
  settings.dir = "/tmp/vtabs-settings-regression"

  local ok, err = pcall(function()
    config.setup { settings = { path = "/tmp/vtabs-old.json" }, backend = { path = "/bin/wez-vtabs" } }
    commit(nil, { "settings", "persist" }, false, "reload", '{"version":1,"options":{"settings":{"persist":false}}}')
    eq(writes[#writes].path, "/tmp/vtabs-old.json", "persist=false is written once before taking effect")

    config.setup { settings = { path = "/tmp/vtabs-old.json" }, backend = { path = "/bin/wez-vtabs" } }
    commit(
      nil,
      { "settings", "path" },
      "/tmp/vtabs-new.json",
      "reload",
      '{"version":1,"options":{"settings":{"path":"/tmp/vtabs-new.json"}}}'
    )
    eq(writes[#writes].path, "/tmp/vtabs-new.json", "the new path receives the path-changing commit")

    config.setup { settings = { path = "/tmp/vtabs-old.json" }, backend = { path = "/bin/wez-vtabs" } }
    commit(nil, { "settings" }, false, "reload", '{"version":1,"options":{"settings":false}}')
    eq(
      writes[#writes].path,
      "/tmp/vtabs-settings-regression/settings.json",
      "settings=false uses the destination selected by the new config"
    )
  end)
  util.write_private = write_private
  settings.dir = directory
  config.setup { backend = { path = "/bin/wez-vtabs" } }
  if not ok then
    error(err, 0)
  end
end)

test("boot and live settings share split and zen-padding projection", function()
  local wezterm = require "wezterm"
  wezterm.reloads = 0
  local cfg = config.setup {
    frame = { zen = true, margin = 9, inset = 4 },
    theme = { split = "hidden" },
    settings = false,
    backend = { path = "/bin/wez-vtabs" },
  }
  local boot = { colors = { background = "#102030" } }
  host_config.apply_boot(boot, cfg, host_config.capture(boot))
  eq(boot.window_padding.left, 13, "boot composes zen margin and inset")
  eq(boot.window_padding.bottom, 13)
  eq(boot.colors.split, "#102030", "hidden split follows the page background")

  local _, gui = H.window(1)
  config.host_config = {}
  config.setup { settings = false, backend = { path = "/bin/wez-vtabs" } }
  commit(gui, { "frame" }, { zen = true, margin = 8, inset = 6 }, "override")
  eq(gui.overrides.window_padding.left, 14, "live zen uses the same padding projection")
  eq(gui.overrides.window_padding.top, 14)
  commit(gui, { "theme", "split" }, "hidden", "override")
  eq(gui.overrides.colors.split, "#1e1e2e", "theme.split has a live nested override")
  eq(commit(gui, { "theme", "split" }, "auto", "override"), "reload")
  eq(gui.overrides.colors, nil, "clearing a nested split override removes the empty colors table")
  eq(wezterm.reloads, 1, "clearing the boot-baked split requests a reload")
  commit(gui, { "frame" }, false, "override")
  eq(gui.overrides.window_padding.right, "1cell", "leaving zen restores edge padding")
  eq(gui.overrides.window_padding.top, "0.5cell")

  -- Boot has already baked this value into the window's base configuration. Clearing only the
  -- override would expose it again, so a nil projection must rebuild the base configuration.
  gui.window_padding = gui.overrides.window_padding
  gui.overrides.window_padding = nil
  eq(commit(gui, { "edge_to_edge" }, false, "reload"), "reload")
  eq(gui.overrides.window_padding, nil, "the obsolete live override is cleared")
  eq(wezterm.reloads, 2, "removing a boot-baked host value reloads the base config")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the raw settings projection preserves lists, segmented paths and opaque functions", function()
  local wire = require "vtabs.wire"
  config.setup {
    padding = { top = 2 },
    strip_actions = {},
    spaces = {
      {
        id = "work",
        match = { domain = {}, host = {}, user = {}, proc = {}, cwd = {}, title = {} },
      },
    },
    backend = { path = function() end, env = { ["A.B"] = "one" } },
  }
  local body = settings_model.body(config.get(), wire.array)
  local encoded = wire.encode(body)
  assert(encoded:find('"strip_actions":[]', 1, true), "an empty option list stays an array")
  for _, field in ipairs { "domain", "host", "user", "proc", "cwd", "title" } do
    assert(encoded:find('"' .. field .. '":[]', 1, true), field .. " empty match list stays an array")
  end
  assert(encoded:find('"spaces":[', 1, true), "a populated option list stays an array")
  assert(encoded:find('"A.B":"one"', 1, true), "an open-map key keeps its dot")
  assert(encoded:find('["backend","path"]', 1, true), "a function is an opaque segmented fact")
  assert(encoded:find('["padding","top"]', 1, true), "the explicit leaf is segmented")
  assert(not encoded:find('["padding"]', 1, true), "a structural parent is not a prefix lock")
  config.setup { spaces = {}, backend = { path = "/bin/wez-vtabs" } }
  local empty = wire.encode(settings_model.body(config.get(), wire.array))
  assert(empty:find('"spaces":[]', 1, true), "a second empty option list stays an array")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("settings projection defaults typed opacity but preserves supported Lua-owned references", function()
  local wire = require "vtabs.wire"
  local title = function() end
  local backend_path = function() end
  local click = function() end
  local cycle = {}
  cycle.LOOP = cycle
  local shared = { callback = function() end }
  local original = {
    width = function() end,
    title = title,
    strip_actions = { { id = "mine", on_click = click } },
    backend = { path = backend_path, env = cycle },
    keys = shared,
    frame = shared,
  }
  local values, opaque, invalid = settings_model.project(original, wire.array)
  local encoded_invalid = wire.encode(settings_model.paths(invalid, wire.array))
  assert(encoded_invalid:find('["width"]', 1, true), "a typed scalar function is invalid, not opaque")
  local encoded_opaque = wire.encode(settings_model.paths(opaque, wire.array))
  for _, path in ipairs {
    '["title"]',
    '["strip_actions"]',
    '["backend","path"]',
    '["backend","env"]',
    '["keys"]',
    '["frame"]',
  } do
    assert(encoded_opaque:find(path, 1, true), path .. " remains Lua-owned")
  end
  settings_model.restore(values, original, opaque)
  eq(values.title, title)
  eq(values.backend.path, backend_path)
  eq(values.strip_actions[1].on_click, click)
  eq(values.backend.env, cycle)
  assert(rawequal(values.keys, values.frame), "shared tables retain identity")
end)

test("fallback validation rejects sparse mixed lists and non-finite numbers", function()
  local defaults = config.defaults
  eq(config.setup({ width = function() end }).width, defaults.width)
  eq(config.setup({ strip_actions = { [2] = "search" } }).strip_actions[1], defaults.strip_actions[1])
  eq(config.setup({ strip_actions = { "search", named = true } }).strip_actions[1], defaults.strip_actions[1])
  eq(config.setup({ strip_actions = { named = true } }).strip_actions[1], defaults.strip_actions[1])
  eq(#config.setup({ strip_actions = {} }).strip_actions, 0, "an intentional empty list remains valid")
  for _, invalid in ipairs { math.huge, -math.huge, 0 / 0 } do
    eq(config.setup({ width = invalid }).width, defaults.width)
  end
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("settings paths accept Windows roots and reject relative drives and traversal", function()
  local was = platform.is_windows
  platform.is_windows = true
  assert(settings.safe_path [[C:\Users\me\settings.json]])
  assert(settings.safe_path [[C:/Users/me/settings.json]])
  assert(settings.safe_path [[\\server\share\settings.json]])
  assert(not settings.safe_path [[C:settings.json]])
  assert(not settings.safe_path [[C:\Users\..\settings.json]])
  platform.is_windows = false
  assert(settings.safe_path "/tmp/settings.json")
  assert(not settings.safe_path "tmp/settings.json")
  platform.is_windows = was
end)

test("private writes never replace the destination after permission or I/O failure", function()
  local wezterm = require "wezterm"
  local path = os.tmpname()
  local function read()
    local f = assert(io.open(path, "r"))
    local body = f:read "a"
    f:close()
    return body
  end
  local f = assert(io.open(path, "w"))
  f:write "old"
  f:close()

  wezterm.chmod_ok = false
  eq(util.write_private(path, "permission", nil, "test-permission"), false)
  wezterm.chmod_ok = true
  eq(read(), "old", "chmod failure leaves the old destination")

  local real_open = util.private_open
  local opens = 0
  util.private_open = function(candidate, mode)
    if candidate:match "/value$" and mode == "w" then
      opens = opens + 1
      if opens == 2 then
        return {
          write = function()
            return nil
          end,
          flush = function()
            return true
          end,
          close = function()
            return true
          end,
        }
      end
    end
    return real_open(candidate, mode)
  end
  local ok, result = pcall(util.write_private, path, "partial", nil, "test-write")
  util.private_open = real_open
  if not ok then
    error(result, 0)
  end
  eq(result, false)
  eq(read(), "old", "write failure leaves the old destination")

  opens = 0
  util.private_open = function(candidate, mode)
    if candidate:match "/value$" and mode == "w" then
      opens = opens + 1
      if opens == 2 then
        return {
          write = function(self)
            return self
          end,
          flush = function()
            return true
          end,
          close = function()
            return nil
          end,
        }
      end
    end
    return real_open(candidate, mode)
  end
  ok, result = pcall(util.write_private, path, "unclosed", nil, "test-close")
  util.private_open = real_open
  if not ok then
    error(result, 0)
  end
  eq(result, false)
  eq(read(), "old", "close failure leaves the old destination")

  assert(util.write_private(path, "complete", nil, "test-success"))
  eq(read(), "complete")
  local saw_mode = false
  for _, args in ipairs(wezterm.spawned) do
    saw_mode = saw_mode or (args[1] == "chmod" and args[2] == "600")
  end
  assert(saw_mode, "the completed replacement was restricted to 0600")
  os.remove(path)
end)

test("boot normalization uses a local binary, restores opaque values, and removes its private request", function()
  local wezterm = require "wezterm"
  local wire = require "vtabs.wire"
  local executable = os.tmpname()
  local request_path
  local function title()
    return "opaque"
  end
  local cycle = {}
  cycle.self = cycle
  local old_dir = settings.dir
  settings.dir = "/tmp"
  wezterm.normalizer = function(args)
    request_path = args[5]
    local f = assert(io.open(request_path, "r"))
    local request = f:read "a"
    f:close()
    assert(request:find('"explicit":[', 1, true), "segmented ownership reaches Rust")
    assert(request:find('"strip_actions":[]', 1, true), "empty lists remain arrays")
    assert(not request:find("opaque", 1, true), "function contents never enter the request")
    local values = util.merge(config.defaults, {
      width = 41,
      strip_actions = {},
      backend = { path = executable, env = {} },
    })
    return true, wire.encode(normalize_reply(values)), ""
  end

  local ok, cfg = pcall(require("vtabs.boot_normalize").try, {
    width = 41,
    title = title,
    strip_actions = {},
    backend = { path = executable, env = { LOOP = cycle } },
  })
  wezterm.normalizer = nil
  settings.dir = old_dir
  os.remove(executable)
  if not ok then
    error(cfg, 0)
  end
  assert(cfg, "the local normalizer answer is adopted")
  eq(cfg.width, 41)
  eq(cfg.title, title, "the function is restored by reference")
  eq(cfg.backend.env.LOOP, cycle, "the cyclic value is restored by reference")
  eq(cfg.backend.env.LOOP.self, cycle, "the cycle itself remains intact")
  assert(request_path and io.open(request_path, "r") == nil, "request is removed after success")
end)

test("a missing or old local normalizer removes its request and preserves the Lua fallback", function()
  local wezterm = require "wezterm"
  local init = require "init"
  local executable = os.tmpname()
  local persisted = os.tmpname()
  local file = assert(io.open(persisted, "w"))
  file:write '{"version":1,"options":{"width":39}}'
  file:close()
  local request_path
  local old_dir = settings.dir
  settings.dir = "/tmp"
  wezterm.normalizer = function(args)
    request_path = args[5]
    return false, "", "unsupported command"
  end
  local host = {}
  local ok, err = pcall(init.apply_to_config, host, {
    -- `persist=false` disables future writes, not reading the existing file.
    settings = { path = persisted, persist = false },
    backend = { path = executable },
  })
  wezterm.normalizer = nil
  settings.dir = old_dir
  os.remove(executable)
  os.remove(persisted)
  if not ok then
    error(err, 0)
  end
  eq(config.get().width, 39, "generated-schema Lua path still loads persistence v1")
  eq(settings.persists(config.get()), false, "the write permission remains disabled")
  assert(request_path and io.open(request_path, "r") == nil, "request is removed after failure")
end)

test("boot normalization rejects missing stale and incomplete response contracts", function()
  local wezterm = require "wezterm"
  local wire = require "vtabs.wire"
  local executable = os.tmpname()
  local old_dir = settings.dir
  settings.dir = "/tmp"
  local replies = {
    { values = config.defaults, warnings = wire.array() },
    normalize_reply(config.defaults),
    normalize_reply(config.defaults),
    normalize_reply {},
    normalize_reply "not-an-object",
  }
  replies[2].normalizer_v = 99
  replies[3].schema_id = "stale"
  local ok, err = pcall(function()
    for index, response in ipairs(replies) do
      local request_path
      wezterm.normalizer = function(args)
        request_path = args[5]
        return true, wire.encode(response), ""
      end
      eq(require("vtabs.boot_normalize").try { backend = { path = executable } }, nil, "invalid response " .. index)
      assert(request_path and io.open(request_path, "r") == nil, "invalid response removes request")
    end
  end)
  wezterm.normalizer = nil
  settings.dir = old_dir
  os.remove(executable)
  if not ok then
    error(err, 0)
  end
end)

test("boot normalization writes no config when private request permissions cannot be enforced", function()
  local wezterm = require "wezterm"
  local executable = os.tmpname()
  local called = false
  local before = #wezterm.spawned
  wezterm.chmod_ok = false
  wezterm.normalizer = function()
    called = true
    return true, "{}", ""
  end
  local ok, value = pcall(require("vtabs.boot_normalize").try, { backend = { path = executable } })
  wezterm.chmod_ok = true
  wezterm.normalizer = nil
  os.remove(executable)
  if not ok then
    error(value, 0)
  end
  eq(value, nil, "the Lua fallback is selected")
  eq(called, false, "the executable never receives an insufficiently private request")
  local dir
  for i = before + 1, #wezterm.spawned do
    local args = wezterm.spawned[i]
    if args[1] == "mkdir" then
      dir = args[4]
    end
  end
  assert(dir, "a private directory was attempted")
  assert(os.rename(dir, dir) == nil, "the failed request directory is removed")
end)

test("segmented commit paths do not split dots inside open-map keys", function()
  config.setup { backend = { path = "/bin/wez-vtabs", env = {} }, settings = false }
  commit(nil, { "backend", "env", "A.B" }, "one")
  eq(config.get().backend.env["A.B"], "one")
  eq(config.get().backend.env.A, nil)
end)

test("the boot loader retains lists, dotted open keys and an empty frame table", function()
  local path = os.tmpname()
  local f = assert(io.open(path, "w"))
  f:write(
    '{"version":1,"options":{"strip_actions":["search"],"spaces":[{"id":"work"}],'
      .. '"backend":{"env":{"A.B":"1"}},"frame":{}}}'
  )
  f:close()
  local loaded = assert(settings.load { settings = { path = path } })
  eq(loaded.strip_actions[1], "search")
  eq(loaded.spaces[1].id, "work")
  eq(loaded.backend.env["A.B"], "1")
  assert(type(loaded.frame) == "table" and next(loaded.frame) == nil)
  os.remove(path)
end)

test("an empty persistence file is handled as corrupt rather than raising", function()
  local path = os.tmpname()
  local f = assert(io.open(path, "w"))
  f:close()
  eq(settings.read_body { settings = { path = path } }, "")
  eq(settings.load { settings = { path = path } }, nil)
  os.remove(path)
end)

test("settings persistence writes exactly the shared cap and refuses one byte over", function()
  local path = os.tmpname()
  os.remove(path)
  local cfg = { settings = { path = path, persist = true } }
  local exact = string.rep("x", protocol.SETTINGS_BODY_MAX_BYTES)
  assert(settings.save_body(cfg, exact), "the inclusive bound is writable")
  local file = assert(io.open(path, "r"))
  eq(#assert(file:read "*a"), protocol.SETTINGS_BODY_MAX_BYTES)
  file:close()

  assert(not settings.save_body(cfg, exact .. "x"), "a body that the next boot would reject is never persisted")
  file = assert(io.open(path, "r"))
  eq(#assert(file:read "*a"), protocol.SETTINGS_BODY_MAX_BYTES, "the existing file remains untouched")
  file:close()
  os.remove(path)
end)

test("gui-attached serves every window that has a GUI, past one the mux holds without", function()
  local wezterm = require "wezterm"
  local fake = require "fake_mux"
  local init = require "init"
  init.apply_to_config({}, {
    settings = { path = "/nonexistent/vtabs-test-settings.json" },
    backend = { path = "/bin/wez-vtabs" },
  })
  local handlers = wezterm.handlers["gui-attached"]
  assert(handlers and #handlers >= 1, "the handler is registered")
  local win = fake.window()
  win:add_tab { title = "g" }
  -- a standalone mux server's window, listed first: `gui_window()` throws for it
  local headless = {
    gui_window = function()
      error "mux window id 9 is not currently associated with a gui window"
    end,
  }
  table.insert(wezterm.windows, 1, headless)
  local ok, err = pcall(handlers[#handlers])
  table.remove(wezterm.windows, 1)
  assert(ok, "the handler survives it: " .. tostring(err))
  eq(H.sidebars_in(win.tab_list[1]), 1, "and the window after it still gets its sidebar")
  fake.close_window(win)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the poll counts a held key by switches to known tabs; a burst of new tabs is served at once", function()
  local wezterm = require "wezterm"
  local fake = require "fake_mux"
  local sidebar = require "vtabs.sidebar"
  local geometry = require "vtabs.geometry"
  local init = require "init"
  init.apply_to_config({}, {
    settings = { path = "/nonexistent/vtabs-test-settings.json" },
    backend = { path = "/bin/wez-vtabs" },
  })
  local handlers = wezterm.handlers["update-status"]
  local poll = handlers[#handlers]
  local win = fake.window()
  local wid = win:window_id()
  local first = win:add_tab { title = "a" }
  poll(win.gui)
  assert(sidebar.find(first), "the first tab is served")
  -- three CMD+T in a row: each new tab is active for a moment and gets its sidebar all the same
  local spawned = {}
  for i = 1, 3 do
    spawned[i] = win:add_tab { title = "new" .. i }
    win.active_tab_ref = spawned[i]
    poll(win.gui)
  end
  eq(geometry.switching(wid), false, "new tabs are not a held key")
  for i = 1, 3 do
    assert(sidebar.find(spawned[i]), "spawned tab " .. i .. " has its sidebar")
  end
  -- the same cadence through tabs the window already knows is a held key
  win.active_tab_ref = first
  poll(win.gui)
  win.active_tab_ref = spawned[1]
  poll(win.gui)
  eq(geometry.switching(wid), true, "two switches to known tabs inside the dwell")
  fake.close_window(win)
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

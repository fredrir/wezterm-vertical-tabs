local H = require "support.helpers"
local wezterm = require "wezterm"
local config = require "vtabs.config"
local schema = require "vtabs.schema"

local test, eq = H.test, H.eq

test("the schema describes every option the defaults expose, and nothing it does not", function()
  local function walk(tbl, prefix, seen)
    for key, value in pairs(tbl) do
      local path = prefix and (prefix .. "." .. key) or key
      local option = schema.by_key[path]
      assert(option, "no descriptor for " .. path)
      seen[path] = true
      -- A list's entries are values, not options; only containers hold more descriptors.
      if type(value) == "table" and not option.open and option.type ~= "list" then
        walk(value, path, seen)
      end
    end
    return seen
  end
  local seen = walk(config.defaults, nil, {})
  for _, option in ipairs(schema.options) do
    if option.default ~= nil and not schema.is_open(option.key) then
      assert(seen[option.key], option.key .. " has a default the config never grows")
    end
    assert(option.label and option.group, option.key .. " needs a label and a group")
    if option.type == "enum" then
      assert(option.enum and #option.enum > 0, option.key .. " is an enum with no values")
      local ok = false
      for _, allowed in ipairs(option.enum) do
        ok = ok or allowed == option.default
      end
      assert(ok, option.key .. " default is not one of its own enum values")
    end
  end
end)

test("schema.defaults is a fresh deep copy each time, so setup cannot poison it", function()
  local a, b = schema.defaults(), schema.defaults()
  assert(a.padding ~= b.padding, "nested tables are not shared")
  a.padding.top = 99
  eq(schema.defaults().padding.top, 1)
  eq(config.defaults.padding.top, 1)
  eq(config.defaults.padding.left, 2)
  eq(config.defaults.padding.right, 2)
  eq(config.defaults.padding.bottom, 1)
end)

test("a key the schema does not know warns, including inside a closed container", function()
  local function warns_for(opts)
    local before = #wezterm.log
    config.setup(opts)
    for i = before + 1, #wezterm.log do
      if wezterm.log[i]:find("unknown option", 1, true) then
        return wezterm.log[i]
      end
    end
    return nil
  end
  assert(warns_for({ widht = 30 }):find "widht", "top-level typo")
  assert(warns_for({ backend = { pth = "/x" } }):find "backend.pth", "nested typo")
  assert(warns_for({ hooks = { fliter = print } }):find "hooks.fliter")
  eq(warns_for { theme = { accent = "#ff0000" } }, nil, "theme is an open container")
  eq(warns_for { icon_map = { nvim = "x" } }, nil, "so is icon_map")
  eq(warns_for { keys = { new_tab = false } }, nil, "and keys")
  eq(warns_for { private = { env = { FOO = "1" } } }, nil, "and private.env")
  eq(warns_for { backend = { path = "/bin/wez-vtabs" } }, nil, "a known nested key is quiet")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("every enum rejects a value outside it and every range rejects the wrong side", function()
  for _, option in ipairs(schema.options) do
    if option.type == "enum" and not option.key:find "%." then
      local cfg = config.setup { [option.key] = "definitely-not-valid" }
      eq(cfg[option.key], option.default, option.key .. " reset to its default")
    end
  end
  eq(config.setup({ width = 4 }).width, 28, "below min")
  eq(config.setup({ width = "wide" }).width, 28, "wrong type")
  eq(config.setup({ row_gap = -1 }).row_gap, 0)
  eq(config.setup({ theme = { elevation = 2 } }).theme.elevation, 0.06, "above max")
  eq(config.setup({ toggle_button = "yes" }).toggle_button, true)
  eq(config.setup({ padding = { top = -1 } }).padding.top, 1, "nested keys validate too")
  eq(config.setup({ width = 40 }).width, 40, "a valid value survives")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("the committed options table is what the schema generates", function()
  local ok, how, code = os.execute "lua ../scripts/gen-docs.lua --check >/dev/null 2>&1"
  local status = code or (ok and 0 or 1)
  eq(status, 0, "docs/configuration.md is stale; run `just docs`")
  assert(how == nil or how == "exit", "generator exited normally")
end)

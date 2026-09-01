local H = require "support.helpers"
local config = require "vtabs.config"
local keys = require "vtabs.keys"
local platform = require "vtabs.platform"

local test, eq = H.test, H.eq

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

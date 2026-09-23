local plugin_path = assert(arg[1])
local host = {}
package.preload.wezterm = function()
  return host
end
local plugin = assert(loadfile(plugin_path))()
local config = { term = "xterm-256color" }

assert(plugin.apply_to_config(config, {}) == config)
assert(config.term == "xterm-256color")

host.gui = {}
local ok, message = pcall(plugin.apply_to_config, config)
assert(not ok and message:find("WezTerm build required", 1, true))
host.vtabs = { capability = 2 }
ok, message = pcall(plugin.apply_to_config, config)
assert(not ok and message:find("contract mismatch", 1, true))

local directory = os.tmpname()
os.remove(directory)
assert(os.execute("mkdir " .. directory))
local watched = {}
host.config_dir = directory
host.add_to_config_reload_watch_list = function(path)
  table.insert(watched, path)
end
local configured, dispatched
local hooks = {
  title = function(tab)
    return tab.title
  end,
}
host.vtabs = {
  capability = 1,
  configure = function(value)
    configured = value
  end,
  dispatch = function(window, action)
    dispatched = { window, action }
  end,
}
host.action_callback = function(callback)
  return callback
end
local options = { profile = "fixture", settings = { width = 280 }, hooks = hooks }
assert(plugin.apply_to_config(config, options) == config)
assert(configured.profile == "fixture" and configured.settings.width == 280)
assert(configured.hooks == nil and host.vtabs.hooks == hooks)
assert(options.hooks == hooks)
local default_file = directory .. "/vtabs_settings.lua"
assert(configured.managed_path == default_file and next(configured.managed) == nil)
assert(#watched == 0)

local managed = assert(io.open(default_file, "w"))
managed:write 'return { settings = { width = 300 }, spaces = { { id = "notes", name = "Notes" } } }'
managed:close()
plugin.apply_to_config(config, options)
assert(configured.managed.settings.width == 300 and configured.managed.spaces[1].id == "notes")
assert(configured.settings.width == 280 and watched[1] == default_file)

local custom = directory .. "/custom.lua"
managed = assert(io.open(custom, "w"))
managed:write "return 42"
managed:close()
ok, message = pcall(plugin.apply_to_config, config, { settings_file = custom })
assert(not ok and message:find("must return a table", 1, true))
os.remove(custom)
plugin.apply_to_config(config, { settings_file = custom })
assert(configured.managed_path == custom and configured.settings_file == nil)
os.remove(default_file)
os.remove(directory)
local window = {}
local action = { CreateSpace = { name = "Work" } }
plugin.action(action)(window)
assert(dispatched[1] == window and dispatched[2] == action)

host.vtabs.configure = function()
  error "invalid configuration"
end
ok, message = pcall(plugin.apply_to_config, config, { settings = { width = -1 } })
assert(not ok and message:find("invalid configuration", 1, true))
print "production Lua boundary passed"

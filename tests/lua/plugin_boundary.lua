local plugin_path = assert(arg[1])
local host = {}
package.preload.wezterm = function() return host end
local plugin = assert(loadfile(plugin_path))()
local config = {term = 'xterm-256color'}

assert(plugin.apply_to_config(config, {}) == config)
assert(config.term == 'xterm-256color')

host.gui = {}
local ok, message = pcall(plugin.apply_to_config, config)
assert(not ok and message:find('native WezTerm build required', 1, true))
host.native_tabs = {capability = 2}
ok, message = pcall(plugin.apply_to_config, config)
assert(not ok and message:find('contract mismatch', 1, true))

local configured, dispatched
local hooks = {title = function(tab) return tab.title end}
host.native_tabs = {
  capability = 1,
  configure = function(value) configured = value end,
  dispatch = function(window, action) dispatched = {window, action} end,
}
host.action_callback = function(callback) return callback end
local options = {profile = 'fixture', settings = {width = 280}, hooks = hooks}
assert(plugin.apply_to_config(config, options) == config)
assert(configured.profile == 'fixture' and configured.settings.width == 280)
assert(configured.hooks == nil and host.native_tabs.hooks == hooks)
assert(options.hooks == hooks)
local window = {}
local action = {CreateSpace = {name = 'Work'}}
plugin.action(action)(window)
assert(dispatched[1] == window and dispatched[2] == action)

host.native_tabs.configure = function() error('invalid native configuration') end
ok, message = pcall(plugin.apply_to_config, config, {settings = {width = -1}})
assert(not ok and message:find('invalid native configuration', 1, true))
print('production Lua boundary passed')

local wezterm = require 'wezterm'
local M = {}

---@param config table
---@param options? table
function M.apply_to_config(config, options)
  -- Mux servers and CLI tools load the same config without a GUI provider.
  if not wezterm.gui then return config end
  local native = assert(wezterm.native_tabs, 'native WezTerm build required; run just build')
  assert(native.capability == 1, 'native tabs contract mismatch; rebuild')
  options = options or {}
  local value = {}
  for key, item in pairs(options) do
    if key ~= 'hooks' then value[key] = item end
  end
  native.configure(value)
  native.hooks = options.hooks
  return config
end

---@param action string|table
function M.action(action)
  return wezterm.action_callback(function(window)
    wezterm.native_tabs.dispatch(window, action)
  end)
end

return M

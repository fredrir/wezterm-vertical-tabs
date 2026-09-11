local wezterm = require "wezterm"
local M = {}

---@param config table
---@param options? TabsOptions
---@return table
function M.apply_to_config(config, options)
  -- Mux servers and CLI tools load the same config without a GUI provider.
  if not wezterm.gui then
    return config
  end
  local vtabs = assert(wezterm.vtabs, "VTabs WezTerm build required; run just build")
  assert(vtabs.capability == 1, "vtabs contract mismatch; rebuild")
  options = options or {}
  local value = {}
  for key, item in pairs(options) do
    if key ~= "hooks" then
      value[key] = item
    end
  end
  vtabs.configure(value)
  vtabs.hooks = options.hooks
  return config
end

---@param action string|table
function M.action(action)
  return wezterm.action_callback(function(window)
    wezterm.vtabs.dispatch(window, action)
  end)
end

return M

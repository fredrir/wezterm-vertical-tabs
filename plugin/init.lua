local wezterm = require "wezterm"
local M = {}

---@param path string
---@return TabsManaged
local function load_managed(path)
  local file = io.open(path, "r")
  if not file then
    return {}
  end
  file:close()
  wezterm.add_to_config_reload_watch_list(path)
  local managed = dofile(path)
  assert(type(managed) == "table", path .. " must return a table")
  return managed
end

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
  local path = options.settings_file or (wezterm.config_dir .. "/vtabs_settings.lua")
  local value = { managed = load_managed(path), managed_path = path }
  for key, item in pairs(options) do
    if key ~= "hooks" and key ~= "settings_file" then
      value[key] = item
    end
  end
  vtabs.configure(value)
  vtabs.hooks = options.hooks
  return config
end

---@param action TabsAction
function M.action(action)
  return wezterm.action_callback(function(window)
    wezterm.vtabs.dispatch(window, action)
  end)
end

return M

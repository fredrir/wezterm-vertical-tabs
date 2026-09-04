---@class Config
local config = {}

---@return unknown
local function load_plugin()
  return {} --[[@as unknown]]
end

local plugin = load_plugin() ---@type VerticalTabs
local options = {} ---@type VerticalTabs.Config
options.positon = "left"

return plugin.apply_to_config(config, options)

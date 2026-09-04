---@class Config
local config = {}

---@return unknown
local function load_plugin()
  return {} --[[@as unknown]]
end

local plugin = load_plugin() ---@type VerticalTabs

return plugin.apply_to_config(config, {
  position = "top",
})

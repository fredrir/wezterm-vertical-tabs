---@class Config
local config = {}

---@return unknown
local function load_plugin()
  return {} --[[@as unknown]]
end

local plugin = load_plugin() ---@type VerticalTabs
local activate_first_tab = plugin.action.activate_tab(0)

local options = {
  position = "left",
  tab_height = "card",
  popover = {
    width = "auto",
    overflow = "grow",
  },
  private = {
    env = { TERM = "xterm-256color" },
  },
  title = function(tab, pane)
    return tostring(tab) .. tostring(pane)
  end,
} ---@type VerticalTabs.Config
local private_env = options.private and options.private.env

return plugin.apply_to_config(config, options), activate_first_tab, private_env

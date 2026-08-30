local platform = require "vtabs.platform"
local actions = require "vtabs.actions"

local M = {}

local SUPER, SUPER_SHIFT = platform.SUPER, platform.SUPER_SHIFT

function M.defaults()
  local keys = {
    toggle_sidebar = { key = "b", mods = SUPER },
    focus_sidebar = { key = "b", mods = SUPER_SHIFT },
    new_tab = { key = "t", mods = SUPER },
    close_tab = { key = "w", mods = SUPER },
    reopen_closed = { key = "t", mods = SUPER_SHIFT },
    pin_tab = { key = "p", mods = SUPER_SHIFT },
    private_window = { key = "n", mods = SUPER_SHIFT },
    new_window = { key = "n", mods = SUPER },
    next_tab = { key = "Tab", mods = "CTRL" },
    prev_tab = { key = "Tab", mods = "CTRL|SHIFT" },
    move_tab_up = { key = "PageUp", mods = SUPER_SHIFT },
    move_tab_down = { key = "PageDown", mods = SUPER_SHIFT },
    tab_last = { key = "9", mods = SUPER },
  }
  for i = 1, 8 do
    keys["tab_" .. i] = { key = tostring(i), mods = SUPER }
  end
  return keys
end

local function action_for(name)
  if name == "tab_last" then
    return actions.action.activate_tab(-1)
  end
  local n = name:match "^tab_(%d)$"
  if n then
    return actions.action.activate_tab(tonumber(n) - 1)
  end
  return actions.action[name]
end

---Builds key bindings from defaults merged with user overrides (`false` disables one).
function M.build(user)
  if user == false then
    return {}
  end
  local keys = M.defaults()
  for name, binding in pairs(user or {}) do
    keys[name] = binding
  end
  local out = {}
  local names = {}
  for name in pairs(keys) do
    names[#names + 1] = name
  end
  table.sort(names)
  for _, name in ipairs(names) do
    local binding = keys[name]
    local action = action_for(name)
    if binding and action then
      out[#out + 1] = { key = binding.key, mods = binding.mods, action = action }
    end
  end
  return out
end

function M.apply(config, cfg)
  local bindings = M.build(cfg.keys)
  if #bindings == 0 then
    return
  end
  config.keys = config.keys or {}
  for _, b in ipairs(bindings) do
    table.insert(config.keys, b)
  end
end

return M

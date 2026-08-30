local platform = require "vtabs.platform"
local actions = require "vtabs.actions"
local util = require "vtabs.util"

local M = {}

local SUPER, SUPER2 = platform.SUPER, platform.SUPER2

---Browser-style defaults; off macOS the first tier is CTRL|SHIFT so shell keys stay intact.
function M.defaults()
  local keys = {
    toggle_sidebar = { key = "b", mods = SUPER },
    focus_sidebar = { key = "b", mods = SUPER2 },
    new_tab = { key = "t", mods = SUPER },
    close_tab = { key = "w", mods = SUPER },
    reopen_closed = { key = "t", mods = SUPER2 },
    pin_tab = { key = "d", mods = SUPER2 },
    private_window = { key = "p", mods = SUPER2 },
    private_window_alt = { key = "n", mods = SUPER2 },
    new_window = { key = "n", mods = SUPER },
    next_tab = { key = "Tab", mods = "CTRL" },
    prev_tab = { key = "Tab", mods = "CTRL|SHIFT" },
    next_tab_alt = { key = "]", mods = SUPER2 },
    prev_tab_alt = { key = "[", mods = SUPER2 },
    move_tab_up = { key = "PageUp", mods = SUPER2 },
    move_tab_down = { key = "PageDown", mods = SUPER2 },
    tab_last = { key = "9", mods = SUPER },
    settings = { key = ",", mods = SUPER },
  }
  if platform.is_mac then
    keys.next_tab_arrow = { key = "RightArrow", mods = "CMD|OPT" }
    keys.prev_tab_arrow = { key = "LeftArrow", mods = "CMD|OPT" }
  end
  for i = 1, 8 do
    keys["tab_" .. i] = { key = tostring(i), mods = SUPER }
  end
  return keys
end

local ALIASES = {
  settings = "open_settings",
  private_window_alt = "private_window",
  next_tab_alt = "next_tab",
  prev_tab_alt = "prev_tab",
  next_tab_arrow = "next_tab",
  prev_tab_arrow = "prev_tab",
}

local function action_for(name)
  name = ALIASES[name] or name
  if name == "tab_last" then
    return actions.action.activate_tab(-1)
  end
  local n = name:match "^tab_(%d+)$"
  if n then
    return actions.action.activate_tab(tonumber(n) - 1)
  end
  local action = actions.action[name]
  return type(action) == "table" and action or nil
end

---Builds key bindings from defaults merged with user overrides (`false` disables one).
function M.build(user)
  if user == false then
    return {}
  end
  local keys = M.defaults()
  for name, binding in pairs(user or {}) do
    if keys[name] == nil and not action_for(name) then
      util.warn("unknown key name %q", name)
    else
      keys[name] = binding
    end
  end
  local out = {}
  for _, name in ipairs(util.sorted_keys(keys)) do
    local binding = keys[name]
    local action = action_for(name)
    if binding and action then
      out[#out + 1] = { key = binding.key, mods = binding.mods, action = action }
    end
  end
  return out
end

local function signature(binding)
  return tostring(binding.key) .. "|" .. tostring(binding.mods or "")
end

---Appends bindings the user has not already defined, so user keys always win.
function M.apply(config, cfg)
  local bindings = M.build(cfg.keys)
  if #bindings == 0 then
    return
  end
  config.keys = config.keys or {}
  local taken = {}
  for _, existing in ipairs(config.keys) do
    taken[signature(existing)] = true
  end
  for _, b in ipairs(bindings) do
    if not taken[signature(b)] then
      table.insert(config.keys, b)
    end
  end
end

return M

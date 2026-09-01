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
    next_space = { key = "e", mods = SUPER },
    prev_space = { key = "e", mods = SUPER2 },
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

-- Second default chords for a behaviour; the behaviour's own renames live in `actions.canonical`.
local ALIASES = {
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
  local action = actions.action[actions.canonical(name)]
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
      -- `vtabs` names the binding for the popover's key hints; `apply` drops it, since wezterm
      -- rejects a key entry carrying a field it does not know.
      out[#out + 1] = { key = binding.key, mods = binding.mods, action = action, vtabs = name }
    end
  end
  return out
end

-- wezterm accepts several spellings of one modifier, so `SUPER+t` and `CMD+t` are the same chord.
local MOD_ALIAS = { CMD = "SUPER", WIN = "SUPER", OPT = "ALT", META = "ALT", NONE = false }

local function signature(binding)
  local mods = {}
  for part in tostring(binding.mods or ""):upper():gmatch "[^|%s]+" do
    local mod = MOD_ALIAS[part]
    if mod ~= false then
      mods[#mods + 1] = mod or part
    end
  end
  table.sort(mods)
  return tostring(binding.key) .. "|" .. table.concat(mods, "|")
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
      table.insert(config.keys, { key = b.key, mods = b.mods, action = b.action })
    end
  end
end

return M

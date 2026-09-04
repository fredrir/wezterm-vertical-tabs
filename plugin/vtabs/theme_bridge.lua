local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local identity = require "vtabs.sidebar_identity"
local store = require "vtabs.store"
local util = require "vtabs.util"

local M = {}

local scope = store.scope "theme_bridge"
local resolved = scope.window()

local function rgb_hex(value)
  if type(value) ~= "table" then
    return nil
  end
  local out = {}
  for i = 1, 3 do
    local channel = tonumber(value[i])
    if not channel or channel < 0 or channel > 255 or channel ~= math.floor(channel) then
      return nil
    end
    out[i] = channel
  end
  return string.format("#%02x%02x%02x", out[1], out[2], out[3])
end

local function colour_hex(value)
  local from_rgb = rgb_hex(value)
  if from_rgb then
    return from_rgb
  end
  if type(value) ~= "string" then
    return nil
  end
  local parsed = util.try(wezterm.color.parse, value)
  if not parsed then
    return nil
  end
  local r, g, b = parsed:srgba_u8()
  return string.format("#%02x%02x%02x", r, g, b)
end

M.colour = colour_hex

local theme_colours = require("vtabs.gen.protocol").THEME_COLOR_FIELDS
local theme_fractions = require("vtabs.gen.protocol").THEME_FRACTION_FIELDS

---Normalizes raw config or hook output to the typed Rust override DTO. RGB triples are accepted for
---hook compatibility, but no resolution happens here: Rust remains the only contrast algorithm.
function M.overrides(user)
  local out = {}
  for key, value in pairs(type(user) == "table" and user or {}) do
    if theme_fractions[key] then
      local number = tonumber(value)
      if number and number >= 0 and number <= 1 then
        out[key] = number
      end
    elseif theme_colours[key] then
      out[key] = colour_hex(value)
    end
  end
  return out
end

local function json_copy(value, seen, depth)
  local kind = type(value)
  if kind == "string" or kind == "boolean" then
    return value, true
  end
  if kind == "number" then
    return value, value == value and value ~= math.huge and value ~= -math.huge
  end
  if kind ~= "table" or depth >= 4 or seen[value] then
    return nil, false
  end
  seen[value] = true
  local out, count = {}, 0
  for key, child in pairs(value) do
    count = count + 1
    if count > 64 or (type(key) ~= "string" and type(key) ~= "number") then
      seen[value] = nil
      return nil, false
    end
    local copy, ok = json_copy(child, seen, depth + 1)
    if not ok then
      seen[value] = nil
      return nil, false
    end
    out[key] = copy
  end
  seen[value] = nil
  return out, true
end

---Space definitions remain raw policy input. This only converts host colour objects into a JSON
---colour spelling; invalid or unknown JSON-safe fields stay present so Rust can diagnose them.
function M.raw_overrides(user)
  local out = {}
  for key, value in pairs(type(user) == "table" and user or {}) do
    local normalized = theme_colours[key] and colour_hex(value) or nil
    if normalized then
      out[key] = normalized
    else
      local copy, ok = json_copy(value, {}, 0)
      if ok then
        out[key] = copy
      end
    end
  end
  return out
end

---Runs the user hook against Rust's resolved base and returns its typed answer.
---An absent/failing/non-table hook answers with an empty overlay so the paused commit can finish.
function M.answer_hook(gui_window, pane, ev)
  if type(ev) ~= "table" or type(ev.theme) ~= "table" then
    return false
  end
  local answer = {}
  local hook = (config.get().hooks or {}).theme
  if type(hook) == "function" then
    local ok, value = pcall(hook, gui_window, ev.theme)
    if not ok then
      util.warn_once("hook-theme", "theme hook failed: %s", tostring(value))
    elseif type(value) == "table" then
      answer = M.overrides(value)
    end
  end
  return identity.send(pane, { t = "theme_hook_result", overrides = answer })
end

---Caches a structurally valid answer from the window's current authenticated backend pane.
function M.accept(gui_window, ev)
  if type(ev) ~= "table" or type(ev.theme) ~= "table" then
    return false
  end
  local wid = gui_window:window_id()
  resolved[wid] = ev.theme
  return true
end

function M.get(window_id)
  return resolved[window_id]
end

function M.clear(window_id)
  if window_id then
    resolved[window_id] = nil
    return
  end
  for id in pairs(resolved) do
    resolved[id] = nil
  end
end

return M

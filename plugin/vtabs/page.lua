local config = require "vtabs.config"
local schema = require "vtabs.schema"
local util = require "vtabs.util"

local M = {}

-- Below this the page says so and draws nothing else; above PREVIEW_COLS the preview box appears.
M.MIN_COLS = 48
M.PREVIEW_COLS = 90

local GROUP_LABELS = {
  layout = "Layout",
  cards = "Cards",
  chrome = "Chrome",
  behaviour = "Behaviour",
  theme = "Theme",
  identity = "Identity",
  hooks = "Hooks",
  backend = "Backend",
  spaces = "Spaces",
}

-- The nav reads in the order the sidebar is built up, not alphabetically.
local GROUP_ORDER = {
  layout = 1,
  cards = 2,
  chrome = 3,
  behaviour = 4,
  theme = 5,
  identity = 6,
  hooks = 7,
  backend = 8,
  spaces = 9,
}

---Keys the plugin can only act on through `apply_to_config`, so an edit needs a config reload.
local RELOAD_KEYS = {
  ["backend.path"] = true,
  ["backend.repo"] = true,
  ["backend.version"] = true,
  ["backend.uservar"] = true,
  hide_native_tab_bar = true,
  skip_close_confirmation = true,
  edge_to_edge = true,
  titlebar = true,
}

---WezTerm keys the plugin writes on the host's config; the page can only set them live where the
---host left them nil, and `set_config_overrides` is the only door.
local HOST_KEYS = {
  edge_to_edge = "window_padding",
  titlebar = "window_decorations",
  dim_inactive_panes = "inactive_pane_hsb",
  hover = "pane_focus_follows_mouse",
  ["theme.split"] = "colors_split",
}

local COLOUR_SUFFIX = { "_fg$", "_bg$", "accent$", "border$", "border_idle$", "separator$", "split$" }

local function looks_like_colour(key, value)
  if type(value) == "string" and value:match "^#%x%x%x%x%x%x$" then
    return true
  end
  for _, pattern in ipairs(COLOUR_SUFFIX) do
    if key:match(pattern) then
      return true
    end
  end
  return false
end

local function same(a, b)
  if type(a) ~= "table" or type(b) ~= "table" then
    return a == b
  end
  for k, v in pairs(a) do
    if not same(v, b[k]) then
      return false
    end
  end
  for k in pairs(b) do
    if a[k] == nil then
      return false
    end
  end
  return true
end

local function count_entries(value)
  local n = 0
  for _ in pairs(type(value) == "table" and value or {}) do
    n = n + 1
  end
  return n
end

---Why this row cannot be edited here, or nil when it can.
local function lock_for(key, value)
  if config.explicit[key] then
    return "wezterm.lua"
  end
  local host = HOST_KEYS[key]
  if host and config.host_config[host] ~= nil then
    return "wezterm.lua (host)"
  end
  if type(value) == "function" then
    return "not editable"
  end
  return nil
end

---The widget a descriptor's own facts ask for. The page never names an option.
local function widget_for(option, key, value)
  if type(value) == "function" then
    return "locked"
  end
  if option and option.type == "boolean" then
    return "toggle"
  end
  if option and option.type == "enum" then
    return "picker"
  end
  if option and option.type == "number" then
    return "stepper"
  end
  if key:match "^keys%." then
    return "recorder"
  end
  if type(value) == "boolean" then
    return "toggle"
  end
  if type(value) == "number" then
    return "stepper"
  end
  if type(value) == "table" then
    return "entries"
  end
  if looks_like_colour(key, value) then
    return "colour"
  end
  return "text"
end

local function field(key, option, cfg, group, opts)
  opts = opts or {}
  local value = schema.get(cfg, key)
  local default = schema.get(config.defaults, key)
  return {
    key = key,
    label = opts.label or key,
    group = group,
    option = option,
    value = value,
    default = default,
    widget = opts.widget or widget_for(option, key, value),
    locked = lock_for(key, value),
    changed = not same(value, default),
    reload = RELOAD_KEYS[key] == true,
    host_key = HOST_KEYS[key],
    depth = opts.depth or 0,
  }
end

---Every row of the form, in schema order, with each `open` container followed by its own entries.
---A new descriptor with a new `group` grows the nav with no edit here.
function M.fields(cfg)
  cfg = cfg or config.get()
  local out = {}
  for _, option in ipairs(schema.options) do
    if not option.container then
      local row = field(option.key, option, cfg, option.group)
      out[#out + 1] = row
      if option.open and type(row.value) == "table" then
        row.widget = "entries"
        row.entries = count_entries(row.value)
        for _, name in ipairs(util.sorted_keys(row.value)) do
          local child = option.key .. "." .. name
          out[#out + 1] = field(child, nil, cfg, option.group, { label = "  " .. name, depth = 1 })
        end
      end
    end
  end
  return out
end

---The nav: the distinct groups the descriptors declare, in reading order.
function M.groups(fields)
  local seen, out = {}, {}
  for _, row in ipairs(fields or M.fields()) do
    if row.group and not seen[row.group] then
      seen[row.group] = true
      out[#out + 1] = row.group
    end
  end
  table.sort(out, function(a, b)
    return (GROUP_ORDER[a] or 99) < (GROUP_ORDER[b] or 99)
  end)
  return out
end

function M.group_label(group)
  return GROUP_LABELS[group] or (group:sub(1, 1):upper() .. group:sub(2))
end

---How a value reads in the value column, by widget.
function M.value_text(row)
  local value = row.value
  if row.widget == "toggle" then
    return value and "[ on ]" or "[ off ]"
  end
  if row.widget == "picker" then
    return "‹ " .. tostring(value) .. " ›"
  end
  if row.widget == "stepper" then
    return "‹ " .. tostring(value) .. " ›"
  end
  if row.widget == "entries" then
    local n = row.entries or count_entries(value)
    return n == 1 and "1 entry" or (tostring(n) .. " entries")
  end
  if row.widget == "recorder" then
    if type(value) == "table" then
      return tostring(value.mods and (value.mods .. "+") or "") .. tostring(value.key)
    end
    return value == false and "off" or tostring(value)
  end
  if type(value) == "function" then
    return "fun()"
  end
  if row.widget == "colour" then
    return "██ " .. tostring(value)
  end
  if type(value) == "string" then
    return string.format("%q", value)
  end
  return tostring(value)
end

---Steps a stepper or picker by `delta`, staying inside the descriptor's own bounds so the page can
---never offer a value `config.validate` would reject.
function M.step(row, delta)
  local option = row.option
  if row.widget == "stepper" then
    local n = tonumber(row.value) or 0
    local step = option and option.integer and 1 or 1
    local next_value = n + delta * step
    if option and option.integer then
      next_value = math.floor(next_value + 0.5)
    end
    if option and option.min and next_value < option.min then
      next_value = option.min
    end
    if option and option.max and next_value > option.max then
      next_value = option.max
    end
    return next_value
  end
  if row.widget == "picker" and option and option.enum then
    local at = 1
    for i, allowed in ipairs(option.enum) do
      if allowed == row.value then
        at = i
      end
    end
    local n = #option.enum
    local next_at = ((at - 1 + delta) % n) + 1
    return option.enum[next_at]
  end
  if row.widget == "toggle" then
    return not row.value
  end
  return row.value
end

---How an edit reaches the running window: most keys are ours to swap, a few are WezTerm's own, and
---a few only exist while `apply_to_config` runs.
function M.apply_mode(row)
  if row.reload then
    return "reload"
  end
  if row.host_key then
    return config.host_config[row.host_key] == nil and "override" or "locked"
  end
  return "instant"
end

---The current non-default set as a paste-ready `apply_to_config` call: the escape hatch that makes
---the page a starting point rather than a lock-in.
function M.as_lua(cfg)
  cfg = cfg or config.get()
  local function render_value(value, indent)
    if type(value) == "string" then
      return string.format("%q", value)
    end
    if type(value) ~= "table" then
      return tostring(value)
    end
    local parts = {}
    for _, k in ipairs(util.sorted_keys(value)) do
      local v = value[k]
      if type(v) ~= "function" then
        parts[#parts + 1] = string.rep(" ", indent + 2) .. k .. " = " .. render_value(v, indent + 2) .. ","
      end
    end
    if #parts == 0 then
      return "{}"
    end
    return "{\n" .. table.concat(parts, "\n") .. "\n" .. string.rep(" ", indent) .. "}"
  end

  local lines = {}
  for _, option in ipairs(schema.options) do
    if not option.container then
      local value = schema.get(cfg, option.key)
      local default = schema.get(config.defaults, option.key)
      if value ~= nil and type(value) ~= "function" and not same(value, default) then
        lines[#lines + 1] = string.format("  %s = %s,", option.key, render_value(value, 2))
      end
    end
  end
  if #lines == 0 then
    return "vtabs.apply_to_config(config, {})"
  end
  return "vtabs.apply_to_config(config, {\n" .. table.concat(lines, "\n") .. "\n})"
end

return M

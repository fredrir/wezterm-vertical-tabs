local config = require "vtabs.config"
local keys_mod = require "vtabs.keys"
local platform = require "vtabs.platform"
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

---Every widget in one table: how it reads, how a step moves it, and what Enter does to it. Adding
---a control is adding a row here, not a branch in three functions.
local WIDGETS = {
  toggle = {
    text = function(row)
      return row.value and "[ on ]" or "[ off ]"
    end,
    step = function(row)
      return not row.value
    end,
    activate = function(row)
      return "commit", not row.value
    end,
  },
  picker = {
    text = function(row)
      return "‹ " .. tostring(row.value) .. " ›"
    end,
    step = function(row, delta)
      local enum = row.option and row.option.enum
      if not enum or #enum == 0 then
        return row.value
      end
      local at = 1
      for i, allowed in ipairs(enum) do
        at = allowed == row.value and i or at
      end
      return enum[((at - 1 + delta) % #enum) + 1]
    end,
  },
  stepper = {
    text = function(row)
      return "‹ " .. tostring(row.value) .. " ›"
    end,
    step = function(row, delta)
      local option = row.option or {}
      local next_value = (tonumber(row.value) or 0) + delta
      if option.integer then
        next_value = math.floor(next_value + 0.5)
      end
      if option.min and next_value < option.min then
        next_value = option.min
      end
      if option.max and next_value > option.max then
        next_value = option.max
      end
      return next_value
    end,
  },
  colour = {
    text = function(row)
      return "██ " .. tostring(row.value)
    end,
    activate = function(row)
      return "edit", tostring(row.value)
    end,
  },
  text = {
    text = function(row)
      return string.format("%q", tostring(row.value))
    end,
    activate = function(row)
      return "edit", tostring(row.value)
    end,
  },
  recorder = {
    text = function(row)
      if row.armed then
        return "press a key…   [ ARMED  ]"
      end
      local binding = "off"
      if type(row.value) == "table" then
        binding = (row.value.mods and row.value.mods .. "+" or "") .. tostring(row.value.key)
      elseif row.value ~= false then
        binding = tostring(row.value)
      end
      return binding .. "   [ record ]"
    end,
    activate = function()
      return "record"
    end,
  },
  variant = {
    text = function(row)
      return "‹ " .. M.variant_name(row) .. " ›"
    end,
    step = function(row, delta)
      local presets = M.variants(row)
      -- a table no preset describes is `custom`: shown, never stepped away from by an arrow key
      if M.variant_name(row) == "custom" then
        return row.value
      end
      local at = 1
      for i, preset in ipairs(presets) do
        at = preset == row.value and i or at
      end
      return presets[((at - 1 + delta) % #presets) + 1]
    end,
    activate = function(row)
      if M.variant_name(row) == "custom" then
        return nil
      end
      return "commit", M.WIDGETS.variant.step(row, 1)
    end,
  },
  entries = {
    text = function(row)
      local n = row.entries or count_entries(row.value)
      return n == 1 and "1 entry" or (tostring(n) .. " entries")
    end,
  },
  locked = {
    text = function(row)
      return type(row.value) == "function" and "fun()" or tostring(row.value)
    end,
  },
}

M.WIDGETS = WIDGETS

---The presets a `type = "any"` key offers. A descriptor names them; otherwise a boolean default
---means the key is simply on or off.
function M.variants(row)
  local option = row.option or {}
  if type(option.variants) == "table" then
    return option.variants
  end
  return { false, true }
end

---What the picker reads for the current value: a named preset, or `custom` for a table that
---matches none of them - which a toggle could never have expressed.
function M.variant_name(row)
  for _, preset in ipairs(M.variants(row)) do
    if preset == row.value then
      return preset == false and "off" or (preset == true and "on" or tostring(preset))
    end
  end
  return "custom"
end

---The widget a descriptor's own facts ask for. The page never names an option.
local function widget_for(option, key, value)
  if type(value) == "function" then
    return "locked"
  end
  local by_type = { boolean = "toggle", enum = "picker", number = "stepper", any = "variant" }
  if option and by_type[option.type] then
    return by_type[option.type]
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
  return type(value) == "string" and "text" or "locked"
end

---`schema.get` walks a dotted key assuming every level is a table; `settings` defaults to `true`,
---so asking for `settings.path`'s default indexes a boolean. Here nothing under a non-table
---default has a default of its own, which is the honest answer anyway.
local function default_for(key)
  local node = config.defaults
  local last = nil
  for part in key:gmatch "[^.]+" do
    if last then
      if type(node[last]) ~= "table" then
        return nil
      end
      node = node[last]
    end
    last = part
  end
  return node[last]
end

local function field(key, option, cfg, group, opts)
  opts = opts or {}
  local value = opts.value
  if value == nil then
    value = schema.get(cfg, key)
  end
  local default = opts.default
  if default == nil then
    default = default_for(key)
  end
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
  local function expand(option, value, defaults)
    for _, name in ipairs(util.sorted_keys(value)) do
      local child = option.key .. "." .. name
      out[#out + 1] = field(child, schema.by_key[child], cfg, option.group, {
        label = "  " .. name,
        depth = 1,
        value = value[name],
        default = defaults and defaults[name] or nil,
      })
    end
  end

  ---`cfg.keys` holds only the user's overrides, but what the user wants to see and rebind is every
  ---binding that is actually live, so the rows come from the resolved set.
  local function effective(option, value)
    if option.key ~= "keys" or value == false then
      return value, nil
    end
    local shipped = keys_mod.defaults()
    return util.merge(shipped, value or {}), shipped
  end
  for _, option in ipairs(schema.options) do
    local value = schema.get(cfg, option.key)
    if not option.container then
      local row = field(option.key, option, cfg, option.group)
      out[#out + 1] = row
      if option.open and type(row.value) == "table" then
        local live, shipped = effective(option, row.value)
        -- a variant keeps its picker and reads `custom`; a plain container just counts its entries
        row.widget = row.widget == "variant" and "variant" or "entries"
        row.entries = count_entries(live)
        expand(option, live, shipped)
      end
    elseif option.open and type(value) == "table" then
      -- `theme` is both: the container row would say nothing the entries do not
      expand(option, value)
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

---How a value reads in the value column.
function M.value_text(row)
  return (WIDGETS[row.widget] or WIDGETS.locked).text(row)
end

---Steps a widget by `delta`, staying inside the descriptor's own bounds so the page can never
---offer a value `config.validate` would reject.
function M.step(row, delta)
  local widget = WIDGETS[row.widget]
  if not widget or not widget.step then
    return row.value
  end
  -- an explicit branch: `and/or` would swallow a toggle stepping to false
  return widget.step(row, delta)
end

---How an edit reaches the running window: most keys are ours to swap, a few are WezTerm's own, and
---a few only exist while `apply_to_config` runs.
function M.apply_mode(row)
  if row.host_key and config.host_config[row.host_key] ~= nil then
    return "locked"
  end
  if row.reload then
    return "reload"
  end
  if row.host_key then
    return "override"
  end
  return "instant"
end

---Everything that differs from the schema default, nested as the user would write it. `settings`
---persists exactly this set and `as_lua` prints it, so the file and the clipboard cannot disagree.
function M.changed(cfg)
  local out = {}
  for _, row in ipairs(M.fields(cfg or config.get())) do
    -- a container's own row is skipped: its children carry the same values one key at a time, and
    -- writing both would try to index the table it had already been set to
    local container = row.entries ~= nil
    if row.changed and not container and type(row.value) ~= "function" then
      schema.set(out, row.key, row.value)
    end
  end
  return out
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

  local changed = M.changed(cfg)
  local lines = {}
  for _, key in ipairs(util.sorted_keys(changed)) do
    lines[#lines + 1] = string.format("  %s = %s,", key, render_value(changed[key], 2))
  end
  if #lines == 0 then
    return "vtabs.apply_to_config(config, {})"
  end
  return "vtabs.apply_to_config(config, {\n" .. table.concat(lines, "\n") .. "\n})"
end

-- Shown only while the recorder is armed, where it is actionable, and never as ambient chrome.
-- `enable_kitty_keyboard` would widen what the pty delivers, but its effects reach far past this
-- plugin, so it is named and left to wezterm.lua.
-- Wrapped to 42 cells: the form is 46 wide at 100 columns and 42 at 60, so one wording serves both.
M.CAVEAT = {
  "⚠ macOS does not deliver CMD to the pty.",
  "  Type it into wezterm.lua, or avoid CMD.",
  "  enable_kitty_keyboard widens what arrives",
}

---True when the armed recorder cannot see what the user is about to press.
function M.caveat_applies()
  return platform.is_mac == true
end

---Three fixed fake tabs, so the preview says the same thing whatever the user happens to have open.
local SAMPLE = {
  { tab_id = 1, index = 1, is_active = false, is_pinned = false, title = "zsh", meta = "~/", process = "zsh" },
  {
    tab_id = 2,
    index = 2,
    is_active = true,
    is_pinned = false,
    title = "init.lua",
    meta = "~/.config",
    process = "nvim",
  },
  {
    tab_id = 3,
    index = 3,
    is_active = false,
    is_pinned = false,
    title = "cargo run",
    meta = "~/src",
    process = "cargo",
  },
}

M.SAMPLE = SAMPLE

local function deep_copy(value)
  if type(value) ~= "table" then
    return value
  end
  local out = {}
  for k, v in pairs(value) do
    out[k] = deep_copy(v)
  end
  return out
end

---The WezTerm key each overridable option maps to, and the value to write. `init.apply_*` is the
---boot-time twin of this; here it is one key at a time, live.
local OVERRIDES = {
  edge_to_edge = function(cfg)
    if cfg.edge_to_edge == false then
      return "window_padding", nil
    end
    local band = cfg.edge_to_edge == "sides" and "0.5cell" or 0
    local padding = { left = 0, right = 0, top = band, bottom = band }
    padding[cfg.position == "left" and "right" or "left"] = "1cell"
    return "window_padding", padding
  end,
  dim_inactive_panes = function(cfg)
    if cfg.dim_inactive_panes ~= false then
      return "inactive_pane_hsb", nil
    end
    return "inactive_pane_hsb", { brightness = 1.0, saturation = 1.0, hue = 1.0 }
  end,
  hover = function(cfg)
    return "pane_focus_follows_mouse", cfg.hover == "follow" or nil
  end,
}

---Applies one edit. Most keys are ours to swap outright; a few are WezTerm's, and those are only
---ours where the host left them nil; a few only exist while `apply_to_config` runs.
---@return string mode "instant" | "override" | "reload" | "locked"
function M.commit(gui_window, row, value)
  local mode = M.apply_mode(row)
  if mode == "locked" or row.locked then
    return "locked"
  end
  local next_cfg = deep_copy(config.get())
  schema.set(next_cfg, row.key, value)
  config.replace(next_cfg)
  local build = OVERRIDES[row.key]
  if build and gui_window then
    local key, wezterm_value = build(next_cfg)
    util.try(function()
      local overrides = gui_window:get_config_overrides() or {}
      overrides[key] = wezterm_value
      gui_window:set_config_overrides(overrides)
    end)
  end
  return mode
end

return M

local config = require "vtabs.config"
local icons = require "vtabs.icons"
local keys_mod = require "vtabs.keys"
local platform = require "vtabs.platform"
local render = require "vtabs.render"
local schema = require "vtabs.schema"
local util = require "vtabs.util"
local version = require "vtabs.version"

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

local function field(key, option, cfg, group, opts)
  opts = opts or {}
  local value = opts.value
  if value == nil then
    value = schema.get(cfg, key)
  end
  local default = opts.default
  if default == nil then
    default = schema.get(config.defaults, key)
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

  local changed = {}
  for _, row in ipairs(M.fields(cfg)) do
    if row.changed and row.widget ~= "entries" and type(row.value) ~= "function" then
      schema.set(changed, row.key, row.value)
    end
  end
  local lines = {}
  for _, key in ipairs(util.sorted_keys(changed)) do
    lines[#lines + 1] = string.format("  %s = %s,", key, render_value(changed[key], 2))
  end
  if #lines == 0 then
    return "vtabs.apply_to_config(config, {})"
  end
  return "vtabs.apply_to_config(config, {\n" .. table.concat(lines, "\n") .. "\n})"
end

---Column landmarks. Two breakpoints and nothing else: under MIN_COLS the page says so, under
---PREVIEW_COLS it is nav plus form, above it the preview box appears.
function M.grid(cols)
  if cols < M.MIN_COLS then
    return nil
  end
  local preview = cols >= M.PREVIEW_COLS
  local nav_w = preview and 18 or 14
  local g = {
    cols = cols,
    preview = preview,
    nav_x1 = 2,
    nav_x2 = 1 + nav_w,
  }
  g.divider = g.nav_x2 + 1
  -- §2's column table: caret one clear of the divider, label two clear of the caret
  g.caret_x = g.divider + 1
  g.label_x = g.caret_x + 2
  if preview then
    g.preview_x1 = cols - 31
    g.preview_x2 = cols
    g.form_x2 = g.preview_x1 - 3
  else
    g.form_x2 = cols - 2
  end
  g.marker_x = g.form_x2
  g.value_x2 = g.form_x2 - 2
  g.label_x2 = g.label_x + 22
  return g
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

---Rows the body has for nav entries and form rows: header, rule, blank … blank, rule, hints.
function M.body_rows(rows)
  return math.max(rows - 6, 0)
end

local function matches(row, filter)
  if not filter or filter == "" then
    return true
  end
  return row.key:lower():find(filter:lower(), 1, true) ~= nil
end

---Everything about a frame of the page that does not depend on colour.
---@param view table `{ cols, rows, cfg, st }`
function M.plan(view)
  local cols, rows = view.cols, view.rows
  local g = M.grid(cols)
  local st = view.st or {}
  local out = { grid = g, rows = {}, hits = {} }
  if not g then
    local row = math.max(math.floor(rows / 2), 1)
    for i = 1, rows do
      out.rows[i] = { kind = "space" }
      out.hits[i] = { kind = "space" }
    end
    out.rows[row] = { kind = "too_narrow", text = "Settings needs " .. M.MIN_COLS .. " columns" }
    return out
  end

  local fields = M.fields(view.cfg)
  local groups = M.groups(fields)
  local group = groups[math.max(math.min(st.group or 1, #groups), 1)]
  local shown = {}
  for _, row in ipairs(fields) do
    if row.group == group and matches(row, st.filter) then
      row.armed = st.armed == row.key
      shown[#shown + 1] = row
      if row.armed and M.caveat_applies() then
        for _, line in ipairs(M.CAVEAT) do
          shown[#shown + 1] = { caveat = line, key = row.key .. ".caveat" }
        end
      end
    end
  end
  out.groups = groups
  out.group = group
  out.fields = shown

  local body = M.body_rows(rows)
  local focus = math.max(math.min(st.focus or 1, math.max(#shown, 1)), 1)
  local scroll = math.max(math.min(st.scroll or 0, math.max(#shown - body, 0)), 0)
  if focus <= scroll then
    scroll = focus - 1
  elseif focus > scroll + body then
    scroll = focus - body
  end
  out.focus, out.scroll = focus, scroll

  out.rows[1] = { kind = "header" }
  out.hits[1] = { kind = "chrome" }
  out.rows[2] = { kind = "rule" }
  out.hits[2] = { kind = "chrome" }
  out.rows[3] = { kind = "space" }
  out.hits[3] = { kind = "space" }
  for i = 1, body do
    local row = 3 + i
    local nav = groups[i]
    local shown_row = shown[i + scroll]
    local caveat = shown_row and shown_row.caveat or nil
    -- a caveat line is not a form row: it has no label, no widget and no target
    local entry = caveat == nil and shown_row or nil
    out.rows[row] = {
      kind = "body",
      nav = nav,
      nav_selected = nav == group,
      caveat = caveat,
      field = entry,
      focused = entry ~= nil and (i + scroll) == focus,
      preview_index = g.preview and i or nil,
    }
    -- one row can carry a nav entry and a form row at once, so the column decides which was clicked
    local spans = {}
    if nav then
      spans[#spans + 1] = { id = "nav", x1 = g.nav_x1, x2 = g.nav_x2 }
    end
    if entry and entry.locked == nil and (entry.widget == "picker" or entry.widget == "stepper") then
      spans[#spans + 1] = { id = "dec", x1 = g.value_x2 - 11, x2 = g.value_x2 - 11 }
      spans[#spans + 1] = { id = "inc", x1 = g.value_x2, x2 = g.value_x2 }
    end
    if entry then
      spans[#spans + 1] = { id = "value", x1 = g.value_x2 - 11, x2 = g.value_x2 }
      spans[#spans + 1] = { id = "field", x1 = g.caret_x, x2 = g.form_x2 }
    end
    if nav or entry then
      out.hits[row] = {
        kind = "body",
        nav = nav,
        id = entry and entry.key or nil,
        index = entry and (i + scroll) or nil,
        x1 = g.nav_x1,
        x2 = g.form_x2,
        spans = spans,
      }
    else
      out.hits[row] = { kind = "space" }
    end
  end
  for row = body + 4, rows - 3 do
    out.rows[row] = { kind = "space" }
    out.hits[row] = { kind = "space" }
  end
  if rows >= 3 then
    out.rows[rows - 2] = { kind = "space" }
    out.hits[rows - 2] = { kind = "space" }
    out.rows[rows - 1] = { kind = "rule" }
    out.hits[rows - 1] = { kind = "chrome" }
    out.rows[rows] = { kind = "hints" }
    out.hits[rows] = { kind = "chrome" }
  end
  return out
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

---Composed from guarded glyphs rather than written out: the arrows are East Asian Ambiguous and
---U+23CE is in barely any monospace font, so both go through the same fallback every glyph does.
local function hints(glyphs, wide)
  local ud = glyphs.hint_up .. glyphs.hint_down
  local lr = glyphs.hint_left .. glyphs.hint_right
  if wide then
    return ud .. " field   " .. lr .. " change   Enter edit   r reset   c copy as Lua   esc close"
  end
  return ud .. " " .. lr .. " Enter r c esc"
end

---A 28-column sidebar built from an explicitly merged table. `config.get()` is never reassigned for
---a preview, which is what keeps a half-typed hex colour out of every real sidebar in the window.
function M.preview_cells(cfg, pending, theme, glyphs, rows)
  local merged = util.merge(cfg, pending or {})
  -- the icons come from the merged map, so editing `icon_map` changes the preview (A6a)
  local map = icons.resolve(merged.icon_map)
  local items = {}
  for i, sample in ipairs(SAMPLE) do
    items[i] = {}
    for k, v in pairs(sample) do
      items[i][k] = v
    end
    items[i].icon = map[sample.process] or map.default
  end
  return render.cells {
    cols = 28,
    rows = rows,
    items = items,
    theme = theme,
    cfg = merged,
    glyphs = glyphs,
    strip = { rows = 1, cols = 0, toggle_row = 1 },
    scroll = 0,
  }
end

---Paints a planned page. The same three cell primitives the sidebar uses, then the same encoder.
---@param view table `{ cols, rows, cfg, theme, glyphs, st, pending }`
function M.paint(view)
  local plan = M.plan(view)
  local g, theme, cols = plan.grid, view.theme, view.cols
  local painted, hits = {}, plan.hits
  local dim = theme.meta_fg or theme.dim

  local preview_frame = nil
  if g and g.preview then
    preview_frame = M.preview_cells(view.cfg, view.pending, theme, view.glyphs, M.body_rows(view.rows) - 2)
  end

  for row = 1, view.rows do
    local spec = plan.rows[row]
    local cells = render.new_line(cols, theme.bg, theme.fg)
    if spec and spec.kind == "too_narrow" then
      local x = math.max(math.floor((cols - util.width(spec.text)) / 2) + 1, 1)
      render.put(cells, x, spec.text, { fg = theme.fg }, cols)
    elseif spec and spec.kind == "header" then
      render.put(cells, 2, (view.glyphs.settings or "*") .. " Settings", { fg = theme.fg, bold = true }, cols)
      local tag = "wez-vtabs " .. version
      if cols < M.PREVIEW_COLS then
        tag = version
      end
      render.put(cells, cols - util.width(tag), tag, { fg = dim }, cols)
    elseif spec and spec.kind == "rule" then
      for x = 2, cols - 1 do
        render.put(cells, x, view.glyphs.rule, { fg = theme.separator }, x)
      end
    elseif spec and spec.kind == "hints" then
      local text = hints(view.glyphs, cols >= M.PREVIEW_COLS)
      render.put(cells, 2, util.truncate(text, cols - 2, view.glyphs.ellipsis), { fg = dim }, cols)
    elseif spec and spec.kind == "body" then
      if spec.nav then
        local fg = spec.nav_selected and theme.fg or dim
        if spec.nav_selected then
          render.fill(cells, g.nav_x1, g.nav_x2, theme.active_bg)
          render.put(cells, g.nav_x1, view.glyphs.active, { fg = theme.accent }, g.nav_x1)
        end
        render.put(cells, g.nav_x1 + 2, M.group_label(spec.nav), { fg = fg }, g.nav_x2)
      end
      render.put(cells, g.divider, "│", { fg = theme.separator }, g.divider)
      if spec.caveat then
        render.put(cells, g.caret_x, util.truncate(spec.caveat, g.form_x2 - g.caret_x + 1, view.glyphs.ellipsis), {
          fg = theme.unseen_fg,
        }, g.form_x2)
      end
      if spec.field then
        M.paint_field(cells, spec, g, view, theme)
      end
      if preview_frame and spec.preview_index then
        M.paint_preview(cells, spec.preview_index, preview_frame, g, theme, view.glyphs)
      end
    end
    painted[row] = cells
  end

  return render.paint {
    cells = painted,
    fades = {},
    hits = hits,
    cols = cols,
    rows = view.rows,
    theme = theme,
  }
end

---Shorter wording for a value column too narrow to name the source in full.
local SHORT_SOURCE = {
  ["wezterm.lua"] = "wezterm.lua",
  ["wezterm.lua (host)"] = "host",
  ["not editable"] = "read-only",
}

---One form row: caret, label, right-aligned value, and the badge that says why it is not editable.
function M.paint_field(cells, spec, g, view, theme)
  local row = spec.field
  local dim = theme.meta_fg or theme.dim
  if spec.focused then
    render.put(cells, g.caret_x, view.glyphs.focus, { fg = theme.accent }, g.caret_x)
  end
  local label_fg = row.locked and dim or theme.fg
  render.put(cells, g.label_x, util.truncate(row.label, g.label_x2 - g.label_x + 1, view.glyphs.ellipsis), {
    fg = label_fg,
  }, g.label_x2)

  local text = M.value_text(row)
  if row.locked then
    -- §4: the badge names which of the two reasons it is, so the user knows where to go to change
    -- it. The value column is 18 cells at 100 and 14 at 60, so the wording steps down to fit
    -- rather than overrunning the label beside it.
    local room = math.max(g.value_x2 - g.label_x2 - 1, 1)
    local badge
    for _, candidate in ipairs { "LOCKED " .. row.locked, row.locked, "LOCKED " .. SHORT_SOURCE[row.locked] } do
      if badge == nil and util.width(candidate) <= room then
        badge = candidate
      end
    end
    badge = badge or util.truncate(SHORT_SOURCE[row.locked], room, view.glyphs.ellipsis)
    render.put(cells, g.value_x2 - util.width(badge) + 1, badge, { fg = theme.unseen_fg }, g.value_x2)
    return
  end
  local shown = util.truncate(text, g.value_x2 - g.label_x2 - 1, view.glyphs.ellipsis)
  local value_fg = spec.focused and theme.fg or (row.changed and theme.fg or dim)
  render.put(cells, g.value_x2 - util.width(shown) + 1, shown, { fg = value_fg }, g.value_x2)
  if row.changed then
    render.put(cells, g.marker_x, view.glyphs.unseen, { fg = theme.accent }, g.marker_x)
  end
end

---Blits one row of the real 28-column frame into the preview box.
function M.paint_preview(cells, index, frame, g, theme, glyphs)
  local box_h = frame.rows
  if index == 1 or index == box_h + 2 then
    local left = index == 1 and glyphs.frame_tl or glyphs.frame_bl
    local right = index == 1 and glyphs.frame_tr or glyphs.frame_br
    render.put(cells, g.preview_x1, left, { fg = theme.border_idle or theme.separator }, g.preview_x1)
    for x = g.preview_x1 + 1, g.preview_x2 - 1 do
      render.put(cells, x, glyphs.rule, { fg = theme.border_idle or theme.separator }, x)
    end
    render.put(cells, g.preview_x2, right, { fg = theme.border_idle or theme.separator }, g.preview_x2)
    return
  end
  if index > box_h + 2 then
    return
  end
  render.put(cells, g.preview_x1, "│", { fg = theme.border_idle or theme.separator }, g.preview_x1)
  render.put(cells, g.preview_x2, "│", { fg = theme.border_idle or theme.separator }, g.preview_x2)
  local source = frame.cells[index - 1]
  if not source then
    return
  end
  for x = 1, math.min(#source, g.preview_x2 - g.preview_x1 - 3) do
    local cell = source[x]
    if cell and cell.ch ~= "" then
      cells[g.preview_x1 + 1 + x] = { ch = cell.ch, fg = cell.fg, bg = cell.bg, bold = cell.bold }
    end
  end
end

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

---Field the page is focused on, or nil.
function M.focused(view)
  local plan = M.plan(view)
  return plan.fields and plan.fields[plan.focus] or nil, plan
end

---One entry per key, so a binding is a row here rather than a branch. Each gets the page context
---and returns true when it consumed the key.
local KEYS = {}

local function move(delta)
  return function(ctx)
    ctx.st.focus = math.max(math.min((ctx.st.focus or 1) + delta, math.max(#ctx.shown, 1)), 1)
    return true
  end
end

KEYS.j = move(1)
KEYS.downarrow = move(1)
KEYS.k = move(-1)
KEYS.uparrow = move(-1)

KEYS.tab = function(ctx)
  ctx.st.group = ((ctx.st.group or 1) % math.max(#ctx.groups, 1)) + 1
  ctx.st.focus, ctx.st.scroll = 1, 0
  return true
end

local function nudge(delta)
  return function(ctx)
    if ctx.row and not ctx.row.locked then
      M.commit(ctx.window, ctx.row, M.step(ctx.row, delta))
    end
    return true
  end
end

KEYS.leftarrow = nudge(-1)
KEYS.rightarrow = nudge(1)

KEYS.enter = function(ctx)
  local row = ctx.row
  if not row or row.locked then
    return true
  end
  local widget = WIDGETS[row.widget]
  if not widget or not widget.activate then
    return true
  end
  local what, value = widget.activate(row)
  if what == "commit" then
    M.commit(ctx.window, row, value)
  elseif what == "edit" then
    ctx.st.editing = { key = row.key, buffer = value }
  elseif what == "record" then
    ctx.st.armed = row.key
  end
  return true
end

KEYS[" "] = KEYS.enter
KEYS.space = KEYS.enter

---`r` resets exactly the focused field to its schema default, which is what makes the `•` badge
---reversible: the file keeps only what differs, so a reset drops the key from it entirely.
KEYS.r = function(ctx)
  if ctx.row and not ctx.row.locked then
    ctx.st.armed = nil
    M.commit(ctx.window, ctx.row, ctx.row.default)
  end
  return true
end

KEYS.c = function(ctx)
  util.try(function()
    ctx.window:copy_to_clipboard(M.as_lua())
  end)
  return true
end

KEYS["/"] = function(ctx)
  ctx.st.filtering, ctx.st.filter, ctx.st.focus = true, "", 1
  return true
end

---A one-line text buffer: the same three keys whether it is a field being edited or the filter.
local function type_into(st, slot, key, on_commit, on_cancel)
  if key == "escape" then
    return on_cancel(st)
  end
  if key == "enter" then
    return on_commit(st)
  end
  if key == "backspace" then
    st[slot] = st[slot]:sub(1, -2)
    return true
  end
  if type(key) == "string" and utf8.len(key) == 1 then
    st[slot] = st[slot] .. key
  end
  return true
end

---One key from the page's own pane. Returns true when the page consumed it.
function M.key(gui_window, view, ev)
  local st = view.st
  local row, plan = M.focused(view)
  local ctx = { window = gui_window, st = st, row = row, shown = plan.fields or {}, groups = plan.groups or {} }
  if st.editing then
    return type_into(st.editing, "buffer", ev.key, function()
      if row then
        M.commit(gui_window, row, st.editing.buffer)
      end
      st.editing = nil
      return true
    end, function()
      st.editing = nil
      return true
    end)
  end
  if st.filtering then
    st.focus = 1
    return type_into(st, "filter", ev.key, function()
      st.filtering = nil
      return true
    end, function()
      st.filtering, st.filter = nil, ""
      return true
    end)
  end
  if st.armed then
    -- whatever the pty delivered is the binding; that is the only thing this side can observe
    st.armed = nil
    if ev.key == "escape" then
      return true
    end
    if row and not row.locked then
      local mods = type(ev.mods) == "table" and table.concat(ev.mods, "|") or ev.mods
      M.commit(gui_window, row, { key = ev.key, mods = (mods ~= "" and mods) or nil })
    end
    return true
  end
  local handler = KEYS[ev.key]
  return handler ~= nil and handler(ctx) or false
end

---One entry per span id, so a click target is a row here as well.
local SPANS = {
  nav = function(ctx, h)
    for i, group in ipairs(ctx.groups) do
      if group == h.nav then
        ctx.st.group, ctx.st.focus, ctx.st.scroll = i, 1, 0
      end
    end
    return true
  end,
  inc = nudge(1),
  dec = nudge(-1),
  value = function(ctx)
    return KEYS.enter(ctx)
  end,
  field = function()
    return true
  end,
}

---A click, routed by the column the hit record says it landed in.
function M.click(gui_window, view, h, x)
  local span = require("vtabs.hit").span(h, x)
  if not span then
    return false
  end
  if span ~= "nav" then
    view.st.focus = h.index or view.st.focus
  end
  local row, plan = M.focused(view)
  local ctx = {
    window = gui_window,
    st = view.st,
    row = row,
    shown = plan.fields or {},
    groups = plan.groups or {},
  }
  local handler = SPANS[span]
  return handler ~= nil and handler(ctx, h) or false
end

return M

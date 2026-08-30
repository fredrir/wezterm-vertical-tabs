local config = require "vtabs.config"
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
  if type(value) == "string" then
    return "text"
  end
  return "locked"
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
  local function expand(option, value)
    for _, name in ipairs(util.sorted_keys(value)) do
      local child = option.key .. "." .. name
      out[#out + 1] = field(child, schema.by_key[child], cfg, option.group, { label = "  " .. name, depth = 1 })
    end
  end
  for _, option in ipairs(schema.options) do
    local value = schema.get(cfg, option.key)
    if not option.container then
      local row = field(option.key, option, cfg, option.group)
      out[#out + 1] = row
      if option.open and type(row.value) == "table" then
        row.widget = "entries"
        row.entries = count_entries(row.value)
        expand(option, row.value)
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
      shown[#shown + 1] = row
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
    local entry = shown[i + scroll]
    out.rows[row] = {
      kind = "body",
      nav = nav,
      nav_selected = nav == group,
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
  { tab_id = 1, index = 1, is_active = false, is_pinned = false, title = "zsh", meta = "~/", icon = "*" },
  { tab_id = 2, index = 2, is_active = true, is_pinned = false, title = "init.lua", meta = "~/.config", icon = "*" },
  { tab_id = 3, index = 3, is_active = false, is_pinned = false, title = "cargo run", meta = "~/src", icon = "*" },
}

local HINTS_WIDE = "↑↓ field   ←→ change   ⏎ edit   r reset   c copy as Lua   esc close"
local HINTS_NARROW = "↑↓ ←→ ⏎ r c esc"

---A 28-column sidebar built from an explicitly merged table. `config.get()` is never reassigned for
---a preview, which is what keeps a half-typed hex colour out of every real sidebar in the window.
function M.preview_cells(cfg, pending, theme, glyphs, rows)
  local merged = util.merge(cfg, pending or {})
  return render.cells {
    cols = 28,
    rows = rows,
    items = SAMPLE,
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
      local text = cols >= M.PREVIEW_COLS and HINTS_WIDE or HINTS_NARROW
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
    local badge = "LOCKED"
    local shown = util.truncate(text, 14, view.glyphs.ellipsis)
    local x = g.value_x2 - util.width(shown) + 1
    render.put(cells, x, shown, { fg = dim }, g.value_x2)
    render.put(cells, x - util.width(badge) - 2, badge, { fg = theme.unseen_fg }, x - 2)
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

return M

local wezterm = require "wezterm" ---@type Wezterm
local layout = require "vtabs.layout"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local store = require "vtabs.store"

local M = {}

---Declared through `store`, so a forgotten pane or window drops its wire state with it.
local scope = store.scope "wire"
local sent = scope.pane()
local revs = scope.window()

local ARRAY = {}

---Tags a list so an empty one still encodes as `[]`, which serde's Vec insists on.
function M.array(t)
  return setmetatable(t or {}, ARRAY)
end

local ESCAPES = {
  ['"'] = '\\"',
  ["\\"] = "\\\\",
  ["\b"] = "\\b",
  ["\f"] = "\\f",
  ["\n"] = "\\n",
  ["\r"] = "\\r",
  ["\t"] = "\\t",
}

local function esc(s)
  return (s:gsub('[%z\1-\31"\\]', function(c)
    return ESCAPES[c] or string.format("\\u%04x", c:byte())
  end))
end

---Deterministic encoder: the dedupe below compares encoded strings, so key order must be stable,
---which `wezterm.json_encode` does not promise.
local function encode(v)
  local t = type(v)
  if t == "string" then
    return '"' .. esc(v) .. '"'
  elseif t == "number" then
    if v == math.floor(v) and math.abs(v) < 2 ^ 53 then
      return string.format("%d", v)
    end
    return string.format("%.17g", v)
  elseif t == "boolean" then
    return tostring(v)
  elseif t == "table" then
    if getmetatable(v) == ARRAY or v[1] ~= nil then
      local parts = {}
      for i = 1, #v do
        parts[i] = encode(v[i])
      end
      return "[" .. table.concat(parts, ",") .. "]"
    end
    local keys = {}
    for k in pairs(v) do
      keys[#keys + 1] = k
    end
    table.sort(keys)
    local parts = {}
    for _, k in ipairs(keys) do
      parts[#parts + 1] = '"' .. esc(tostring(k)) .. '":' .. encode(v[k])
    end
    return "{" .. table.concat(parts, ",") .. "}"
  end
  return "null"
end
M.encode = encode

---Any user-written colour becomes hex before it crosses the wire; Rust parses hex only.
local function hexc(c)
  if type(c) ~= "string" then
    return c
  end
  local ok, parsed = pcall(wezterm.color.parse, c)
  if not ok or parsed == nil then
    return nil
  end
  local r, g, b = parsed:srgba_u8()
  return string.format("#%02x%02x%02x", r, g, b)
end

local function normalized_overrides(user)
  local out = {}
  for k, v in pairs(user or {}) do
    if type(v) == "string" then
      out[k] = hexc(v)
    elseif type(v) == "number" or type(v) == "boolean" then
      out[k] = v
    end
  end
  return out
end

local function config_body(cfg, ctx)
  local effective = ctx.effective or {}
  local window = ctx.window_dims or {}
  return {
    desired_width = cfg.width,
    rail_width = cfg.rail_width,
    position = cfg.position,
    collapsed = cfg.collapsed,
    icons = cfg.icons,
    icon_map = cfg.icon_map or {},
    meta = cfg.meta ~= false and cfg.meta or nil,
    meta_sep = cfg.meta_sep,
    glyphs = {
      custom_block = effective.custom_block_glyphs ~= false,
      east_asian_wide = effective.treat_east_asian_ambiguous_width_as_wide == true,
    },
    animate = cfg.animations ~= "off",
    double_click_ms = cfg.double_click_ms,
    tear_off = cfg.tear_off,
    wheel = cfg.wheel,
    context = cfg.context,
    hover_timeout_ms = cfg.hover_timeout_ms,
    render = {
      meta = cfg.meta ~= false,
      padding = cfg.padding,
      frame = layout.framed(cfg),
      tab_height = cfg.tab_height,
      row_gap = cfg.row_gap,
      separator = cfg.separator,
      pinned_style = cfg.pinned_style,
      close_button = cfg.close_button,
      show_index = cfg.show_index,
      scroll_indicator = cfg.scroll_indicator == false and "never" or cfg.scroll_indicator,
      new_tab_button = not not cfg.new_tab_button,
      new_tab_label = cfg.new_tab_label,
      hover = cfg.hover,
    },
    mac = {
      integrated_buttons = ctx.chrome and ctx.chrome.integrated_buttons or false,
      native_button_style = ctx.chrome and ctx.chrome.native_button_style or false,
      preview = ctx.chrome and ctx.chrome.preview or false,
      is_full_screen = window.is_full_screen == true,
    },
  }
end

local function theme_body(cfg, ctx)
  local palette = (ctx.effective or {}).resolved_palette or {}
  local tab_bar = palette.tab_bar or {}
  local user = ctx.theme_override or cfg.theme or {}
  return {
    scheme = {
      background = hexc(palette.background),
      foreground = hexc(palette.foreground),
      cursor_bg = hexc(palette.cursor_bg),
      selection_bg = hexc(palette.selection_bg),
      active_tab_bg = hexc((tab_bar.active_tab or {}).bg_color),
      ansi = M.array(palette.ansi or {}),
      brights = M.array(palette.brights or {}),
    },
    overrides = normalized_overrides(user),
    elevation = tonumber(user.elevation),
  }
end

local function tab_record(item)
  local raw = item.raw
  return {
    id = item.tab_id,
    index = item.index,
    ["override"] = raw and raw.override or nil,
    title = raw and raw.title or (not raw and item.title) or nil,
    pane_title = raw and raw.pane_title or nil,
    proc = raw and raw.proc or nil,
    cwd = raw and raw.cwd or nil,
    host = raw and raw.host or nil,
    user = raw and raw.user or nil,
    domain = raw and raw.domain or nil,
    pinned = item.is_pinned,
    private = item.is_private or false,
    unseen = item.has_unseen,
  }
end

local function model_body(cfg, ctx, wid)
  local tabs = M.array()
  for _, item in ipairs(ctx.items or {}) do
    tabs[#tabs + 1] = tab_record(item)
  end
  local footer = M.array()
  for _, entry in ipairs(ctx.footer or {}) do
    footer[#footer + 1] = type(entry) == "string" and { text = entry } or entry
  end
  local drag = store.drag[wid]
  local buttons = M.array()
  for _, action in ipairs(layout.resolved_actions(cfg)) do
    buttons[#buttons + 1] = { id = action.id, icon = action.icon }
  end
  return {
    screen = "sidebar",
    active = ctx.active_tab_id,
    focus = { on = state.has_focus(wid), index = store.focus_index[wid] or 1 },
    scroll = { top = store.scroll[wid] or 0, user = store.user_scrolled[wid] == true },
    drag = drag and {
      id = drag.tab_id,
      active = drag.active == true,
      slot = drag.over_index,
      outside = drag.outside == true,
      origin = { x = drag.origin_x, y = drag.origin_y, at = drag.began },
    } or nil,
    strip = { buttons = buttons },
    popover = ctx.popover and (function(rect)
      local rows = M.array()
      for _, row in ipairs(rect.rows or {}) do
        local spans = M.array()
        for _, span in ipairs(row.spans or {}) do
          spans[#spans + 1] = { x = span.x, text = span.text, fg = span.fg, bold = span.bold }
        end
        rows[#rows + 1] = {
          bg = row.bg,
          fg = row.fg,
          spans = spans,
          id = row.hit and row.hit.id or nil,
          disabled = row.hit and row.hit.disabled or nil,
        }
      end
      return {
        x = rect.x,
        y = rect.y,
        w = rect.w,
        h = rect.h,
        scrim = rect.scrim,
        bg = rect.bg,
        rows = rows,
      }
    end)(ctx.popover) or nil,
    footer = footer,
    tabs = tabs,
    private = state.is_private(wid),
  }
end

---Bumps the per-window rev only when the body changed; the encoded string is the change detector.
local function versioned(wid, kind, body)
  local per = revs[wid]
  if not per then
    per = {}
    revs[wid] = per
  end
  local entry = per[kind]
  if not entry or entry.body ~= body then
    entry = { rev = (entry and entry.rev or 0) + 1, body = body }
    per[kind] = entry
    entry.line = string.format('{"t":"%s","rev":%d,%s', kind, entry.rev, body:sub(2))
  end
  return entry.line
end

---Sends the v2 state to every ready v2 sidebar in the window, skipping unchanged sends per pane.
function M.sync(gui_window, ctx)
  local wid = gui_window:window_id()
  local cfg = ctx.cfg
  local lines = {
    config = versioned(wid, "config", encode(config_body(cfg, ctx))),
    theme = versioned(wid, "theme", encode(theme_body(cfg, ctx))),
    model = versioned(wid, "model", encode(model_body(cfg, ctx, wid))),
  }
  for _, info in ipairs(gui_window:mux_window():tabs_with_info()) do
    local sb = sidebar.find(info.tab)
    if sb and sidebar.is_ready(sb) and (store.proto[sb:pane_id()] or 1) >= 2 then
      local pid = sb:pane_id()
      local seen = sent[pid]
      if not seen then
        seen = {}
        sent[pid] = seen
      end
      for _, kind in ipairs { "config", "theme", "model" } do
        local line = lines[kind]
        if seen[kind] ~= line and sidebar.send_raw(sb, line) then
          seen[kind] = line
        end
      end
    end
  end
end

return M

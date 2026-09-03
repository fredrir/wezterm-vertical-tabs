local config = require "vtabs.config"
local protocol = require "vtabs.gen.protocol"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local store = require "vtabs.store"
local theme_bridge = require "vtabs.theme_bridge"
local util = require "vtabs.util"

local M = {}

---Declared through `store`, so a forgotten pane or window drops its wire state with it.
local scope = store.scope "wire"
local sent = scope.pane()
local revs = scope.window()
local generations = scope.window()
local debug_metrics = scope.window()

local function note_delivery(wid, cfg, mode, changed, bytes, committed)
  local metrics = debug_metrics[wid] or { deliveries = 0, sections = 0, bytes = 0, commits = 0 }
  debug_metrics[wid] = metrics
  metrics.deliveries = metrics.deliveries + 1
  metrics.sections = metrics.sections + #changed
  metrics.bytes = metrics.bytes + bytes
  metrics.commits = metrics.commits + (committed and 1 or 0)
  if cfg.debug then
    util.log(
      "wire: window=%d mode=%s changed=%s bytes=%d totals={deliveries=%d,sections=%d,bytes=%d,commits=%d}",
      wid,
      mode,
      table.concat(changed, ","),
      bytes,
      metrics.deliveries,
      metrics.sections,
      metrics.bytes,
      metrics.commits
    )
  end
end

---A ready event means the process in this pane has an empty model even when the pane id survived.
---Forget only delivery state; authentication and lifecycle state still belong to the pane.
function M.reset_pane(pane_id)
  sent[pane_id] = nil
end

---True once a section has reached the pane: a bare pane is written to whichever tab it is in.
function M.dressed(pane_id)
  return sent[pane_id] ~= nil
end

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
local function encode_value(v, seen, depth)
  local t = type(v)
  if t == "string" then
    return '"' .. esc(v) .. '"'
  elseif t == "number" then
    if v ~= v or v == math.huge or v == -math.huge then
      return "null"
    end
    if v == math.floor(v) and math.abs(v) < 2 ^ 53 then
      return string.format("%d", v)
    end
    return string.format("%.17g", v)
  elseif t == "boolean" then
    return tostring(v)
  elseif t == "table" then
    if seen[v] or depth >= 64 then
      return "null"
    end
    seen[v] = true
    local tagged = getmetatable(v) == ARRAY
    local array_like = tagged or v[1] ~= nil
    if array_like then
      local count, last = 0, 0
      for key in pairs(v) do
        if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
          seen[v] = nil
          return "null"
        end
        count = count + 1
        last = math.max(last, key)
      end
      if count ~= last then
        seen[v] = nil
        return "null"
      end
      local parts = {}
      for i = 1, #v do
        parts[i] = encode_value(v[i], seen, depth + 1)
      end
      seen[v] = nil
      return "[" .. table.concat(parts, ",") .. "]"
    end
    local keys = {}
    for k in pairs(v) do
      if type(k) ~= "string" then
        seen[v] = nil
        return "null"
      end
      keys[#keys + 1] = k
    end
    table.sort(keys)
    local parts = {}
    for _, k in ipairs(keys) do
      parts[#parts + 1] = '"' .. esc(k) .. '":' .. encode_value(v[k], seen, depth + 1)
    end
    seen[v] = nil
    return "{" .. table.concat(parts, ",") .. "}"
  end
  return "null"
end
local function encode(v)
  return encode_value(v, {}, 0)
end
M.encode = encode

local function palette_list(values)
  local out = M.array()
  for _, value in ipairs(type(values) == "table" and values or {}) do
    local colour = theme_bridge.colour(value)
    if colour then
      out[#out + 1] = colour
    end
  end
  return out
end

---The renderer-facing config surface, normalised before it crosses the wire.
function M.render_section(cfg)
  return {
    meta = cfg.meta ~= false,
    padding = cfg.padding,
    frame = config.framed(cfg),
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
  }
end

local function config_body(cfg, ctx)
  local effective = ctx.effective or {}
  return {
    rail_width = cfg.rail_width,
    position = cfg.position,
    icons = cfg.icons,
    icon_map = cfg.icon_map or {},
    meta = cfg.meta ~= false and cfg.meta or nil,
    meta_sep = cfg.meta_sep,
    glyphs = {
      custom_block = effective.custom_block_glyphs ~= false,
      east_asian_wide = effective.treat_east_asian_ambiguous_width_as_wide == true,
    },
    double_click_ms = cfg.double_click_ms,
    tear_off = cfg.tear_off,
    wheel = cfg.wheel,
    context = cfg.context,
    hover_timeout_ms = cfg.hover_timeout_ms,
    hover_highlight = cfg.hover_highlight ~= false,
    ellipsis = cfg.ellipsis,
    popover = {
      width = cfg.popover.width,
      follow_pointer = cfg.popover.follow_pointer,
      overflow = cfg.popover.overflow,
    },
    render = M.render_section(cfg),
  }
end

local function theme_body(cfg, ctx)
  local palette = (ctx.effective or {}).resolved_palette or {}
  local tab_bar = palette.tab_bar or {}
  local user = ctx.theme_base or cfg.theme or {}
  return {
    scheme = {
      background = theme_bridge.colour(palette.background),
      foreground = theme_bridge.colour(palette.foreground),
      cursor_bg = theme_bridge.colour(palette.cursor_bg),
      active_tab_bg = theme_bridge.colour((tab_bar.active_tab or {}).bg_color),
      ansi = palette_list(palette.ansi),
      brights = palette_list(palette.brights),
    },
    overrides = theme_bridge.overrides(user),
    hook = type((cfg.hooks or {}).theme) == "function" or nil,
    private = ctx.private == true or nil,
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
    icon = raw and raw.icon or nil,
    cwd = raw and raw.cwd or nil,
    host = raw and raw.host or nil,
    user = raw and raw.user or nil,
    domain = raw and raw.domain or nil,
    pinned = item.is_pinned,
    unseen = item.has_unseen,
    settings = item.is_settings or nil,
  }
end

---The switcher only earns its row once there is something to switch between.
local function spaces_body(list)
  if type(list) ~= "table" or #list < 2 then
    return nil
  end
  local out = M.array()
  for _, space in ipairs(list) do
    out[#out + 1] = { id = space.id, name = space.name, icon = space.icon, unseen = space.unseen }
  end
  return out
end

local function model_body(cfg, ctx, wid)
  local tabs = M.array()
  for _, item in ipairs(ctx.items or {}) do
    tabs[#tabs + 1] = tab_record(item)
  end
  local spaces = spaces_body(ctx.spaces)
  local footer = M.array()
  for _, entry in ipairs(ctx.footer or {}) do
    footer[#footer + 1] = type(entry) == "string" and { text = entry } or entry
  end
  local drag = store.drag[wid]
  local buttons = M.array()
  for _, action in ipairs(require("vtabs.actions").resolved_strip(cfg)) do
    buttons[#buttons + 1] = { id = action.id, icon = action.icon }
  end
  return {
    screen = "sidebar",
    rail = state.is_collapsed(wid) and cfg.collapsed == "rail" or false,
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
    strip = ctx.strip and {
      dpi = ctx.strip.dpi,
      chrome = ctx.strip.chrome,
      buttons = buttons,
    } or { buttons = buttons },
    footer = footer,
    tabs = tabs,
    private = state.is_private(wid),
    space = spaces and ctx.space or nil,
    spaces = spaces,
  }
end

local memo = scope.window()

---Config and theme are built from a handful of tables that only change on a reload, a toggle or a
---space switch; while every one of them is the same table as last poll, so is the encoded body.
local function settled(wid, kind, ctx, build)
  local per = memo[wid]
  if not per then
    per = {}
    memo[wid] = per
  end
  local seen = per[kind]
  local same = seen ~= nil
    and seen.cfg == ctx.cfg
    and seen.effective == ctx.effective
    and seen.theme_base == ctx.theme_base
    and seen.private == ctx.private
  if same then
    return seen.body
  end
  local body = encode(build(ctx.cfg, ctx))
  per[kind] = {
    cfg = ctx.cfg,
    effective = ctx.effective,
    theme_base = ctx.theme_base,
    private = ctx.private,
    body = body,
  }
  return body
end

---Bumps the per-window rev only when the body changed; the encoded string is the change detector.
---`tag` is the wire's `t` when it differs from the dedupe kind: two models share one tag.
local function versioned(wid, kind, body, tag)
  local per = revs[wid]
  if not per then
    per = {}
    revs[wid] = per
  end
  local entry = per[kind]
  if not entry or entry.body ~= body then
    entry = { rev = (entry and entry.rev or 0) + 1, body = body }
    per[kind] = entry
    entry.line = string.format('{"t":"%s","rev":%d,%s', tag or kind, entry.rev, body:sub(2))
  end
  return entry.line
end

local SEMANTIC = { "config", "theme", "spaces", "model", "settings", "menu" }

---One window generation covers all of the semantic sections. Per-section revisions remain in their
---v2 messages for compatibility; atomic backends use the generation to stage them as one state.
local function generation_for(wid, lines)
  local current = generations[wid]
  if not current then
    current = { value = 0, lines = {} }
    generations[wid] = current
  end
  local changed = false
  for _, kind in ipairs(SEMANTIC) do
    if current.lines[kind] ~= lines[kind] then
      changed = true
    end
  end
  if changed then
    current.value = current.value + 1
    for _, kind in ipairs(SEMANTIC) do
      current.lines[kind] = lines[kind]
    end
  end
  return current.value
end

function M.generation(window_id)
  local current = generations[window_id]
  return current and current.value or nil
end

function M.revision(window_id, kind)
  local per = revs[window_id]
  local entry = per and per[kind]
  return entry and entry.rev or nil
end

local function sendable(wid, kinds, lines)
  for _, kind in ipairs(kinds) do
    local line = lines[kind]
    if line and #line > protocol.LINE_MAX then
      util.warn_once(
        string.format("wire-size-%d-%s", wid, kind),
        "backend %s section is too large (%d > %d bytes); keeping the last committed view",
        kind,
        #line,
        protocol.LINE_MAX
      )
      return false
    end
  end
  return true
end

M.sendable = sendable

---Sends the v2 state to the sidebar the window shows, skipping unchanged sends per pane. A sidebar
---in a background tab is left as it is and catches up the poll its tab comes to the front: nothing
---it paints meanwhile can be seen, and every frame it did paint would cross the mux and be rendered
---all the same. A pane that has never been dressed is the exception, so it paints at all.
function M.sync(gui_window, ctx)
  local wid = gui_window:window_id()
  local cfg = ctx.cfg
  local menu = require("vtabs.popover").wire_body(gui_window, ctx.survey)
  local lines = {
    config = versioned(wid, "config", settled(wid, "config", ctx, config_body)),
    theme = versioned(wid, "theme", settled(wid, "theme", ctx, theme_body)),
    spaces = versioned(wid, "spaces", encode(require("vtabs.spaces").body(cfg, wid, ctx.survey.all, M.array))),
    model = versioned(wid, "model", encode(model_body(cfg, ctx, wid))),
    menu = versioned(wid, "menu", encode(menu or { open = false })),
  }
  local mux_win = gui_window:mux_window()
  local settings = require "vtabs.settings"
  local page_tab, page_pane = settings.find(mux_win, ctx.snapshot)
  if page_pane then
    local body = require("vtabs.settings_model").body(cfg, M.array)
    lines.settings = versioned(wid, "settings", encode(body))
  end
  local generation = generation_for(wid, lines)

  local function push_legacy(pane, kinds)
    if not sendable(wid, kinds, lines) then
      return false
    end
    local pid = pane:pane_id()
    local seen = sent[pid] or {}
    for _, kind in ipairs(kinds) do
      local line = lines[kind]
      if seen[kind] ~= line and sidebar.send_raw(pane, line) then
        sent[pid] = seen
        seen[kind] = line
        note_delivery(wid, cfg, "legacy", { kind }, #line, false)
      end
    end
  end

  local function push_atomic(pane, kinds)
    if not sendable(wid, kinds, lines) then
      return false
    end
    local pid = pane:pane_id()
    local seen = sent[pid] or {}
    local changed, batch = {}, { string.format('{"t":"begin","generation":%d}', generation) }
    for _, kind in ipairs(kinds) do
      local line = lines[kind]
      if line ~= nil and seen[kind] ~= line then
        changed[#changed + 1] = kind
        batch[#batch + 1] = line
      end
    end
    if #changed == 0 then
      return true
    end
    batch[#batch + 1] = string.format('{"t":"commit","generation":%d}', generation)
    local payload = table.concat(batch, "\n")
    if not sidebar.send_raw(pane, payload) then
      return false
    end
    -- Delivery is all-or-nothing from Lua's point of view: a failed write advances no section.
    sent[pid] = seen
    for _, kind in ipairs(changed) do
      seen[kind] = lines[kind]
    end
    seen.generation = generation
    note_delivery(wid, cfg, "atomic", changed, #payload, true)
    return true
  end

  local function push(pane, kinds)
    if sidebar.supports(pane, "atomic_sync") then
      return push_atomic(pane, kinds)
    end
    return push_legacy(pane, kinds)
  end
  local function shown_or_bare(pane, tab_id)
    return sent[pane:pane_id()] == nil or (ctx.active_tab_id ~= nil and tab_id == ctx.active_tab_id)
  end
  local function speaks_v2(pane, panes)
    return sidebar.is_ready(pane, panes) and (store.proto[pane:pane_id()] or 1) >= 2
  end
  local observed_tabs = ctx.snapshot and ctx.snapshot.tabs or mux_win:tabs_with_info()
  for _, observed in ipairs(observed_tabs) do
    local info = observed.info or observed
    local tab_id = observed.tab_id or info.tab:tab_id()
    local sb
    if ctx.snapshot then
      sb = observed.sidebar
    else
      sb = sidebar.find(info.tab)
    end
    if sb and shown_or_bare(sb, tab_id) and not sidebar.is_settings(sb) and speaks_v2(sb, observed.panes) then
      if sidebar.supports(sb, "spaces_policy") then
        push(sb, { "config", "theme", "spaces", "model", "menu" })
      else
        push(sb, { "config", "theme", "model", "menu" })
      end
    end
  end
  -- The settings pane shares config and theme; its raw host projection becomes a Rust-owned model.
  local page_id = page_tab and page_tab:tab_id() or nil
  local settings_panes = ctx.snapshot and ctx.snapshot.settings and ctx.snapshot.settings.entry.panes or nil
  if page_pane and shown_or_bare(page_pane, page_id) and speaks_v2(page_pane, settings_panes) then
    if sidebar.supports(page_pane, "settings_document") then
      if sidebar.supports(page_pane, "spaces_policy") then
        push(page_pane, { "config", "theme", "spaces", "settings" })
      else
        push(page_pane, { "config", "theme", "settings" })
      end
    else
      util.warn_once(
        "settings-document-" .. page_pane:pane_id(),
        "settings backend lacks settings_document; update the bundled backend"
      )
    end
  end
end

return M

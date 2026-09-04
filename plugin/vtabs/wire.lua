local config = require "vtabs.config"
local protocol = require "vtabs.gen.protocol"
local sidebar = require "vtabs.sidebar"
local state = require "vtabs.state"
local store = require "vtabs.store"
local theme_bridge = require "vtabs.theme_bridge"
local transport = require "vtabs.transport"
local util = require "vtabs.util"

local M = {}

local POLICY_RESET_MS = 3000
M.POLICY_RESET_MS = POLICY_RESET_MS

---Declared through `store`, so a forgotten pane or window drops its wire state with it.
local scope = store.scope "wire"
local sent = scope.pane()
local debug_metrics = scope.window()
local policy = scope.window()
local policy_seen = scope.window()
local policy_owner = scope.process()
local policy_reset = scope.process()
local policy_failed = scope.pane()
local policy_recovered = scope.pane()

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

---The sole pane allowed to project Rust's space/theme result onto window-global host state. It is
---stable across tab activation; an unarmed pane is still resetting and has no authority yet.
function M.policy_pane(window_id)
  local current = policy[window_id]
  return current and current.pane_id or nil
end

function M.is_policy_authority(window_id, pane_id)
  local current = policy[window_id]
  return current ~= nil and current.pane_id == pane_id and current.armed == true
end

---A reset candidate becomes eligible only after the backend has acknowledged its fresh auth with
---a structurally valid Ready. The first complete publication arms it below.
function M.policy_ready(window_id, pane_id)
  local current = policy[window_id]
  if not current or current.pane_id ~= pane_id then
    if policy_failed[pane_id] == true then
      policy_failed[pane_id] = nil
      if current == nil then
        policy_recovered[pane_id] = true
      end
    end
    return false
  end
  current.ready = true
  current.armed = false
  sent[pane_id] = nil
  return true
end

local function invalidate_policy_pane(window_id, pane_id, force)
  if pane_id == nil then
    return false
  end
  local owner = policy_owner[pane_id]
  if not force and owner ~= nil and owner ~= window_id then
    return false
  end
  sent[pane_id] = nil
  store.ready[pane_id] = nil
  transport.forget(pane_id)
  policy_reset[pane_id] = true
  if force or owner == window_id then
    policy_owner[pane_id] = nil
  end
  return true
end

local function candidate_by_id(candidates, pane_id)
  for _, candidate in ipairs(candidates) do
    if candidate.pane:pane_id() == pane_id then
      return candidate
    end
  end
  return nil
end

local function choose_policy_candidate(candidates, active_tab_id)
  local function eligible(candidate)
    local pid = candidate.pane:pane_id()
    return candidate.ready
      and policy_failed[pid] ~= true
      and store.given_up[pid] ~= true
      and store.quitting[pid] ~= true
  end
  for _, candidate in ipairs(candidates) do
    if candidate.role == "sidebar" and eligible(candidate) and candidate.tab_id == active_tab_id then
      return candidate
    end
  end
  for _, candidate in ipairs(candidates) do
    if candidate.role == "sidebar" and eligible(candidate) then
      return candidate
    end
  end
  for _, candidate in ipairs(candidates) do
    if eligible(candidate) then
      return candidate
    end
  end
  return nil
end

---Keeps one policy pane until it disappears or changes role. A successor that may already hold a
---staged publication is reset with a fresh auth token; the same token is retried after a failed
---write, never rotated again for the same handoff.
local function ensure_policy(gui_window, candidates, active_tab_id)
  local wid = gui_window:window_id()
  local current = policy[wid]
  if current then
    local existing = candidate_by_id(candidates, current.pane_id)
    local pid = current.pane_id
    local unavailable = store.given_up[pid] == true or store.quitting[pid] == true
    local expired = current.resetting and current.deadline ~= nil and util.now_ms() >= current.deadline
    if existing and existing.role == current.role and not unavailable and not expired then
      if current.resetting and not current.auth_sent and current.frame_token then
        current.auth_sent = sidebar.auth(existing.pane, current.frame_token) == true
      end
      return current
    end
    invalidate_policy_pane(wid, pid, false)
    if unavailable or expired then
      policy_failed[pid] = true
    end
    policy[wid] = nil
  end

  local candidate = choose_policy_candidate(candidates, active_tab_id)
  if not candidate then
    return nil
  end
  local pid = candidate.pane:pane_id()
  local recovered = policy_recovered[pid] == true
  for _, other in ipairs(candidates) do
    local other_pid = other.pane:pane_id()
    if other_pid ~= pid then
      policy_recovered[other_pid] = nil
    end
  end
  local needs_reset = not recovered
    and (
      policy_seen[wid] == true
      or policy_reset[pid] == true
      or sent[pid] ~= nil
      or (policy_owner[pid] ~= nil and policy_owner[pid] ~= wid)
    )
  current = {
    pane_id = pid,
    role = candidate.role,
    ready = candidate.ready and not needs_reset,
    armed = false,
    resetting = needs_reset,
    deadline = needs_reset and (util.now_ms() + POLICY_RESET_MS) or nil,
  }
  policy[wid] = current
  policy_seen[wid] = true
  policy_owner[pid] = wid
  sent[pid] = nil
  if recovered then
    policy_recovered[pid] = nil
    policy_reset[pid] = nil
  end
  if needs_reset then
    local frame_token = state.token_for(pid)
    current.frame_token = frame_token
    invalidate_policy_pane(wid, pid, true)
    policy_owner[pid] = wid
    if type(frame_token) == "string" and frame_token ~= "" then
      state.set_token(pid, util.random_token())
      current.auth_sent = sidebar.auth(candidate.pane, frame_token) == true
    else
      current.auth_sent = false
    end
  end
  return current
end

local function arm_policy(window_id, pane_id)
  local current = policy[window_id]
  if not current or current.pane_id ~= pane_id or not current.ready then
    return
  end
  current.armed = true
  current.resetting = false
  current.deadline = nil
  current.frame_token = nil
  policy_reset[pane_id] = nil
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
    scroll_indicator = cfg.scroll_indicator,
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
    hover_highlight = cfg.hover_highlight,
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
    hook = type((cfg.hooks or {}).theme) == "function",
    private = ctx.private == true,
  }
end

local function model_body(cfg, ctx, wid)
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

local function section(kind, body)
  return string.format('{"t":"%s",%s', kind, body:sub(2))
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

---Sends changed state to the shown pane and the stable policy pane. Other background panes catch up
---when shown because their pixels cannot be seen; the policy pane stays current in the background
---so its ordered Rust results remain authoritative for the whole window. Every bare pane is dressed
---once regardless.
function M.sync(gui_window, ctx)
  local wid = gui_window:window_id()
  local cfg = ctx.cfg
  local menu = require("vtabs.popover").wire_body(gui_window, ctx.survey)
  local lines = {
    config = section("config", settled(wid, "config", ctx, config_body)),
    theme = section("theme", settled(wid, "theme", ctx, theme_body)),
    spaces = section("spaces", encode(require("vtabs.spaces").body(cfg, wid, ctx.survey.all, M.array))),
    model = section("model", encode(model_body(cfg, ctx, wid))),
    menu = section("menu", encode(menu or { open = false })),
  }
  local mux_win = gui_window:mux_window()
  local settings = require "vtabs.settings"
  local page_tab, page_pane = settings.find(mux_win, ctx.snapshot)
  if page_pane then
    local body = require("vtabs.settings_model").body(cfg, M.array)
    lines.settings = section("settings", encode(body))
  end
  local observed_tabs = ctx.snapshot and ctx.snapshot.tabs or mux_win:tabs_with_info()
  local deliveries = {}
  local candidates = {}
  for _, observed in ipairs(observed_tabs) do
    local info = observed.info or observed
    local tab_id = observed.tab_id or info.tab:tab_id()
    local sb = ctx.snapshot and observed.sidebar or sidebar.find(info.tab)
    if sb and not sidebar.is_settings(sb) then
      local ready = sidebar.is_ready(sb, observed.panes)
      deliveries[#deliveries + 1] = {
        pane = sb,
        tab_id = tab_id,
        ready = ready,
        kinds = { "config", "theme", "spaces", "model", "menu" },
      }
      candidates[#candidates + 1] = { pane = sb, tab_id = tab_id, ready = ready, role = "sidebar" }
    end
  end
  local page_id = page_tab and page_tab:tab_id() or nil
  local settings_panes = ctx.snapshot and ctx.snapshot.settings and ctx.snapshot.settings.entry.panes or nil
  if page_pane then
    local ready = sidebar.is_ready(page_pane, settings_panes)
    deliveries[#deliveries + 1] = {
      pane = page_pane,
      tab_id = page_id,
      ready = ready,
      kinds = { "config", "theme", "spaces", "settings" },
    }
    candidates[#candidates + 1] = { pane = page_pane, tab_id = page_id, ready = ready, role = "settings" }
  end
  ensure_policy(gui_window, candidates, ctx.active_tab_id)

  local function push(pane, kinds)
    if not sendable(wid, kinds, lines) then
      return false
    end
    local pid = pane:pane_id()
    local seen = sent[pid] or {}
    local changed, batch = {}, { '{"t":"begin"}' }
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
    batch[#batch + 1] = '{"t":"commit"}'
    local payload = table.concat(batch, "\n")
    if not sidebar.send_raw(pane, payload) then
      return false
    end
    -- Delivery is all-or-nothing from Lua's point of view: a failed write advances no section.
    sent[pid] = seen
    for _, kind in ipairs(changed) do
      seen[kind] = lines[kind]
    end
    arm_policy(wid, pid)
    note_delivery(wid, cfg, "atomic", changed, #payload, true)
    return true
  end

  local function shown_or_bare(pane, tab_id)
    return sent[pane:pane_id()] == nil or (ctx.active_tab_id ~= nil and tab_id == ctx.active_tab_id)
  end
  for _, delivery in ipairs(deliveries) do
    local pid = delivery.pane:pane_id()
    local authority = policy[wid]
    local waiting_for_ready = authority ~= nil and authority.pane_id == pid and authority.ready ~= true
    if
      delivery.ready
      and not waiting_for_ready
      and (shown_or_bare(delivery.pane, delivery.tab_id) or M.policy_pane(wid) == pid)
    then
      push(delivery.pane, delivery.kinds)
    end
  end
end

return M

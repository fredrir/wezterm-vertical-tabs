local backend = require "vtabs.backend"
local state = require "vtabs.state"
local store = require "vtabs.store"
local util = require "vtabs.util"

---Rust owns space validation, routing, sticky assignment, summaries and theme selection. This
---module is only the WezTerm boundary: it sends raw snapshot/state facts, executes route hooks when
---Rust asks, and applies generation-bound results to the mux-side state cache.
local M = {}

local scope = store.scope "spaces"
local route_key = scope.tab()
local last_active = scope.window()
local last_tab = scope.window()
local dynamics = scope.window()
local resolved = scope.window()
local hook_answers = scope.window()

local function raw_match(value, array)
  if type(value) ~= "table" then
    return value
  end
  local out = {}
  for field, pattern in pairs(value) do
    if type(pattern) == "table" then
      local count, last, dense = 0, 0, true
      for key in pairs(pattern) do
        if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
          dense = false
          break
        end
        count = count + 1
        last = math.max(last, key)
      end
      if dense and count == last then
        local list = array()
        for i = 1, count do
          list[#list + 1] = pattern[i]
        end
        out[field] = list
      else
        -- Preserve an invalid shape for the safe encoder (which lowers cycles/mixed keys to null),
        -- so Rust can reject this field without poisoning the whole transaction.
        out[field] = pattern
      end
    else
      out[field] = pattern
    end
  end
  return out
end

function M.enabled(cfg)
  return #(type(cfg.spaces) == "table" and cfg.spaces or {}) > 0 or type((cfg.hooks or {}).route) == "function"
end

local function definitions(cfg, array)
  local out = array()
  for _, entry in ipairs(type(cfg.spaces) == "table" and cfg.spaces or {}) do
    if type(entry) == "table" then
      local theme = entry.theme
      if type(theme) == "table" then
        theme = require("vtabs.theme_bridge").raw_overrides(theme)
      end
      out[#out + 1] = {
        id = entry.id,
        name = entry.name,
        icon = entry.icon,
        theme = theme,
        match = raw_match(entry.match, array),
      }
    else
      out[#out + 1] = entry
    end
  end
  return out
end

local function dynamic_facts(wid, array)
  local out = array()
  for id, meta in pairs(dynamics[wid] or {}) do
    if type(id) == "string" and type(meta) == "table" then
      local seq = tonumber(meta.seq)
      if not seq or seq < 0 or seq == math.huge or seq ~= math.floor(seq) then
        seq = 9007199254740991
      end
      out[#out + 1] = { id = id, name = meta.name, template = meta.template, seq = seq }
    end
  end
  table.sort(out, function(a, b)
    local sa, sb = a.seq or math.huge, b.seq or math.huge
    return sa == sb and a.id < b.id or sa < sb
  end)
  return out
end

local function last_tab_facts(wid, array)
  local out = array()
  for space_id, tab_id in pairs(last_tab[wid] or {}) do
    out[#out + 1] = { space_id = space_id, tab_id = tab_id }
  end
  table.sort(out, function(a, b)
    return a.space_id < b.space_id
  end)
  return out
end

local function tab_fact(item)
  local raw = item.raw or {}
  return {
    id = item.tab_id,
    index = item.index,
    ["override"] = raw.override,
    title = raw.title,
    pane_title = raw.pane_title,
    proc = raw.proc,
    icon = raw.icon,
    cwd = raw.cwd,
    host = raw.host,
    user = raw.user,
    domain = raw.domain,
    pinned = item.is_pinned,
    unseen = item.has_unseen,
    settings = item.is_settings or nil,
    remote = not backend.is_local(raw.domain, raw.host),
    space = state.space_of(item.tab_id),
    manual = state.space_manual(item.tab_id),
    fingerprint = route_key[item.tab_id],
  }
end

---Raw policy input shared by sidebar and settings panes in the same atomic generation.
function M.body(cfg, wid, items, array)
  local tabs = array()
  local active_tab = nil
  for _, item in ipairs(items or {}) do
    tabs[#tabs + 1] = tab_fact(item)
    if item.is_active then
      active_tab = item.tab_id
    end
  end
  local follow = last_active[wid]
  return {
    window_id = wid,
    enabled = M.enabled(cfg),
    hook = type((cfg.hooks or {}).route) == "function" or nil,
    definitions = definitions(cfg, array),
    tabs = tabs,
    active_tab = active_tab,
    active_space = state.active_space(wid),
    follow = follow and { tab_id = follow.tab, space = follow.space } or nil,
    last_tabs = last_tab_facts(wid, array),
    dynamics = dynamic_facts(wid, array),
  }
end

local function warning_text(warning)
  local suffix = warning.space_id and (" " .. string.format("%q", warning.space_id)) or ""
  if warning.field then
    suffix = suffix .. "." .. tostring(warning.field)
  end
  return "spaces: " .. tostring(warning.code or "invalid") .. suffix
end

---Accepts only the current semantic generation. Multiple sidebar panes may answer for one window;
---the first valid result applies shared state and later duplicates are inert.
function M.accept(ev, expected_window, current_generation)
  local wid = tonumber(ev and ev.window_id)
  local generation = tonumber(ev and ev.generation)
  if not wid or wid ~= tonumber(expected_window) or not generation or generation ~= tonumber(current_generation) then
    return false
  end
  local previous = resolved[wid]
  if previous and previous.generation == generation then
    return false
  end
  for _, assignment in ipairs(type(ev.assignments) == "table" and ev.assignments or {}) do
    local tab_id = tonumber(assignment.tab_id)
    if tab_id then
      state.set_space(tab_id, type(assignment.space) == "string" and assignment.space or nil, assignment.manual == true)
      route_key[tab_id] = type(assignment.fingerprint) == "string" and assignment.fingerprint or nil
    end
  end
  local next_dynamics = {}
  for _, dynamic in ipairs(type(ev.dynamics) == "table" and ev.dynamics or {}) do
    if type(dynamic.id) == "string" then
      next_dynamics[dynamic.id] = {
        name = type(dynamic.name) == "string" and dynamic.name or dynamic.id,
        template = type(dynamic.template) == "string" and dynamic.template or nil,
        seq = tonumber(dynamic.seq) or math.huge,
      }
    end
  end
  -- Rust's result is authoritative: empty dynamic spaces disappear instead of leaking into later
  -- windows or surviving after their last tab closes.
  dynamics[wid] = next_dynamics
  state.set_active_space(wid, type(ev.active) == "string" and ev.active or nil)
  local follow = type(ev.follow) == "table" and ev.follow or nil
  last_active[wid] = follow and { tab = tonumber(follow.tab_id), space = follow.space } or nil
  local remembered = {}
  for _, entry in ipairs(type(ev.last_tabs) == "table" and ev.last_tabs or {}) do
    if type(entry.space_id) == "string" and tonumber(entry.tab_id) then
      remembered[entry.space_id] = tonumber(entry.tab_id)
    end
  end
  last_tab[wid] = remembered
  for _, warning in ipairs(type(ev.warnings) == "table" and ev.warnings or {}) do
    util.warn_once("space-policy-" .. tostring(warning.code), "%s", warning_text(warning))
  end
  resolved[wid] = { generation = generation, value = ev }
  return true
end

---Projects the last Rust answer onto freshly collected item handles. No answer (or an old backend)
---degrades to one unpartitioned list; Lua never runs a second routing implementation.
function M.project(cfg, wid, items, capable)
  if not M.enabled(cfg) then
    return { all = items, visible = items }
  end
  if not capable then
    util.warn_once("spaces-policy", "spaces need a backend advertising spaces_policy; showing all tabs")
    return { all = items, visible = items }
  end
  local cached = resolved[wid] and resolved[wid].value or nil
  if not cached then
    return { all = items, visible = items }
  end
  local by_id = {}
  for _, item in ipairs(items) do
    item.space = state.space_of(item.tab_id)
    by_id[item.tab_id] = item
  end
  local visible = {}
  for _, id in ipairs(type(cached.visible_tab_ids) == "table" and cached.visible_tab_ids or {}) do
    local item = by_id[tonumber(id)]
    if item then
      visible[#visible + 1] = item
    end
  end
  return {
    all = items,
    visible = visible,
    space = cached.active,
    spaces = type(cached.summary) == "table" and cached.summary or nil,
  }
end

function M.set_active(wid, id)
  if state.active_space(wid) == id then
    return false
  end
  state.set_active_space(wid, id)
  store.scroll[wid] = nil
  store.user_scrolled[wid] = nil
  store.drag[wid] = nil
  return true
end

function M.last_tab_in(wid, id)
  return last_tab[wid] and last_tab[wid][id] or nil
end

function M.move(tab_id, id, manual)
  state.set_space(tab_id, id or state.space_of(tab_id), id ~= nil and manual == true)
  route_key[tab_id] = nil
  for wid in pairs(resolved) do
    resolved[wid] = nil
  end
end

local function prune_answers(per, generation)
  for old in pairs(per) do
    if type(old) == "number" and old + 4 < generation then
      per[old] = nil
    end
  end
end

---Runs one shared hook batch per window/generation and sends the cached answer to every backend
---pane that requested it. Rust decides which tabs need asking and how each answer affects routing.
function M.answer_hook(gui_window, pane, ev)
  local wid = tonumber(ev and ev.window_id)
  local generation = tonumber(ev and ev.generation)
  if not wid or not generation or wid ~= gui_window:window_id() then
    return false
  end
  if generation ~= tonumber(require("vtabs.wire").generation(wid)) then
    return false
  end
  local per = hook_answers[wid] or {}
  hook_answers[wid] = per
  local routes = per[generation]
  if routes == nil then
    routes = {}
    local hook = (require("vtabs.config").get().hooks or {}).route
    for _, facts in ipairs(type(ev.tabs) == "table" and ev.tabs or {}) do
      local space = nil
      if type(hook) == "function" then
        local ok, answer = pcall(hook, facts)
        if not ok then
          util.warn_once("hook-route", "route hook failed: %s", tostring(answer))
        elseif type(answer) == "string" and answer ~= "" then
          space = answer
        end
      end
      routes[#routes + 1] = { tab_id = facts.tab_id, space = space }
    end
    per[generation] = routes
    prune_answers(per, generation)
  end
  return require("vtabs.sidebar_identity").send(
    pane,
    { t = "space_route_hook_result", generation = generation, routes = routes }
  )
end

return M

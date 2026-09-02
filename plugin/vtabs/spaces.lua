local backend = require "vtabs.backend"
local protocol = require "vtabs.gen.protocol"
local state = require "vtabs.state"
local store = require "vtabs.store"
local util = require "vtabs.util"

---Spaces are per-window partitions of the tab list. This module is mux-free: rules, assignment and
---the active-space bookkeeping take a `cfg` and plain facts, so the pure half tests without a fake mux.
---The verbs that touch tabs (`switch_space`, `move_to_space`) live in `actions`.
local M = {}

M.MAX = protocol.MODEL_MAX_SPACES

-- A remote host may write the strings a template expands, so the id it produces is capped.
local ID_MAX = 48
local FIELDS = { "domain", "host", "user", "proc", "cwd", "title" }
local ACCENT_SLOTS = { 5, 3, 4, 6, 7, 2 }
local IMPLICIT = { { id = "home", name = "Home" } }

local scope = store.scope "spaces"
local route_key = scope.tab()
local last_active = scope.window()
local last_tab = scope.window()

function M.enabled(cfg)
  return #(cfg.spaces or {}) > 0 or type((cfg.hooks or {}).route) == "function"
end

---The declared entries in order; a hook-only config gets one implicit default space.
function M.statics(cfg)
  if #(cfg.spaces or {}) > 0 then
    return cfg.spaces
  end
  return M.enabled(cfg) and IMPLICIT or {}
end

function M.default_id(cfg)
  local first = M.statics(cfg)[1]
  return first and first.id or nil
end

local function is_template(s)
  return type(s) == "string" and s:find("$", 1, true) ~= nil
end

M.is_template = is_template

local glob_cache = {}

---`*` matches any run; everything else is literal, and the pattern is anchored at both ends.
function M.glob(pattern)
  local cached = glob_cache[pattern]
  if cached then
    return cached
  end
  local escaped = pattern:gsub("[%^%$%(%)%%%.%[%]%+%-%?]", "%%%0")
  local out = "^" .. escaped:gsub("%*", ".*") .. "$"
  glob_cache[pattern] = out
  return out
end

---A `cwd` pattern without `*` is a path prefix, so `~/work` matches `~/work/x` and `~/work` itself.
local function match_one(field, pattern, value)
  if type(value) ~= "string" then
    return false
  end
  if field == "cwd" and not pattern:find("*", 1, true) then
    return value == pattern or value:sub(1, #pattern + 1) == pattern .. "/"
  end
  return value:find(M.glob(pattern)) ~= nil
end

local function match_field(field, want, value)
  if type(want) == "string" then
    return match_one(field, want, value)
  end
  for _, pattern in ipairs(type(want) == "table" and want or {}) do
    if type(pattern) == "string" and match_one(field, pattern, value) then
      return true
    end
  end
  return false
end

---Every field the rule names must match; a list of patterns is any-of. An entry without `match`
---is not a rule at all: it is reached by hand or as the default.
function M.matches(rule, facts)
  if type(rule) ~= "table" then
    return false
  end
  if rule.remote ~= nil and (facts.remote == true) ~= (rule.remote == true) then
    return false
  end
  for _, field in ipairs(FIELDS) do
    if rule[field] ~= nil and not match_field(field, rule[field], facts[field]) then
      return false
    end
  end
  return true
end

---Replaces `$domain $host $user $proc $cwd`; nil when a named fact is missing, so a template is
---skipped rather than minting a space called "".
function M.expand(text, facts)
  if not is_template(text) then
    return text
  end
  local missing = false
  local out = text:gsub("%$(%a+)", function(name)
    local value = facts[name]
    if type(value) ~= "string" or value == "" then
      missing = true
      return ""
    end
    return value
  end)
  if missing then
    return nil
  end
  out = util.sanitize(out:sub(1, ID_MAX))
  return out ~= "" and out or nil
end

---First entry whose rule passes, in list order; templates expand against the facts.
function M.route(cfg, facts)
  for _, entry in ipairs(M.statics(cfg)) do
    if M.matches(entry.match, facts) then
      local id = M.expand(entry.id, facts)
      if id then
        return id, entry
      end
    end
  end
  return nil, nil
end

---What a rule or the hook may look at, from a built item; `remote` is by the same test the
---backend uses to decide where it can run.
function M.facts_of(item, wid)
  local raw = item.raw or {}
  return {
    tab_id = item.tab_id,
    window_id = wid,
    title = item.title,
    proc = raw.proc,
    cwd = raw.cwd,
    host = raw.host,
    user = raw.user,
    domain = raw.domain,
    remote = not backend.is_local(raw.domain, raw.host),
    space = state.space_of(item.tab_id),
    manual = state.space_manual(item.tab_id),
  }
end

local function uses_title(cfg)
  for _, entry in ipairs(M.statics(cfg)) do
    if type(entry.match) == "table" and entry.match.title ~= nil then
      return true
    end
  end
  return false
end

local function fingerprint(facts, with_title)
  return table.concat({
    facts.proc or "",
    facts.cwd or "",
    facts.host or "",
    facts.user or "",
    facts.domain or "",
    with_title and facts.title or "",
  }, "\0")
end

local function hook_route(cfg, facts)
  local hook = (cfg.hooks or {}).route
  if type(hook) ~= "function" then
    return nil
  end
  local ok, id = pcall(hook, facts)
  if not ok then
    util.warn_once("hook-route", "route hook failed: %s", tostring(id))
    return nil
  end
  if type(id) ~= "string" or id == "" then
    return nil
  end
  id = util.sanitize(id:sub(1, ID_MAX))
  return id ~= "" and id or nil
end

local function is_static(cfg, id)
  for _, entry in ipairs(M.statics(cfg)) do
    if entry.id == id then
      return true
    end
  end
  return false
end

local function count_of(set)
  local n = 0
  for _ in pairs(set) do
    n = n + 1
  end
  return n
end

---Admits `id` to the window, recording it when no entry declares it. Refused past the wire's cap:
---the model would be dropped whole, which is worse than one more tab in the space it already has.
local function admit(cfg, id, entry, facts, present)
  if not present[id] and count_of(present) >= M.MAX then
    util.warn_once("spaces-max", "spaces: %d is the most a window can show; %q was not created", M.MAX, id)
    return false
  end
  if is_static(cfg, id) or state.dynamic_space(id) then
    return true
  end
  local name = entry and M.expand(entry.name or entry.id, facts) or id
  local seq = count_of(state.dynamic_spaces()) + 1
  state.set_dynamic_space(id, { seq = seq, name = name or id, template = entry and entry.id or nil })
  return true
end

---The sticky machine. A hand-placed tab is left alone; an unchanged tab is not re-asked; otherwise
---the hook, then the rules, and on first sight the window's own space. A tab only ever moves
---*into* a space that wants it: no match means no move, which is what makes a rule sticky.
---`present` is the set of spaces this window already shows, which the cap is measured against.
---@return string|nil space_id, boolean changed
function M.assign(cfg, wid, item, present)
  local tab_id = item.tab_id
  local current = state.space_of(tab_id)
  if current and state.space_manual(tab_id) then
    return current, false
  end
  local facts = M.facts_of(item, wid)
  local key = fingerprint(facts, uses_title(cfg))
  if current and route_key[tab_id] == key then
    return current, false
  end
  route_key[tab_id] = key
  local target, entry = hook_route(cfg, facts), nil
  if not target then
    target, entry = M.route(cfg, facts)
  end
  if target and not admit(cfg, target, entry, facts, present or {}) then
    target = nil
  end
  if not target and not current then
    target = state.active_space(wid) or M.default_id(cfg)
  end
  if target and target ~= current then
    state.set_space(tab_id, target, false)
    return target, true
  end
  return current, false
end

---The window's active space, falling back to the default when the one it remembers is gone.
function M.active(cfg, wid, present)
  local id = state.active_space(wid)
  if id and present[id] then
    return id
  end
  id = M.default_id(cfg)
  state.set_active_space(wid, id)
  return id
end

---Switching resets what was scoped to the old list: its scroll, and a drag that has lost its slots.
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

---The follow rule: the sidebar follows the active tab into its space when the pair (tab, space)
---differs from the last poll's. A trigger, not an invariant, so merely viewing an empty space holds.
function M.reconcile(wid, active_tab_id, tab_space)
  if tab_space then
    last_tab[wid] = last_tab[wid] or {}
    last_tab[wid][tab_space] = active_tab_id
  end
  local last = last_active[wid]
  if last and last.tab == active_tab_id and last.space == tab_space then
    return false
  end
  last_active[wid] = { tab = active_tab_id, space = tab_space }
  if tab_space and tab_space ~= state.active_space(wid) then
    return M.set_active(wid, tab_space)
  end
  return false
end

---The tab the window last had active inside `id`, or nil.
function M.last_tab_in(wid, id)
  return last_tab[wid] and last_tab[wid][id] or nil
end

---The declared entry for an id, or the template a dynamic id came from.
function M.entry_for(cfg, id)
  for _, entry in ipairs(M.statics(cfg)) do
    if entry.id == id then
      return entry
    end
  end
  local meta = state.dynamic_space(id)
  local template = meta and meta.template
  if template then
    for _, entry in ipairs(M.statics(cfg)) do
      if entry.id == template then
        return entry
      end
    end
  end
  return nil
end

---Every space the window can show, in switcher order: declared ones first, then the dynamic ones
---that hold a tab, by first sight. `all_items` is the whole window, not the visible list.
function M.summary(cfg, wid, all_items)
  local active = state.active_space(wid)
  local count, unseen = {}, {}
  for _, item in ipairs(all_items) do
    local id = item.space
    if id and not item.is_settings then
      count[id] = (count[id] or 0) + 1
      unseen[id] = unseen[id] or item.has_unseen == true
    end
  end
  local out, seen = {}, {}
  local function push(id, name, icon)
    seen[id] = true
    out[#out + 1] = {
      id = id,
      name = name,
      icon = icon,
      active = id == active,
      count = count[id] or 0,
      unseen = unseen[id] == true,
    }
  end
  for _, entry in ipairs(M.statics(cfg)) do
    if not is_template(entry.id) then
      push(entry.id, entry.name or entry.id, entry.icon)
    end
  end
  local dynamic = {}
  for id in pairs(count) do
    if not seen[id] then
      -- a space a tab carries that no entry or record knows: survives as itself, named by its id
      if not state.dynamic_space(id) then
        state.set_dynamic_space(id, { seq = math.huge, name = id })
      end
      dynamic[#dynamic + 1] = id
    end
  end
  table.sort(dynamic, function(a, b)
    local sa, sb = state.dynamic_space(a).seq, state.dynamic_space(b).seq
    if sa ~= sb then
      return sa < sb
    end
    return a < b
  end)
  for _, id in ipairs(dynamic) do
    local meta = state.dynamic_space(id)
    local entry = M.entry_for(cfg, id)
    push(id, meta.name or id, entry and entry.icon or nil)
  end
  return out
end

local function hexc(colour)
  if type(colour) == "table" then
    return string.format("#%02x%02x%02x", colour[1], colour[2], colour[3])
  end
  return colour
end

function M.accent_of(cfg, id, palette)
  local entry = M.entry_for(cfg, id)
  local theme = entry and entry.theme
  if type(theme) == "table" then
    return theme.accent and hexc(theme.accent) or nil
  end
  if entry and theme ~= "auto" and not is_template(entry.id) then
    return nil
  end
  palette = palette or {}
  local slots = {}
  for _, source in ipairs { palette.ansi or {}, palette.brights or {} } do
    for _, i in ipairs(ACCENT_SLOTS) do
      if type(source[i]) == "string" then
        slots[#slots + 1] = source[i]
      end
    end
  end
  if #slots == 0 then
    return nil
  end
  return slots[(tonumber(util.fnv1a(id), 16) % #slots) + 1]
end

---The theme the window paints with: the user's, with the active space's own laid over it.
function M.theme_for(cfg, wid, palette)
  if not M.enabled(cfg) then
    return cfg.theme
  end
  local id = state.active_space(wid) or M.default_id(cfg)
  local entry = M.entry_for(cfg, id)
  if entry and type(entry.theme) == "table" then
    return util.merge(cfg.theme, entry.theme)
  end
  local accent = M.accent_of(cfg, id, palette)
  if accent then
    return util.merge(cfg.theme, { accent = accent })
  end
  return cfg.theme
end

function M.move(tab_id, id, manual)
  state.set_space(tab_id, id or state.space_of(tab_id), id ~= nil and manual == true)
  route_key[tab_id] = nil
end

local MATCH_KEYS = { domain = true, host = true, user = true, proc = true, cwd = true, title = true, remote = true }

---Copies the entries the config can use and warns once per kind of mistake; the user's own tables
---are never edited in place.
function M.validate(cfg)
  local out, seen = {}, {}
  for _, entry in ipairs(type(cfg.spaces) == "table" and cfg.spaces or {}) do
    if type(entry) ~= "table" or type(entry.id) ~= "string" or entry.id == "" then
      util.warn_once("spaces-id", "spaces: an entry without an id was dropped")
    elseif seen[entry.id] then
      util.warn_once("spaces-dup", "spaces: duplicate id %q dropped", entry.id)
    elseif #out >= M.MAX then
      util.warn_once("spaces-max", "spaces: more than %d entries; the rest were dropped", M.MAX)
    else
      seen[entry.id] = true
      local clean = { id = entry.id, name = entry.name, icon = entry.icon, theme = entry.theme, match = entry.match }
      if clean.theme ~= nil and clean.theme ~= "auto" and type(clean.theme) ~= "table" then
        util.warn_once("spaces-theme", 'spaces: %s.theme must be a table or "auto"', entry.id)
        clean.theme = nil
      end
      if clean.match ~= nil then
        if type(clean.match) ~= "table" then
          util.warn_once("spaces-match", "spaces: %s.match must be a table", entry.id)
          clean.match = nil
        else
          for key in pairs(clean.match) do
            if not MATCH_KEYS[key] then
              util.warn_once("spaces-match-key", "spaces: %s.match.%s is not a rule field", entry.id, tostring(key))
            end
          end
        end
      end
      out[#out + 1] = clean
    end
  end
  cfg.spaces = out
  return cfg
end

return M

local wezterm = require "wezterm" ---@type Wezterm
local state = require "vtabs.state"
local store = require "vtabs.store"
local mux = require "vtabs.mux"
local link = require "vtabs.link"
local util = require "vtabs.util"

---Which pane is what: the roles this backend claims by title, the token that proves one, the rank
---that settles the sidebar slot, and the per-poll caches that answer all of it with one mux read.
local M = {}

---Title the backend sets on itself; adoption evidence only, any process can set a title.
local MARKER = "^wez%-vtabs:%x+$"

---Same backend, other role: a settings pane is content, never a tab list.
local SETTINGS_MARKER = "^wez%-vtabs%-settings:%x+$"

local function tab_id_of(pane)
  return mux.tab_id(mux.tab_of(pane))
end

local function user_vars(pane)
  return mux.user_vars(pane) or {}
end

---GUI-managed panes (connection UI, debug overlay) cannot host splits.
function M.is_overlay(pane)
  local domain = mux.domain(pane)
  return type(domain) ~= "string" or domain:find "TermWiz" ~= nil
end

local tick = 0

---One title read per pane per poll: every role check starts from the title, and half a dozen
---callers ask per poll, while a title only ever changes between polls.
local function pane_title(pane)
  local pid = pane:pane_id()
  local seen = store.title[pid]
  if seen and seen.tick == tick then
    return seen.value
  end
  local value = mux.title(pane)
  store.title[pid] = { tick = tick, value = value }
  return value
end

M.title = pane_title

---Role a backend title claims, from one read: "sidebar", "settings", or nil.
local function title_role(title)
  if type(title) ~= "string" then
    return nil
  end
  if title:match(MARKER) then
    return "sidebar"
  end
  if title:match(SETTINGS_MARKER) then
    return "settings"
  end
  return nil
end

---Any title this backend sets, in any role: kept out of the tab list whatever the pane is doing.
function M.marker(title)
  return title_role(title) ~= nil
end

---Adoption evidence, sidebar role only.
function M.has_marker(pane)
  return title_role(pane_title(pane)) == "sidebar"
end

function M.is_settings(pane)
  return pane ~= nil and title_role(pane_title(pane)) == "settings"
end

---One title read, classified once: this runs for every pane on the classify hot path.
local function claims_sidebar(pane)
  return title_role(pane_title(pane)) == "sidebar" and not M.is_overlay(pane)
end

---Re-points the map when the mux renumbers panes; only this process knows the token it minted.
local function claim_echoed_token(pane, pid, panes)
  -- the settings page carries a token of its own; claiming it would hand the tab's sidebar slot away
  if M.is_settings(pane) then
    return false
  end
  local token = user_vars(pane).vtabs_token
  local owner = state.pane_for_token(token)
  if owner == nil or owner == pid then
    return false
  end
  local tab = mux.tab_of(pane)
  for _, p in ipairs(panes or mux.panes(tab) or {}) do
    if p:pane_id() == owner then
      return false
    end
  end
  state.set_sidebar(tab:tab_id(), pid, token)
  return true
end

---True once the backend in `pane` echoed a token this process minted for it: the only trusted state.
---Role-blind: the settings page authenticates over the same bridge, for its own role.
function M.is_ready(pane, panes)
  if not pane or tab_id_of(pane) == nil then
    return false
  end
  local pid = pane:pane_id()
  if store.ready[pid] then
    return true
  end
  local token = state.token_for(pid)
  if (token and user_vars(pane).vtabs_token == token) or claim_echoed_token(pane, pid, panes) then
    store.ready[pid] = true
    store.seen[pid] = util.now_ms()
    return true
  end
  return false
end

---Normalises ready capabilities into a set. Arrays are the wire form; accepting a boolean map too
---keeps tests and future adapters from having to manufacture an array merely to ask a question.
function M.set_capabilities(pane, capabilities)
  local out = {}
  for key, value in pairs(type(capabilities) == "table" and capabilities or {}) do
    if type(key) == "number" and type(value) == "string" then
      out[value] = true
    elseif type(key) == "string" and value == true then
      out[key] = true
    end
  end
  store.capabilities[pane:pane_id()] = out
  return out
end

function M.supports(pane, capability)
  local available = pane and store.capabilities[pane:pane_id()] or nil
  return available ~= nil and available[capability] == true
end

local RANK = { none = 0, marker = 1, mapped = 2, ready = 3 }

---Declared through `store`, so a forgotten tab or window takes them with it.
local scope = store.scope "sidebar"
local classified = scope.tab()

---Title and domain reads cross into the mux; one answer per pane per poll is enough.
local function has_marker_cached(pane, pid)
  local seen = store.marker[pid]
  if seen and seen.tick == tick then
    return seen.value
  end
  local value = claims_sidebar(pane)
  store.marker[pid] = { tick = tick, value = value }
  return value
end

---`pure` reads the caches without promoting a pane.
local function claims(pane, pid, pure)
  if pure then
    return claims_sidebar(pane)
  end
  return has_marker_cached(pane, pid)
end

---How strong a pane's claim to the sidebar role is. The role gates the claim ahead of readiness:
---the settings page authenticates on the same bridge, and a ready one is still its tab's content.
local function sidebar_rank(pane, pure, panes)
  local pid = pane:pane_id()
  -- asked ahead of the gate: this is the read that promotes a pane the mux renumbered
  local ready = store.ready[pid] or (not pure and M.is_ready(pane, panes))
  local tab_id = tab_id_of(pane)
  if tab_id == nil then
    return RANK.none
  end
  -- On its way out: it keeps the slot so no second sidebar splits in beside it, and it is never
  -- content, whatever title it carries.
  if store.quitting[pid] then
    return RANK.marker
  end
  local mapped = state.sidebar_pane_id(tab_id) == pid
  if not mapped and not claims(pane, pid, pure) then
    return RANK.none
  end
  if ready then
    return RANK.ready
  end
  if mapped then
    return RANK.mapped
  end
  if store.given_up[pid] then
    return RANK.none
  end
  return RANK.marker
end

---Any pane presenting as a sidebar backend. Side-effect free: for skipping panes, never for trust.
function M.is_backend(pane)
  return pane ~= nil and sidebar_rank(pane, true) > RANK.none
end

---Splits a tab into { content = Pane[], sidebar = Pane|nil }; only the best claim holds the role.
function M.classify(tab, panes)
  local tab_id = tab:tab_id()
  panes = panes or mux.panes(tab) or {}
  local seen = classified[tab_id]
  if seen and seen.tick == tick and seen.n == #panes then
    return seen.content, seen.sb
  end
  local sb, best = nil, RANK.none
  for _, p in ipairs(panes) do
    local rank = sidebar_rank(p, nil, panes)
    if rank > best then
      sb, best = p, rank
    end
  end
  local sb_id = sb and sb:pane_id()
  local content = {}
  for _, p in ipairs(panes) do
    if p:pane_id() ~= sb_id then
      content[#content + 1] = p
    end
  end
  classified[tab_id] = { tick = tick, n = #panes, content = content, sb = sb }
  return content, sb
end

function M.find(tab)
  local _, sb = M.classify(tab)
  return sb
end

function M.content_pane(tab, panes, active)
  local content, sb = M.classify(tab, panes)
  local sb_id = sb and sb:pane_id()
  active = active or mux.active_pane(tab)
  if active and active:pane_id() ~= sb_id then
    return active
  end
  local remembered = store.content_pane[tab:tab_id()]
  for _, p in ipairs(content) do
    if p:pane_id() == remembered then
      return p
    end
  end
  return content[1]
end

---Opens the next round of cached answers: one poll, one read per pane.
function M.next_poll()
  tick = tick + 1
  -- cleared in place: rebinding would hand `store` a table nothing writes to any more
  for id in pairs(classified) do
    classified[id] = nil
  end
end

---Drops one tab's cached split, for a caller that just changed which panes it holds.
function M.forget_split(tab_id)
  classified[tab_id] = nil
end

local function cwd_path(pane)
  local cwd = mux.cwd(pane)
  if not cwd then
    return nil
  end
  if type(cwd) == "string" then
    return (cwd:gsub("^file://[^/]*", ""))
  end
  return cwd.file_path
end

---Host named in the pane's OSC 7 cwd; panes proxied through a mux server only reveal their host this way.
function M.cwd_host(pane)
  local cwd = mux.cwd(pane)
  if not cwd then
    return nil
  end
  local url = type(cwd) == "string" and cwd or tostring(cwd)
  local host = url:match "^file://([^/]*)/"
  return host ~= "" and host or nil
end

local function clean(s)
  return type(s) == "string" and util.sanitize(s) or nil
end

function M.tab_meta(tab, pane)
  local title = clean(tab:get_title()) or ""
  if M.marker(title) then
    title = ""
  end
  local tab_id = tab:tab_id()
  return {
    cwd = clean(cwd_path(pane)),
    domain = clean(mux.domain(pane)),
    title = title ~= "" and title or nil,
    pinned = state.is_pinned(tab_id),
    space = state.space_of(tab_id),
    space_manual = state.space_manual(tab_id) or nil,
  }
end

function M.send(pane, message, frame_token)
  return M.send_raw(pane, wezterm.json_encode(message), frame_token)
end

---For pre-encoded lines: wire encodes once per window, then this boundary frames every record with
---the pane's current authentication token. A literal JSON line typed at the pane is never control.
function M.send_raw(pane, line, frame_token)
  local token = frame_token or state.token_for(pane:pane_id())
  local protocol = require "vtabs.gen.protocol"
  if type(token) ~= "string" or token == "" or #token > protocol.CONTROL_TOKEN_MAX_BYTES or token:find "[%s%c]" then
    return false
  end
  local framed = {}
  for record in line:gmatch "[^\n]+" do
    framed[#framed + 1] = protocol.CONTROL_PREFIX .. token .. " " .. record
  end
  if #framed == 0 then
    return false
  end
  local text = table.concat(framed, "\n") .. "\n"
  if link.defer(pane, text) then
    return true
  end
  return pcall(function()
    pane:send_text(text)
  end)
end

function M.auth(pane)
  local token = state.token_for(pane:pane_id())
  if token then
    store.authed_at[pane:pane_id()] = util.now_ms()
    M.send(pane, {
      t = "auth",
      token = token,
      caps = { "typed_intents", "theme_hooks", "settings_document", "spaces_policy" },
    }, user_vars(pane).vtabs_token or token)
  end
end

return M

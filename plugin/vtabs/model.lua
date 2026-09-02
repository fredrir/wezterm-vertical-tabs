local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local mux = require "vtabs.mux"
local spaces = require "vtabs.spaces"
local store = require "vtabs.store"
local util = require "vtabs.util"

local M = {}

---The three title sources, separately: v2 sends them raw so Rust can own the fallback.
local function title_parts(tab, pane, cfg, observed, pane_facts)
  local override = nil
  if cfg.title then
    local ok, custom = pcall(cfg.title, tab, pane)
    if not ok then
      util.warn_once("hook-title", "title hook failed: %s", tostring(custom))
    elseif custom and custom ~= "" then
      override = custom
    end
  end
  local title = observed and observed.title or nil
  if not observed then
    title = mux.tab_title(tab)
  end
  if title == "" or sidebar.marker(title) then
    title = nil
  end
  local pane_title = pane_facts and pane_facts.title or nil
  if not pane_facts then
    pane_title = sidebar.title(pane)
  end
  if not pane_title or pane_title == "" or sidebar.marker(pane_title) then
    pane_title = nil
  end
  return override, title, pane_title
end

local ICON_PATTERN_MAGIC = { "^", "$", "*", "+", "?", "[" }

local FACTS_TTL_MS = 60000
---Declared through `store`, so forgetting a tab clears it without a hook to register.
local scope = store.scope "model"
local facts_cache = scope.tab()

---`file_path` prefixes a Windows drive with a slash (url-funcs/src/lib.rs:60-76); drop it.
local function local_path(path)
  if type(path) ~= "string" then
    return nil
  end
  return path:match "^/(%a:[/\\].*)$" or path
end

local function some(s)
  return type(s) == "string" and s ~= "" and s or nil
end

---Path, host and user of a pane's OSC 7 cwd; the user is the URL's, never the local `$USER`.
local function cwd_of(pane, observed)
  local cwd = observed and observed.cwd or nil
  if not observed then
    cwd = mux.cwd(pane)
  end
  if not cwd then
    return nil, nil, nil
  end
  if type(cwd) == "string" then
    local authority, path = cwd:match "^file://([^/]*)(/.*)$"
    if not authority then
      return local_path(cwd), nil, nil
    end
    local user, host = authority:match "^([^@]*)@(.*)$"
    return local_path(path), some(host or authority), some(user)
  end
  return local_path(cwd.file_path), some(cwd.host), some(cwd.username)
end

---`~` for the user's home, so the meta line spends its 20 columns on what differs.
local function tilde(path)
  local home = some(wezterm.home_dir) or os.getenv "HOME" or os.getenv "USERPROFILE"
  if not path or not home then
    return path
  end
  if path == home then
    return "~"
  end
  local head = path:sub(1, #home + 1)
  if head == home .. "/" or head == home .. "\\" then
    return "~" .. path:sub(#home + 1)
  end
  return path
end

---One probe of the pane's mux-visible facts; Rust composes the displayed title and meta line.
local function facts_for(pane, observed)
  local path, host, remote_user = cwd_of(pane, observed)
  return {
    cwd = tilde(path),
    host = host,
    user = remote_user,
    proc = util.basename(observed and observed.foreground or (not observed and mux.foreground(pane) or nil)),
    domain = observed and observed.domain or (not observed and mux.domain(pane) or nil),
  }
end

local function is_icon_pattern(key)
  for _, magic in ipairs(ICON_PATTERN_MAGIC) do
    if key:find(magic, 1, true) then
      return true
    end
  end
  return false
end

---User mappings are Lua values, so Lua is the only honest place to interpret their pattern keys.
---Exact names win; patterned keys keep the old deterministic sorted-key precedence.
local function custom_icon(process, icon_map)
  if not process or type(icon_map) ~= "table" then
    return nil
  end
  if type(icon_map[process]) == "string" then
    return icon_map[process]
  end
  local patterns = {}
  for key, icon in pairs(icon_map) do
    if type(key) == "string" and type(icon) == "string" and is_icon_pattern(key) then
      patterns[#patterns + 1] = key
    end
  end
  table.sort(patterns)
  for _, pattern in ipairs(patterns) do
    local ok, matched = pcall(string.match, process, pattern)
    if ok and matched then
      return icon_map[pattern]
    end
  end
  return type(icon_map.default) == "string" and icon_map.default or nil
end

local function cached_facts(tab_id, pane, cfg, now, observed)
  local seen = facts_cache[tab_id]
  if seen and seen.icon_map == cfg.icon_map and now - seen.at < cfg.poll_ms then
    return seen.facts
  end
  local facts = facts_for(pane, observed)
  facts.icon = custom_icon(facts.proc, cfg.icon_map)
  facts_cache[tab_id] = { facts = facts, at = now, icon_map = cfg.icon_map }
  return facts
end

M.forget_tab = scope.forget_tab

local pruned_at = 0

---Sweeping every build is pointless work on the hot path; entries only expire once per TTL.
local function prune_facts(now)
  if now - pruned_at < FACTS_TTL_MS then
    return
  end
  pruned_at = now
  for tab_id, entry in pairs(facts_cache) do
    if now - entry.at > FACTS_TTL_MS then
      facts_cache[tab_id] = nil
    end
  end
end

local function included(cfg, tab, mux_win)
  if not cfg.hooks.filter then
    return true
  end
  local ok, keep = pcall(cfg.hooks.filter, tab, mux_win)
  if not ok then
    util.warn_once("hook-filter", "filter hook failed: %s", tostring(keep))
    return true
  end
  return keep ~= false
end

---Walks every tab of the window once: what each is, which space holds it, which space the window
---shows and what the switcher lists. `build` is the visible half of the same walk.
---@return { all: table, visible: table, space: string|nil, spaces: table|nil }
function M.survey(gui_window, snapshot)
  local cfg = snapshot and snapshot.cfg or config.get()
  local mux_win = snapshot and snapshot.mux_window or gui_window:mux_window()
  local wid = snapshot and snapshot.window_id or gui_window:window_id()
  local now = snapshot and snapshot.now or util.now_ms()
  prune_facts(now)
  local items = {}
  local observed_tabs = snapshot and snapshot.tabs or mux.tabs_with_info(mux_win) or {}
  for _, observed in ipairs(observed_tabs) do
    -- a tab that dies mid-poll drops out of the list instead of failing the whole window
    local ok, item = pcall(function()
      local info = observed.info or observed
      local tab = info.tab
      local pane = included(cfg, tab, mux_win)
          and (observed.content_pane or sidebar.content_pane(tab, observed.panes, observed.active_pane))
        or nil
      if not pane then
        return nil
      end
      local tab_id = observed.tab_id or tab:tab_id()
      if sidebar.is_settings(pane) then
        return {
          tab_id = tab_id,
          index = info.index + 1,
          is_active = info.is_active,
          is_pinned = state.is_pinned(tab_id),
          title = "Settings",
          has_unseen = false,
          is_settings = true,
        }
      end
      local pane_facts = snapshot and snapshot.panes[pane:pane_id()] or nil
      local override, tab_title, pane_title = title_parts(tab, pane, cfg, observed.info and observed or nil, pane_facts)
      local facts = cached_facts(tab_id, pane, cfg, now, pane_facts)
      return {
        tab_id = tab_id,
        index = info.index + 1,
        is_active = info.is_active,
        is_pinned = state.is_pinned(tab_id),
        has_unseen = pane_facts and pane_facts.unseen == true or (not pane_facts and mux.unseen(pane) == true),
        raw = {
          override = override and util.sanitize(override) or nil,
          title = tab_title and util.sanitize(tab_title) or nil,
          pane_title = pane_title and util.sanitize(pane_title) or nil,
          proc = facts.proc,
          icon = facts.icon,
          cwd = facts.cwd,
          host = facts.host,
          user = facts.user,
          domain = facts.domain,
        },
      }
    end)
    if ok and item then
      items[#items + 1] = item
    elseif not ok then
      util.warn_once("model-tab", "tab skipped: %s", tostring(item):match "^[^\n]*")
    end
  end
  if not spaces.enabled(cfg) then
    return { all = items, visible = items }
  end
  local active = snapshot and snapshot.active or nil
  local active_sidebar = active and active.sidebar or sidebar.find(util.active_tab(gui_window))
  local capable = active_sidebar and sidebar.supports(active_sidebar, "spaces_policy") or false
  return spaces.project(cfg, wid, items, capable)
end

---The sidebar's list for a window: the active space's tabs, in physical order.
function M.build(gui_window, snapshot)
  return M.survey(gui_window, snapshot).visible
end

---Rendered order: pinned first, then the rest, both in physical order.
function M.ordered(items)
  local pinned, rest = util.partition(items, function(item)
    return item.is_pinned
  end)
  for _, item in ipairs(rest) do
    pinned[#pinned + 1] = item
  end
  return pinned
end

function M.find(items, tab_id)
  for i, item in ipairs(items) do
    if item.tab_id == tab_id then
      return item, i
    end
  end
  return nil
end

function M.ids(items)
  local ids = {}
  for i, item in ipairs(items) do
    ids[i] = item.tab_id
  end
  return ids
end

return M

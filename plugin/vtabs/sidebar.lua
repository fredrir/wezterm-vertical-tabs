local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local backend = require "vtabs.backend"
local theme = require "vtabs.theme"
local util = require "vtabs.util"

local M = {}

local ATTACH_RETRY_MS = 5000
local PING_AFTER_MS = 8000
local DEAD_AFTER_MS = 20000
local READY_TIMEOUT_MS = 12000
local ADOPT_RETRY_MS = 2000
local ADOPT_TRIES = 5
local ADOPT_WINDOW_MS = 30000
local FAILED_DOMAIN_MS = 60000
local PRUNE_MS = 30000
local PIN_GRACE_MS = 3000

---Title the backend sets on itself; adoption evidence only, any process can set a title.
local MARKER = "^wez%-vtabs:%x+$"

local session = state.session

local function tab_id_of(pane)
  local tab = util.try(function()
    return pane:tab()
  end)
  return tab and tab:tab_id() or nil
end

local function user_vars(pane)
  return util.try(function()
    return pane:get_user_vars()
  end) or {}
end

---GUI-managed panes (connection UI, debug overlay) cannot host splits.
function M.is_overlay(pane)
  local domain = util.try(function()
    return pane:get_domain_name()
  end)
  return type(domain) ~= "string" or domain:find "TermWiz" ~= nil
end

function M.marker(title)
  return type(title) == "string" and title:match(MARKER) ~= nil
end

function M.has_marker(pane)
  return M.marker(util.try(function()
    return pane:get_title()
  end))
end

---Re-points the map when the mux renumbers panes; only this process knows the token it minted.
local function claim_echoed_token(pane, pid)
  local token = user_vars(pane).vtabs_token
  local owner = state.pane_for_token(token)
  if owner == nil or owner == pid then
    return false
  end
  local tab = pane:tab()
  for _, p in ipairs(tab:panes()) do
    if p:pane_id() == owner then
      return false
    end
  end
  state.set_sidebar(tab:tab_id(), pid, token)
  return true
end

---True once the backend in `pane` echoed a token this process minted for it: the only trusted state.
function M.is_ready(pane)
  if not pane or tab_id_of(pane) == nil then
    return false
  end
  local pid = pane:pane_id()
  if session.ready[pid] then
    return true
  end
  local token = state.token_for(pid)
  if (token and user_vars(pane).vtabs_token == token) or claim_echoed_token(pane, pid) then
    session.ready[pid] = true
    session.seen[pid] = util.now_ms()
    return true
  end
  return false
end

local RANK = { none = 0, marker = 1, mapped = 2, ready = 3 }

local tick = 0
local classified = {}

---Title and domain reads cross into the mux; one answer per pane per poll is enough.
local function has_marker_cached(pane, pid)
  local seen = session.marker[pid]
  if seen and seen.tick == tick then
    return seen.value
  end
  local value = M.has_marker(pane) and not M.is_overlay(pane)
  session.marker[pid] = { tick = tick, value = value }
  return value
end

---How strong a pane's claim to the sidebar role is; a marker alone is the weakest and authorises nothing.
local function sidebar_rank(pane)
  local pid = pane:pane_id()
  if session.ready[pid] then
    return RANK.ready
  end
  local tab_id = tab_id_of(pane)
  if tab_id == nil then
    return RANK.none
  end
  if M.is_ready(pane) then
    return RANK.ready
  end
  if state.sidebar_pane_id(tab_id) == pid then
    return RANK.mapped
  end
  if session.given_up[pid] or not has_marker_cached(pane, pid) then
    return RANK.none
  end
  return RANK.marker
end

function M.is_backend(pane)
  return pane ~= nil and sidebar_rank(pane) > RANK.none
end

---Splits a tab into { content = Pane[], sidebar = Pane|nil }; only the best claim holds the role.
function M.classify(tab)
  local tab_id = tab:tab_id()
  local panes = tab:panes()
  local seen = classified[tab_id]
  if seen and seen.tick == tick and seen.n == #panes then
    return seen.content, seen.sb
  end
  local sb, best = nil, RANK.none
  for _, p in ipairs(panes) do
    local rank = sidebar_rank(p)
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

function M.content_pane(tab)
  local content, sb = M.classify(tab)
  local sb_id = sb and sb:pane_id()
  local active = tab:active_pane()
  if active and active:pane_id() ~= sb_id then
    return active
  end
  local remembered = session.content_pane[tab:tab_id()]
  for _, p in ipairs(content) do
    if p:pane_id() == remembered then
      return p
    end
  end
  return content[1]
end

local function cwd_path(pane)
  local cwd = util.try(function()
    return pane:get_current_working_dir()
  end)
  if not cwd then
    return nil
  end
  if type(cwd) == "string" then
    return (cwd:gsub("^file://[^/]*", ""))
  end
  return cwd.file_path
end

---Host named in the pane's OSC 7 cwd; panes proxied through a mux server only reveal their host this way.
local function cwd_host(pane)
  local cwd = util.try(function()
    return pane:get_current_working_dir()
  end)
  if not cwd then
    return nil
  end
  local url = type(cwd) == "string" and cwd or tostring(cwd)
  local host = url:match "^file://([^/]*)/"
  return host ~= "" and host or nil
end

function M.tab_meta(tab, pane)
  local title = tab:get_title()
  if M.marker(title) then
    title = ""
  end
  return {
    cwd = cwd_path(pane),
    domain = util.try(function()
      return pane:get_domain_name()
    end),
    title = title ~= "" and title or nil,
    pinned = state.is_pinned(tab:tab_id()),
  }
end

function M.send(pane, message)
  return pcall(function()
    pane:send_text(wezterm.json_encode(message) .. "\n")
  end)
end

function M.auth(pane)
  local token = state.token_for(pane:pane_id())
  if token then
    session.authed_at[pane:pane_id()] = util.now_ms()
    M.send(pane, { t = "auth", token = token })
  end
end

local fitted = {}

---Mux-domain splits can grow the tab past the window; re-sending the window size makes the mux refit it.
local function fit_to_window(tab)
  local gui = util.try(function()
    return tab:window():gui_window()
  end)
  if not gui then
    return
  end
  local wid = gui:window_id()
  if fitted[wid] then
    return
  end
  fitted[wid] = true
  local ok, err = pcall(function()
    local dims = gui:get_dimensions()
    gui:set_inner_size(dims.pixel_width, dims.pixel_height)
  end)
  if not ok then
    util.log("fit to window failed: %s", tostring(err))
  end
end

local function theme_bg(tab)
  local palette = util.try(function()
    return tab:window():gui_window():effective_config().resolved_palette
  end)
  local resolved = util.try(theme.resolve, config.get().theme, palette or {})
  local rgb = resolved and resolved.bg
  if type(rgb) ~= "table" then
    return nil
  end
  return string.format("#%02x%02x%02x", rgb[1], rgb[2], rgb[3])
end

---Domain the sidebar would be spawned in for this tab.
local function attach_domain(cfg, base)
  if cfg.domain ~= "CurrentPaneDomain" then
    return cfg.domain
  end
  return util.try(function()
    return base:get_domain_name()
  end)
end

---"auto" adopts only where this plugin spawns backends itself; see docs/limitations.md.
local function may_adopt(cfg, domain, host, place)
  if cfg.adopt ~= "auto" then
    return cfg.adopt == true
  end
  return backend.is_local(domain, host)
    or session.spawned_domains[place] == true
    or backend.resolve_path(cfg, domain, host) ~= nil
end

---A candidate must sit in the very domain this plugin would have spawned the sidebar in.
local function adoptable(tab, sb)
  local cfg = config.get()
  local base = M.content_pane(tab) or tab:active_pane()
  if not base then
    return false
  end
  local domain = attach_domain(cfg, base)
  local pane_domain = util.try(function()
    return sb:get_domain_name()
  end)
  if domain == nil or domain ~= pane_domain then
    return false
  end
  local host = cwd_host(base)
  return may_adopt(cfg, domain, host, domain .. "@" .. (host or ""))
end

local function domain_failed(place, now)
  local at = session.failed_domains[place]
  if not at then
    return false
  end
  if now - at > FAILED_DOMAIN_MS then
    session.failed_domains[place] = nil
    return false
  end
  return true
end

---Splits off the sidebar pane; guarded because splits are async on mux domains.
function M.attach(tab)
  local cfg = config.get()
  local tab_id = tab:tab_id()
  local now = util.now_ms()
  local pending = session.attaching[tab_id]
  if pending and now - pending < ATTACH_RETRY_MS then
    return nil
  end
  local base = M.content_pane(tab)
  if not base or M.is_overlay(base) then
    return nil
  end
  local pane_domain = attach_domain(cfg, base)
  if not pane_domain then
    util.warn_once("domain-" .. tab_id, "cannot determine domain for tab %d; sidebar skipped", tab_id)
    return nil
  end
  local host = cwd_host(base)
  local place = pane_domain .. "@" .. (host or "")
  if domain_failed(place, now) then
    return nil
  end
  local args = backend.spawn_args(cfg, pane_domain, host)
  if not session.logged_domains[place] then
    session.logged_domains[place] = true
    util.log(
      "domain %s host %s: sidebar command %s",
      pane_domain,
      host or "?",
      args and args[#args]:sub(1, 120) or "none"
    )
  end
  if not args then
    util.warn_once(
      "backend-" .. tostring(pane_domain),
      "no backend for domain %s; set backend.path",
      tostring(pane_domain)
    )
    return nil
  end
  session.attaching[tab_id] = now
  local domain = cfg.domain == "CurrentPaneDomain" and "CurrentPaneDomain" or { DomainName = cfg.domain }
  local ok, sb = pcall(function()
    return base:split {
      direction = cfg.position == "left" and "Left" or "Right",
      top_level = true,
      size = cfg.width,
      args = args,
      set_environment_variables = backend.env(cfg, pane_domain, host, theme_bg(tab)),
      domain = domain,
    }
  end)
  if not ok or not sb then
    util.warn("sidebar split failed: %s", tostring(sb):match "^[^\n]*")
    return nil
  end
  session.attaching[tab_id] = nil
  local pid = sb:pane_id()
  state.set_sidebar(tab_id, pid, util.random_token())
  session.seen[pid] = now
  session.spawned[pid] = now
  session.pane_domain[pid] = place
  session.spawned_domains[place] = true
  M.auth(sb)
  base:activate()
  if not backend.is_local(pane_domain, host) then
    fit_to_window(tab)
  end
  return sb
end

---Takes over a backend pane the mux kept across a GUI restart: fresh token, then wait for its echo.
local function adopt(tab, sb, now)
  local pid = sb:pane_id()
  state.set_sidebar(tab:tab_id(), pid, util.random_token())
  session.adopted[pid] = now
  session.auth_tries[pid] = 1
  session.seen[pid] = now
  M.auth(sb)
end

---A pane that never echoes its token is never trusted; an adopted one is retried a few times, then left.
local function await_auth(gui_window, tab, sb, now)
  local pid = sb:pane_id()
  if state.sidebar_pane_id(tab:tab_id()) ~= pid then
    -- classify ranks a ready or mapped pane above a marker, so this tab has neither
    if not M.has_marker(sb) or M.is_overlay(sb) then
      return
    end
    if adoptable(tab, sb) then
      adopt(tab, sb, now)
    else
      -- not ours to take over: let it be content so the tab still gets a sidebar
      session.given_up[sb:pane_id()] = true
    end
    return
  end
  if session.adopted[pid] then
    local tries = session.auth_tries[pid] or 0
    if tries >= ADOPT_TRIES or now - session.adopted[pid] > ADOPT_WINDOW_MS then
      -- it kept the marker but never echoed: hand the tab back and treat the pane as content
      util.warn_once("adopt-" .. pid, "pane %d never authenticated; not a sidebar", pid)
      session.given_up[pid] = true
      session.adopted[pid] = nil
      state.set_sidebar(tab:tab_id(), nil)
      classified[tab:tab_id()] = nil
    elseif now - (session.authed_at[pid] or 0) > ADOPT_RETRY_MS then
      session.auth_tries[pid] = tries + 1
      M.auth(sb)
    end
    return
  end
  if not session.given_up[pid] and now - (session.seen[pid] or now) > READY_TIMEOUT_MS then
    M.give_up(gui_window, tab, sb)
  end
end

---CloseCurrentPane/CloseCurrentTab act on the active pane/tab, so targets are activated first.
function M.detach(gui_window, tab)
  local sb = M.find(tab)
  if sb and not M.is_ready(sb) then
    util.warn_once("detach-" .. sb:pane_id(), "sidebar %d never authenticated; left open", sb:pane_id())
    return
  end
  if sb then
    state.forget_pane(sb:pane_id())
    local content = M.content_pane(tab)
    local previous = gui_window:mux_window():active_tab()
    sb:activate()
    gui_window:perform_action(act.CloseCurrentPane { confirm = false }, sb)
    if content then
      content:activate()
    end
    if previous and previous:tab_id() ~= tab:tab_id() then
      previous:activate()
    end
  end
  state.set_sidebar(tab:tab_id(), nil)
end

---Closes a tab that only holds a sidebar without disturbing the active tab.
function M.close_orphan(gui_window, tab, sb)
  state.forget_pane(sb:pane_id())
  local previous = gui_window:mux_window():active_tab()
  local switching = previous and previous:tab_id() ~= tab:tab_id()
  if switching then
    tab:activate()
  end
  gui_window:perform_action(act.CloseCurrentTab { confirm = false }, sb)
  if switching then
    previous:activate()
  end
end

---A backend that never answers usually died at spawn; closing such panes over a mux link can wedge
---WezTerm's focus reconciliation, so the pane is left for the user and its place is not retried.
function M.give_up(_, _, sb)
  local pid = sb:pane_id()
  local now = util.now_ms()
  local place = session.pane_domain[pid] or "local@"
  if domain_failed(place, now) then
    return
  end
  session.failed_domains[place] = now
  session.given_up[pid] = true
  util.warn("sidebar backend did not start in %s; fix backend.path and close that pane", place)
end

---Pings idle sidebars; replaces one whose backend stopped answering.
local function check_liveness(gui_window, tab, sb, now)
  local pid = sb:pane_id()
  local seen = session.seen[pid] or now
  session.seen[pid] = seen
  local idle = now - seen
  if idle > DEAD_AFTER_MS then
    util.warn("sidebar %d unresponsive, restarting", pid)
    M.detach(gui_window, tab)
    return false
  end
  if idle > PING_AFTER_MS and (session.pinged[pid] or 0) < seen then
    session.pinged[pid] = now
    M.send(sb, { t = "ping", n = now })
  end
  return true
end

local function record_closed_tabs(wid, seen, private)
  local previous = session.known_tabs[wid] or {}
  for tab_id in pairs(previous) do
    if not seen[tab_id] then
      local meta = session.tab_meta[tab_id]
      if meta and not session.moving[tab_id] and not private then
        state.push_closed(meta)
      end
      local pid = state.sidebar_pane_id(tab_id)
      if pid then
        state.forget_pane(pid)
      end
      state.forget_tab(tab_id)
    end
  end
  session.known_tabs[wid] = seen
end

local pins_deadline = nil

---Tab ids only mean something while the mux that minted them lives; a surviving backend pane proves it.
local function resolve_pins(tabs, now)
  if not state.pins_pending() then
    return
  end
  for _, info in ipairs(tabs) do
    for _, p in ipairs(info.tab:panes()) do
      if M.has_marker(p) then
        state.restore_pins()
        pins_deadline = nil
        return
      end
    end
  end
  pins_deadline = pins_deadline or now + PIN_GRACE_MS
  if now > pins_deadline then
    state.discard_pins()
    pins_deadline = nil
  end
end

local pruned_at = 0

local function prune_windows(now)
  if now - pruned_at < PRUNE_MS then
    return
  end
  pruned_at = now
  local windows = util.try(function()
    return wezterm.mux.all_windows()
  end)
  if type(windows) ~= "table" or #windows == 0 then
    return
  end
  local live = {}
  for _, w in ipairs(windows) do
    local id = util.try(function()
      return w:window_id()
    end)
    if id then
      live[id] = true
    end
  end
  for id in pairs(fitted) do
    if not live[id] then
      fitted[id] = nil
    end
  end
  state.forget_windows_except(live)
end

---Makes every tab match the collapsed/expanded state and closes tabs left with only a sidebar.
function M.ensure(gui_window)
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  local collapsed = state.is_collapsed(wid)
  local private = state.is_private(wid)
  local now = util.now_ms()
  local seen = {}
  tick = tick + 1
  classified = {}
  local tabs = mux_win:tabs_with_info()

  resolve_pins(tabs, now)
  prune_windows(now)

  for _, info in ipairs(tabs) do
    local tab = info.tab
    local tab_id = tab:tab_id()
    local content, sb = M.classify(tab)
    if #content == 0 then
      if sb then
        -- the tab still exists; forgetting it would strand a sidebar that authenticates later
        seen[tab_id] = true
        if not M.is_ready(sb) then
          await_auth(gui_window, tab, sb, now)
        elseif #tab:panes() == 1 then
          M.close_orphan(gui_window, tab, sb)
        end
      end
    else
      seen[tab_id] = true
      local active = tab:active_pane()
      if active and (not sb or active:pane_id() ~= sb:pane_id()) then
        session.content_pane[tab_id] = active:pane_id()
      end
      session.tab_meta[tab_id] = M.tab_meta(tab, M.content_pane(tab))
      if collapsed then
        if sb and M.is_ready(sb) then
          M.detach(gui_window, tab)
        end
      elseif sb then
        if M.is_ready(sb) then
          check_liveness(gui_window, tab, sb, now)
        else
          await_auth(gui_window, tab, sb, now)
        end
      else
        M.attach(tab)
      end
    end
  end
  record_closed_tabs(wid, seen, private)
end

function M.set_collapsed(gui_window, collapsed)
  local wid = gui_window:window_id()
  state.set_collapsed(wid, collapsed)
  state.set_focus(wid, false)
  M.ensure(gui_window)
end

function M.toggle(gui_window)
  M.set_collapsed(gui_window, not state.is_collapsed(gui_window:window_id()))
end

return M

local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local backend = require "vtabs.backend"
local util = require "vtabs.util"

local M = {}

local ATTACH_RETRY_MS = 5000
local PING_AFTER_MS = 8000
local DEAD_AFTER_MS = 20000
local READY_TIMEOUT_MS = 12000

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

---Only panes the plugin registered count; user vars alone are never trusted.
function M.is_sidebar(pane)
  if not pane then
    return false
  end
  local tab_id = tab_id_of(pane)
  return tab_id ~= nil and state.sidebar_pane_id(tab_id) == pane:pane_id()
end

---True once the backend in `pane` has echoed the token it was given over stdin.
function M.is_ready(pane)
  local pid = pane:pane_id()
  if session.ready[pid] then
    return true
  end
  local token = state.token_for(pid)
  if token and user_vars(pane).vtabs_token == token then
    session.ready[pid] = true
    session.seen[pid] = util.now_ms()
    return true
  end
  return false
end

---Splits a tab's panes into { content = Pane[], sidebar = Pane|nil }.
function M.classify(tab)
  local content, sb = {}, nil
  for _, p in ipairs(tab:panes()) do
    if M.is_sidebar(p) then
      sb = sb or p
    else
      content[#content + 1] = p
    end
  end
  return content, sb
end

function M.find(tab)
  local _, sb = M.classify(tab)
  return sb
end

function M.content_pane(tab)
  local active = tab:active_pane()
  if active and not M.is_sidebar(active) then
    return active
  end
  local content = M.classify(tab)
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

function M.tab_meta(tab, pane)
  local title = tab:get_title()
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
    M.send(pane, { t = "auth", token = token })
  end
end

local function is_local(pane)
  local domain = util.try(function()
    return pane:get_domain_name()
  end)
  return domain == "local"
end

---Mux-domain splits can grow the tab past the window; re-sending the window size makes the mux refit it.
local function fit_to_window(tab)
  pcall(function()
    local gui = tab:window():gui_window()
    if gui then
      local dims = gui:get_dimensions()
      gui:set_inner_size(dims.pixel_width, dims.pixel_height)
    end
  end)
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
  local pane_domain = cfg.domain == "CurrentPaneDomain" and util.try(function()
    return base:get_domain_name()
  end) or cfg.domain
  if session.failed_domains[pane_domain] then
    return nil
  end
  local args = backend.spawn_args(cfg, pane_domain)
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
      set_environment_variables = backend.env(cfg, pane_domain),
      domain = domain,
    }
  end)
  if not ok or not sb then
    util.warn("sidebar split failed: %s", tostring(sb):match "^[^\n]*")
    return nil
  end
  session.attaching[tab_id] = nil
  local token = util.random_token()
  state.set_sidebar(tab_id, sb:pane_id(), token)
  session.seen[sb:pane_id()] = now
  session.pane_domain[sb:pane_id()] = pane_domain
  M.auth(sb)
  base:activate()
  if not is_local(base) then
    fit_to_window(tab)
  end
  return sb
end

---CloseCurrentPane/CloseCurrentTab act on the active pane/tab, so targets are activated first.
function M.detach(gui_window, tab)
  local sb = M.find(tab)
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

---A backend that never answers means the binary is missing there; stop retrying in that domain.
function M.give_up(gui_window, tab, sb)
  local domain = session.pane_domain[sb:pane_id()] or "local"
  session.failed_domains[domain] = true
  util.warn("sidebar backend not responding in domain %s; set backend.path for it", tostring(domain))
  M.detach(gui_window, tab)
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
    M.send(sb, { t = "ping" })
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

---Makes every tab match the collapsed/expanded state and closes tabs left with only a sidebar.
function M.ensure(gui_window)
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  local collapsed = state.is_collapsed(wid)
  local private = state.is_private(wid)
  local now = util.now_ms()
  local seen = {}

  for _, info in ipairs(mux_win:tabs_with_info()) do
    local tab = info.tab
    local tab_id = tab:tab_id()
    local content, sb = M.classify(tab)
    if #content == 0 then
      if sb then
        M.close_orphan(gui_window, tab, sb)
      end
    else
      seen[tab_id] = true
      local active = tab:active_pane()
      if active and not M.is_sidebar(active) then
        session.content_pane[tab_id] = active:pane_id()
      end
      session.tab_meta[tab_id] = M.tab_meta(tab, M.content_pane(tab))
      if collapsed then
        if sb then
          M.detach(gui_window, tab)
        end
      elseif sb then
        if M.is_ready(sb) then
          check_liveness(gui_window, tab, sb, now)
        elseif now - (session.seen[sb:pane_id()] or now) > READY_TIMEOUT_MS then
          M.give_up(gui_window, tab, sb)
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

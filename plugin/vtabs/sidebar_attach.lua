local wezterm = require "wezterm" ---@type Wezterm
local act = wezterm.action
local config = require "vtabs.config"
local state = require "vtabs.state"
local store = require "vtabs.store"
local backend = require "vtabs.backend"
local spaces = require "vtabs.spaces"
local theme = require "vtabs.theme"
local mux = require "vtabs.mux"
local util = require "vtabs.util"
local identity = require "vtabs.sidebar_identity"
local rescue = require "vtabs.sidebar_rescue"

---How a sidebar pane comes and goes: the split that spawns one, the handshake that adopts one the
---mux kept, the ledger of places that failed, and the poll that holds every tab to the same shape.
local M = {}

local ATTACH_RETRY_MS = 5000
local READY_TIMEOUT_MS = 12000
local ADOPT_RETRY_MS = 2000
local ADOPT_TRIES = 5
local ADOPT_WINDOW_MS = 30000
local FAILED_DOMAIN_MS = 60000

---Declared through `store`, so a forgotten window takes them with it.
local scope = store.scope "sidebar_attach"
local fitted = scope.window()

---Mux-domain splits can grow the tab past the window; re-sending the window size makes the mux refit it.
local function fit_to_window(tab)
  local gui = mux.call(mux.call(tab, "window"), "gui_window")
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
  local gui = mux.call(mux.call(tab, "window"), "gui_window")
  local palette = (mux.effective_config(gui) or {}).resolved_palette or {}
  local base = spaces.theme_for(config.get(), mux.window_id(gui), palette)
  local resolved = util.try(theme.resolve, base, palette)
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
  return mux.domain(base)
end

---"auto" adopts only where this plugin spawns backends itself; see docs/limitations.md.
local function may_adopt(cfg, domain, host, place)
  if cfg.adopt ~= "auto" then
    return cfg.adopt == true
  end
  return backend.is_local(domain, host)
    or store.spawned_domains[place] == true
    or backend.resolve_path(cfg, domain, host) ~= nil
end

---A candidate must sit in the very domain this plugin would have spawned the sidebar in.
local function adoptable(tab, sb)
  local cfg = config.get()
  local base = identity.content_pane(tab) or tab:active_pane()
  if not base then
    return false
  end
  local domain = attach_domain(cfg, base)
  local pane_domain = mux.domain(sb)
  if domain == nil or domain ~= pane_domain then
    return false
  end
  local host = identity.cwd_host(base)
  return may_adopt(cfg, domain, host, domain .. "@" .. (host or ""))
end

local function domain_failed(place, now)
  local at = store.failed_domains[place]
  if not at then
    return false
  end
  if now - at > FAILED_DOMAIN_MS then
    store.failed_domains[place] = nil
    return false
  end
  return true
end

---Splits off the sidebar pane; guarded because splits are async on mux domains.
---`new_tab`, `new_window` and `tear_off` call this directly, and a poll landing inside their await
---can attach first and clear the pending guard, so the tab is asked again here. A second split
---would stick: the loser keeps its marker but `classify` demotes it to content for good.
function M.attach(tab)
  local cfg = config.get()
  local tab_id = tab:tab_id()
  local now = util.now_ms()
  local pending = store.attaching[tab_id]
  if pending and now - pending < ATTACH_RETRY_MS then
    return nil
  end
  -- The per-poll cache predates the panes a split added, so it cannot answer this.
  identity.forget_split(tab_id)
  if identity.find(tab) then
    return nil
  end
  local base = identity.content_pane(tab)
  if not base or identity.is_overlay(base) then
    return nil
  end
  local pane_domain = attach_domain(cfg, base)
  if not pane_domain then
    util.warn_once("domain-" .. tab_id, "cannot determine domain for tab %d; sidebar skipped", tab_id)
    return nil
  end
  local host = identity.cwd_host(base)
  local place = pane_domain .. "@" .. (host or "")
  if domain_failed(place, now) then
    return nil
  end
  local args = backend.spawn_args(cfg, pane_domain, host)
  if not store.logged_domains[place] then
    store.logged_domains[place] = true
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
  store.attaching[tab_id] = now
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
  store.attaching[tab_id] = nil
  local pid = sb:pane_id()
  state.set_sidebar(tab_id, pid, util.random_token())
  store.seen[pid] = now
  store.pane_domain[pid] = place
  store.spawned_domains[place] = true
  identity.auth(sb)
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
  store.adopted[pid] = now
  store.auth_tries[pid] = 1
  store.seen[pid] = now
  identity.auth(sb)
end

---A pane that never echoes its token is never trusted; an adopted one is retried a few times, then left.
local function await_auth(gui_window, tab, sb, now)
  local pid = sb:pane_id()
  if state.sidebar_pane_id(tab:tab_id()) ~= pid then
    -- classify ranks a ready or mapped pane above a marker, so this tab has neither
    if not identity.has_marker(sb) or identity.is_overlay(sb) then
      return
    end
    if adoptable(tab, sb) then
      adopt(tab, sb, now)
    else
      -- not ours to take over: let it be content so the tab still gets a sidebar
      store.given_up[sb:pane_id()] = true
    end
    return
  end
  if store.adopted[pid] then
    local tries = store.auth_tries[pid] or 0
    if tries >= ADOPT_TRIES or now - store.adopted[pid] > ADOPT_WINDOW_MS then
      -- it kept the marker but never echoed: hand the tab back and treat the pane as content
      util.warn_once("adopt-" .. pid, "pane %d never authenticated; not a sidebar", pid)
      store.given_up[pid] = true
      store.adopted[pid] = nil
      state.set_sidebar(tab:tab_id(), nil)
      identity.forget_split(tab:tab_id())
    elseif now - (store.authed_at[pid] or 0) > ADOPT_RETRY_MS then
      store.auth_tries[pid] = tries + 1
      identity.auth(sb)
    end
    return
  end
  if not store.given_up[pid] and now - (store.seen[pid] or now) > READY_TIMEOUT_MS then
    M.give_up(gui_window, tab, sb)
  end
end

---`perform_action` ignores its pane argument, so the intended target must still be active when it runs.
local function still_active(gui_window, tab, pane)
  local active_tab = mux.active_tab(mux.call(gui_window, "mux_window"))
  if not active_tab or active_tab:tab_id() ~= tab:tab_id() then
    return false
  end
  local active_pane = mux.active_pane(active_tab)
  return active_pane ~= nil and active_pane:pane_id() == pane:pane_id()
end

local function close_pane_by_activation(gui_window, tab, sb)
  local content = identity.content_pane(tab)
  local previous = gui_window:mux_window():active_tab()
  sb:activate()
  if not still_active(gui_window, tab, sb) then
    util.warn("sidebar %d lost focus before close; left open", sb:pane_id())
    return
  end
  gui_window:perform_action(act.CloseCurrentPane { confirm = false }, sb)
  if content then
    content:activate()
  end
  if previous and previous:tab_id() ~= tab:tab_id() then
    previous:activate()
  end
end

function M.detach(gui_window, tab)
  local sb = identity.find(tab)
  if sb and not identity.is_ready(sb) then
    util.warn_once("detach-" .. sb:pane_id(), "sidebar %d never authenticated; left open", sb:pane_id())
    return
  end
  if sb then
    state.forget_pane(sb:pane_id())
    if not rescue.cli_kill(sb:pane_id()) then
      close_pane_by_activation(gui_window, tab, sb)
    end
  end
  state.set_sidebar(tab:tab_id(), nil)
end

---Closes a tab that only holds a sidebar without disturbing the active tab.
function M.close_orphan(gui_window, tab, sb)
  state.forget_pane(sb:pane_id())
  if rescue.cli_kill(sb:pane_id()) then
    return
  end
  local previous = gui_window:mux_window():active_tab()
  local switching = previous and previous:tab_id() ~= tab:tab_id()
  if switching then
    tab:activate()
  end
  if not still_active(gui_window, tab, sb) then
    util.warn("orphan tab %d lost focus before close; left open", tab:tab_id())
    return
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
  local place = store.pane_domain[pid] or "local@"
  if domain_failed(place, now) then
    return
  end
  store.failed_domains[place] = now
  store.given_up[pid] = true
  util.warn("sidebar backend did not start in %s; fix backend.path and close that pane", place)
end

---A backend that announced less than v2 or does not paint cannot be driven: its place is not
---retried for a while and the pane is left for the user, as a backend that never answered is.
function M.refuse_v1(pane, version)
  local pid = pane:pane_id()
  local place = store.pane_domain[pid] or "local@"
  store.given_up[pid] = true
  if not domain_failed(place, util.now_ms()) then
    store.failed_domains[place] = util.now_ms()
    util.warn_once(
      "v1-" .. place,
      "backend in %s speaks protocol v%s and cannot paint; update it (backend.path) and close that pane",
      place,
      tostring(version)
    )
  end
end

---Makes every tab match the collapsed/expanded state and closes tabs left with only a sidebar.
local function ensure_window(gui_window)
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  -- A rail keeps the pane and only narrows it (geometry.desired); "hidden" detaches as before.
  local collapsed = state.is_collapsed(wid) and config.get().collapsed == "hidden"
  local private = state.is_private(wid)
  local now = util.now_ms()
  local seen = {}
  identity.next_poll()
  local tabs = mux_win:tabs_with_info()
  local active_tab = mux.active_tab(mux_win)
  local active_id = active_tab and active_tab:tab_id() or nil

  rescue.resolve_pins(tabs, now)
  rescue.prune_windows(now)

  for _, info in ipairs(tabs) do
    local tab = info.tab
    local tab_id = tab:tab_id()
    local content, sb = identity.classify(tab)
    if #content == 0 then
      if sb then
        -- the tab still exists; forgetting it would strand a sidebar that authenticates later
        seen[tab_id] = true
        if not identity.is_ready(sb) then
          await_auth(gui_window, tab, sb, now)
        elseif #tab:panes() == 1 then
          M.close_orphan(gui_window, tab, sb)
        end
      end
    else
      seen[tab_id] = true
      local active = tab:active_pane()
      if active and (not sb or active:pane_id() ~= sb:pane_id()) then
        store.content_pane[tab_id] = active:pane_id()
      end
      store.tab_meta[tab_id] = identity.tab_meta(tab, identity.content_pane(tab))
      if collapsed then
        if sb and identity.is_ready(sb) then
          M.detach(gui_window, tab)
        end
      elseif sb then
        if identity.is_ready(sb) then
          rescue.rescue_splits(gui_window, tab)
          rescue.check_liveness(gui_window, tab, sb, now)
        else
          await_auth(gui_window, tab, sb, now)
        end
      elseif tab_id == active_id then
        -- background tabs attach when they are first activated: 20 splits at once cost ~460 ms
        M.attach(tab)
      end
    end
  end
  rescue.record_closed_tabs(wid, seen, private)
end

local ensuring = scope.window()

---Splits and closes await, so several event handlers can be inside `ensure` for one window at once.
function M.ensure(gui_window)
  local wid = gui_window:window_id()
  if ensuring[wid] then
    return
  end
  ensuring[wid] = true
  local ok, err = pcall(ensure_window, gui_window)
  ensuring[wid] = nil
  if not ok then
    error(err, 0)
  end
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

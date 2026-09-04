local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local geometry = require "vtabs.geometry"
local platform = require "vtabs.platform"
local mux = require "vtabs.mux"
local link = require "vtabs.link"
local snapshot = require "vtabs.snapshot"
local theme_bridge = require "vtabs.theme_bridge"
local transport = require "vtabs.transport"
local util = require "vtabs.util"

local M = {}

---Declared through `store`, so forgetting a window clears them without a list to keep in step.
local scope = store.scope "view"
local chrome = scope.window()
local banding, banding_saved = scope.window(), scope.window()
local effective = scope.window()
local last_strip_dpi = scope.window()

---`effective_config` hands over the whole config as a Lua table, every colour scheme included, on
---each call. Nothing read from it moves without a reload, and a reload drops this.
function M.effective_config(gui_window)
  local wid = gui_window:window_id()
  local cached = effective[wid]
  if cached == nil then
    cached = mux.effective_config(gui_window)
    effective[wid] = cached
  end
  return cached
end

---WezTerm titles the window after its active pane; under `hover = "follow"` that is the sidebar.
function M.window_title(tab, pane, tabs, panes)
  if type(tab) ~= "table" or type(pane) ~= "table" then
    return nil
  end
  local function backend(info)
    local resolved = info and mux.pane_by_id(info.pane_id)
    return resolved ~= nil and sidebar.is_backend(resolved)
  end
  if not backend(pane) then
    return nil
  end
  local title = nil
  for _, info in ipairs(panes or {}) do
    if info.pane_id ~= pane.pane_id and not backend(info) then
      title = util.sanitize(info.title)
      break
    end
  end
  title = (title ~= "" and title) or util.sanitize(tab.tab_title)
  if title == "" then
    title = nil
  end
  if not title then
    return nil
  end
  local count = tabs and #tabs or 1
  if count > 1 then
    return string.format("[%d/%d] %s", (tab.tab_index or 0) + 1, count, title)
  end
  return title
end

---The active tab's painting sidebar when this window may fade, or nil.
local function fade_context(gui_window)
  local cfg = config.get()
  if cfg.animations == "off" then
    return nil
  end
  local tab = util.active_tab(gui_window)
  local sb = tab and sidebar.find(tab)
  if not sb or not sidebar.is_ready(sb) then
    return nil
  end
  if cfg.animations == "auto" and mux.domain(sb) ~= "local" then
    return nil
  end
  return cfg, sb
end

-- The configured duration is the fade the user sees; the pre-resize phase keeps its own constant.
local PHASE_MS = { expand_in = "expand_ms", collapse_in = "collapse_ms" }

---Fades the active tab's sidebar through one phase on the backend's own clock.
function M.animate(gui_window, phase)
  local cfg, sb = fade_context(gui_window)
  if not cfg then
    return false
  end
  local key = PHASE_MS[phase]
  return sidebar.send(sb, { t = "fx", phase = phase, ms = key and cfg.animation[key] or nil, fps = cfg.animation.fps })
end

---The menu rises out of the sidebar surface in one colour fade over its own rows; nothing on close.
function M.animate_popover(gui_window)
  local cfg, sb = fade_context(gui_window)
  if not cfg or cfg.popover.fade_ms <= 0 then
    return false
  end
  return sidebar.send(sb, { t = "fx", phase = "popover_in", ms = cfg.popover.fade_ms, fps = cfg.animation.fps })
end

---Without a window id, every window's: a config reload invalidates them all at once.
function M.invalidate_theme(window_id, preserve_resolved)
  if not preserve_resolved then
    theme_bridge.clear(window_id)
  end
  if window_id then
    chrome[window_id] = nil
    effective[window_id] = nil
    return
  end
  for _, cache in ipairs { chrome, effective } do
    for id in pairs(cache) do
      cache[id] = nil
    end
  end
end

---macOS shows its buttons only for `INTEGRATED_BUTTONS` with a native style; both gate the reserve.
local function chrome_for(gui_window, cfg, observed)
  local wid = gui_window:window_id()
  if not chrome[wid] then
    local config_now = observed and observed.effective or M.effective_config(gui_window) or {}
    local decorations = tostring(config_now.window_decorations or "")
    -- `titlebar = "macos"` is the preview knob: it claims the reserve on a machine that has none.
    local preview = cfg.titlebar == "macos"
    local asked = preview
      or cfg.titlebar == "integrate"
      or (cfg.titlebar == "auto" and decorations:find("INTEGRATED_BUTTONS", 1, true) ~= nil)
    local native = preview
      or config_now.integrated_title_button_style == nil
      or config_now.integrated_title_button_style == "MacOsNative"
    if platform.is_mac and asked and not native then
      util.warn_once("button-style", "integrated_title_button_style must be MacOsNative to reserve cells")
    end
    chrome[wid] = {
      integrated_buttons = asked,
      native_button_style = native,
      preview = preview,
    }
  end
  return chrome[wid]
end

---Whether AppKit is drawing its buttons over this window's grid right now.
local function lights_overhang(gui_window, cfg)
  local c = chrome_for(gui_window, cfg)
  -- the preview claims the reserve, so it has to claim the overhang too or the two disagree
  if not (platform.is_mac or c.preview) then
    return false
  end
  if not (c.integrated_buttons and c.native_button_style) then
    return false
  end
  return (mux.dims(gui_window) or {}).is_full_screen ~= true
end

---`collapsed = "hidden"` detaches the pane that reserved cells for the traffic lights, so nothing
---owns the window's top-left and the lights land on the user's shell. A window-wide padding band
---is the only mechanism that moves the grid out from under them; it costs one relayout per toggle.
function M.apply_titlebar_band(gui_window)
  local cfg = config.get()
  if cfg.rail_titlebar == "none" then
    return false
  end
  local wid = gui_window:window_id()
  local wanted = cfg.collapsed == "hidden" and state.is_collapsed(wid) and lights_overhang(gui_window, cfg)
  local overrides = mux.overrides(gui_window) or {}
  local banded = banding[wid] == true
  if wanted == banded then
    return false
  end
  local merged = {}
  for key, value in pairs(overrides) do
    merged[key] = value
  end
  if wanted then
    local user = (mux.effective_config(gui_window) or {}).window_padding or {}
    banding_saved[wid] = overrides.window_padding
    merged.window_padding = {
      left = user.left,
      right = user.right,
      bottom = user.bottom,
      top = platform.TITLEBAR_PAD,
    }
  else
    merged.window_padding = banding_saved[wid]
    banding_saved[wid] = nil
  end
  banding[wid] = wanted or nil
  -- The override fires `window-config-reloaded`; the guard keeps it from re-entering correction.
  store.applying[wid] = util.now_ms()
  mux.call(gui_window, "set_config_overrides", merged)
  return true
end

---Fresh chrome plus the window's dpi for Rust's sole strip-geometry calculation. The pane's cells
---and pixel size the backend measures for itself; the dpi is the one number it cannot.
local function strip_facts(gui_window, cfg, dpi, window, observed)
  local facts = chrome_for(gui_window, cfg, observed)
  window = window or {}
  return {
    dpi = dpi,
    chrome = {
      is_mac = platform.is_mac,
      integrated_buttons = facts.integrated_buttons,
      native_button_style = facts.native_button_style,
      preview = facts.preview,
      is_full_screen = window.is_full_screen == true,
    },
  }
end

---Nil when the pane cannot report a size; the frame is then skipped rather than painted at a guess.
local function dims_of(pane, observed)
  local d
  if observed then
    local facts = observed.panes[pane:pane_id()]
    d = facts and facts.dimensions or nil
  else
    d = mux.dims(pane)
  end
  if d and d.cols and d.viewport_rows then
    return d
  end
  return nil
end

local function footer_for(cfg, mux_win)
  if not cfg.hooks.footer then
    return nil
  end
  local ok, rows = pcall(cfg.hooks.footer, mux_win)
  if not ok then
    util.warn_once("hook-footer", "footer hook failed: %s", tostring(rows))
    return nil
  end
  return type(rows) == "table" and rows or nil
end

---One frame of a window resize. WezTerm has already dealt the sidebar its share of the new columns
---by the time this fires, so the frame is corrected here and now; the timer armed by the last frame
---corrects whatever a tab had to wait for, then publishes.
function M.on_resize(gui_window)
  local wid = gui_window:window_id()
  local tick = geometry.on_resize(wid)
  geometry.correct(gui_window)
  local attempts = 0
  local settle
  settle = function()
    if geometry.resize_tick(wid) ~= tick then
      return
    end
    attempts = attempts + 1
    local ok, err = pcall(function()
      geometry.correct(gui_window)
      M.sync(gui_window)
    end)
    if not ok and not util.window_gone(err) then
      util.warn("window-resized: %s", tostring(err))
    end
    if ok and attempts < 4 and geometry.has_pending_adjust(wid) then
      wezterm.time.call_after(geometry.REMOTE_APPLY_MS / 1000, settle)
    end
  end
  wezterm.time.call_after(geometry.SETTLE_MS / 1000, settle)
end

---Whether this publish would type a frame into a pane on a busy mux link. Only such a pane holds a
---publish back: one whose frames go through its inbox is written to at once, and a local pane
---never blocks.
local function held_by_link(observed)
  local wire = require "vtabs.wire"
  local function stalls(pane, tab_id)
    if pane == nil then
      return false
    end
    local pid = pane:pane_id()
    -- the wire writes to the shown tab's pane and to one never dressed; nothing else is touched
    if wire.dressed(pid) and tab_id ~= observed.active_tab_id then
      return false
    end
    local facts = observed.panes[pid]
    local domain = facts and facts.domain or mux.domain(pane)
    return link.busy(domain) and transport.state(pane) ~= "active"
  end
  for _, tab in ipairs(observed.tabs) do
    if tab.sidebar_ready and stalls(tab.sidebar, tab.tab_id) then
      return true
    end
  end
  local page = observed.settings
  return page ~= nil and stalls(page.pane, page.tab_id)
end

---Publishes this window's state: the wire sends whatever changed to every painting pane.
function M.sync(gui_window)
  local cfg = config.get()
  -- Every frame of a window resize changes the sidebar's metrics, and a publish per frame is a
  -- full spaces/theme round trip per frame. The sidebar repaints itself from
  -- its own size meanwhile; the settle timer publishes once the frames have stopped.
  if geometry.in_burst(gui_window:window_id()) then
    return false
  end
  if cfg.debug then
    util.log("sync: window %d", gui_window:window_id())
  end
  local observed = snapshot.capture(gui_window, { cfg = cfg, effective = M.effective_config(gui_window) })
  -- Nor while a mux link is busy rebuilding its mirror and a frame of this publish would cross it
  -- on this thread, blocking, which has deadlocked the GUI (link.lua). A pane on its inbox transport
  -- is published to all the same: none of its frames crosses the link.
  if link.busy_any() and held_by_link(observed) then
    return false
  end
  if cfg.debug then
    util.log(
      "snapshot: window=%d mux_collections=%d pane_collections=%d tabs=%d",
      observed.window_id,
      observed.stats.mux_collections,
      observed.stats.pane_collections,
      #observed.tabs
    )
  end
  local mux_win = observed.mux_window
  local wid = observed.window_id
  local survey = model.survey(gui_window, observed)
  local items = survey.visible
  local footer = footer_for(cfg, mux_win)
  local active_tab_id = observed.active_tab_id
  local now = observed.now
  -- A correction changes the layout this observation describes. Publish nothing from it; the
  -- sidebar reports its new size at once, and that report publishes a fresh observation.
  if geometry.sync(gui_window, observed) then
    return false
  end
  -- After the width settles, so the card is drawn at the pane rect the correction leaves behind.
  require("vtabs.frame").sync(gui_window, observed)

  local window = observed.window_dims
  for _, tab in ipairs(observed.tabs) do
    local sb = tab.sidebar
    if sb and tab.sidebar_ready then
      local pid = sb:pane_id()
      local dims = dims_of(sb, observed)
      if dims then
        store.dims[pid] = { cols = dims.cols, rows = dims.viewport_rows }
        store.sent_at[pid] = now
      end
    end
  end

  -- Only the active tab's current sidebar can describe the shared model. A missing observation may
  -- reuse that exact pane's last dpi, never a background or replaced pane's.
  local active = observed.active
  local active_sb = active and active.sidebar or nil
  local active_pid = active_sb and active_sb:pane_id() or nil
  local dims = active_sb and dims_of(active_sb, observed) or nil
  local dpi = dims and dims.dpi or nil
  if dpi then
    last_strip_dpi[wid] = { tab_id = active_tab_id, pane_id = active_pid, value = dpi }
  else
    local cached = last_strip_dpi[wid]
    if cached and cached.tab_id == active_tab_id and cached.pane_id == active_pid then
      dpi = cached.value
    end
  end
  -- Chrome and fullscreen are deliberately rebuilt even when the dpi came from the cache.
  local wire_strip = strip_facts(gui_window, cfg, dpi, window, observed)
  local config_now = observed.effective
  require("vtabs.wire").sync(gui_window, {
    cfg = cfg,
    items = items,
    footer = footer,
    active_tab_id = active_tab_id,
    effective = config_now,
    strip = wire_strip,
    theme_base = cfg.theme,
    private = state.is_private(wid),
    space = survey.space,
    spaces = survey.spaces,
    survey = survey,
    snapshot = observed,
    window_dims = window,
  })
  return true
end

return M

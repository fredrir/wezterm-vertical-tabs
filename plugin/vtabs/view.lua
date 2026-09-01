local config = require "vtabs.config"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local geometry = require "vtabs.geometry"
local platform = require "vtabs.platform"
local mux = require "vtabs.mux"
local util = require "vtabs.util"

local M = {}

---Declared through `store`, so forgetting a window clears them without a list to keep in step.
local scope = store.scope "view"
local theme_hooks = scope.window()
local chrome = scope.window()
local banding, banding_saved = scope.window(), scope.window()

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
function M.invalidate_theme(window_id)
  if window_id then
    theme_hooks[window_id] = nil
    chrome[window_id] = nil
    return
  end
  for _, cache in ipairs { theme_hooks, chrome } do
    for id in pairs(cache) do
      cache[id] = nil
    end
  end
end

---macOS shows its buttons only for `INTEGRATED_BUTTONS` with a native style; both gate the reserve.
local function chrome_for(gui_window, cfg)
  local wid = gui_window:window_id()
  if not chrome[wid] then
    local effective = mux.effective_config(gui_window) or {}
    local decorations = tostring(effective.window_decorations or "")
    -- `titlebar = "macos"` is the preview knob: it claims the reserve on a machine that has none.
    local preview = cfg.titlebar == "macos"
    local asked = preview
      or cfg.titlebar == "integrate"
      or (cfg.titlebar == "auto" and decorations:find("INTEGRATED_BUTTONS", 1, true) ~= nil)
    local native = preview
      or effective.integrated_title_button_style == nil
      or effective.integrated_title_button_style == "MacOsNative"
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

---A rail has no room beside the lights, so its toggle centres below them; without telling
---`strip_geometry` which mode it is in, the toggle lands off the end of the rail.
local function strip_for(gui_window, cfg, dims, rail)
  local facts = chrome_for(gui_window, cfg)
  local wid = gui_window:window_id()
  local window = mux.dims(gui_window) or {}
  local g = platform.strip_geometry(dims, {
    is_mac = platform.is_mac or facts.preview,
    preview = facts.preview and not platform.is_mac or nil,
    integrated_buttons = facts.integrated_buttons,
    native_button_style = facts.native_button_style,
    is_full_screen = window.is_full_screen == true,
    position = cfg.position,
    padding_top = cfg.padding.top,
    toggle_button = cfg.toggle_button,
    card_x1 = cfg.padding.left + 1,
    rail = rail or nil,
    rail_width = rail and cfg.rail_width or nil,
  })
  -- `desired` has no cell size of its own, so the reserve a frame measured is handed to geometry.
  geometry.set_rail_cols(wid, g.cols)
  local toggle = nil
  if cfg.toggle_button and g.rows > 0 then
    toggle = { row = g.toggle_row, x = g.toggle_x, x1 = math.max(1, g.toggle_x - 1), x2 = g.toggle_x + 2 }
  end
  return { rows = g.rows, cols = g.cols, cell_w = g.cell_w, toggle = toggle, toggle_row = g.toggle_row }
end

---What `hooks.theme` answers for this window, cached until a reload; nil when it has nothing to say.
local function theme_override_for(gui_window, cfg)
  local wid = gui_window:window_id()
  if theme_hooks[wid] == nil then
    local custom = false
    if cfg.hooks.theme then
      local effective = mux.effective_config(gui_window)
      local palette = effective and effective.resolved_palette or {}
      local resolved = require("vtabs.theme").resolve(cfg.theme, palette, { private = state.is_private(wid) })
      local ok, answer = pcall(cfg.hooks.theme, gui_window, resolved)
      if not ok then
        util.warn_once("hook-theme", "theme hook failed: %s", tostring(answer))
      elseif type(answer) == "table" then
        custom = answer
      end
    end
    theme_hooks[wid] = custom
  end
  return theme_hooks[wid] or nil
end

---Nil when the pane cannot report a size; the frame is then skipped rather than painted at a guess.
local function dims_of(pane)
  local d = mux.dims(pane)
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

---Publishes this window's state: the wire sends whatever changed to every painting pane.
function M.sync(gui_window)
  local cfg = config.get()
  if cfg.debug then
    util.log("sync: window %d", gui_window:window_id())
  end
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  local items = model.build(gui_window)
  local footer = footer_for(cfg, mux_win)
  local active_tab = mux_win:active_tab()
  local active_tab_id = active_tab and active_tab:tab_id() or nil
  local now = util.now_ms()
  geometry.sync(gui_window, active_tab_id)
  -- After the width settles, so the card is drawn at the pane rect the correction leaves behind.
  require("vtabs.frame").sync(gui_window)

  local wire_strip = nil
  for _, info in ipairs(mux_win:tabs_with_info()) do
    local sb = sidebar.find(info.tab)
    if sb and sidebar.is_ready(sb) then
      local pid = sb:pane_id()
      local is_active = info.tab:tab_id() == active_tab_id
      local dims = dims_of(sb)
      if dims then
        local rail = state.is_collapsed(wid) and cfg.collapsed == "rail" or nil
        local strip = strip_for(gui_window, cfg, dims, rail)
        if is_active or wire_strip == nil then
          wire_strip = strip
        end
        store.dims[pid] = { cols = dims.cols, rows = dims.viewport_rows }
        store.sent_at[pid] = now
      end
    end
  end
  require("vtabs.wire").sync(gui_window, {
    cfg = cfg,
    items = items,
    footer = footer,
    active_tab_id = active_tab_id,
    effective = mux.effective_config(gui_window),
    chrome = chrome_for(gui_window, cfg),
    strip = wire_strip,
    theme_override = theme_override_for(gui_window, cfg),
    window_dims = mux.dims(gui_window),
  })
end

return M

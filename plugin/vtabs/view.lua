local wezterm = require "wezterm" ---@type Wezterm
local ansi = require "vtabs.ansi"
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local settings = require "vtabs.settings"
local page = require "vtabs.page"
local model = require "vtabs.model"
local render = require "vtabs.render"
local geometry = require "vtabs.geometry"
local popover = require "vtabs.popover"
local anim = require "vtabs.anim"
local theme = require "vtabs.theme"
local platform = require "vtabs.platform"
local glyphs = require "vtabs.glyphs"
local util = require "vtabs.util"

local M = {}

local INACTIVE_REFRESH_MS = 1000

local themes = {}
local chrome = {}
local banding, banding_saved = {}, {}
local popover_rect = {}
local session = state.session

---WezTerm titles the window after its active pane; under `hover = "follow"` that is the sidebar.
function M.window_title(tab, pane, tabs, panes)
  if type(tab) ~= "table" or type(pane) ~= "table" then
    return nil
  end
  local function backend(info)
    local resolved = info and util.try(function()
      return wezterm.mux.get_pane(info.pane_id)
    end)
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

---Everything a fade needs, or nil when this window cannot play one.
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
  local domain = util.try(function()
    return sb:get_domain_name()
  end)
  if cfg.animations == "auto" and domain ~= "local" then
    return nil
  end
  local cached = session.frames[sb:pane_id()]
  if not cached or not cached.text then
    return nil
  end
  local resolved = themes[gui_window:window_id()]
  if not resolved or not resolved.bg then
    return nil
  end
  return cfg, sb, cached, resolved
end

local function hex(rgb)
  return string.format("#%02x%02x%02x", rgb[1], rgb[2], rgb[3])
end

-- The configured duration is the fade the user sees; the pre-resize phase keeps its own constant.
local PHASE_MS = { expand_in = "expand_ms", collapse_in = "collapse_ms" }

---Fades the active tab's sidebar through one phase; the width still changes in a single step.
function M.animate(gui_window, phase)
  local cfg, sb, cached, resolved = fade_context(gui_window)
  if not cfg then
    return false
  end
  local key = PHASE_MS[phase]
  local command = anim.build(phase, { rows = cached.text, rows_n = cached.n }, {
    anchor = hex(resolved.bg),
    fps = cfg.animation.fps,
    ms = key and cfg.animation[key] or nil,
  })
  return command ~= nil and sidebar.send(sb, command) or false
end

---The menu rises out of the sidebar surface in one colour fade over its own rows; nothing on close.
function M.animate_popover(gui_window)
  local rect = popover_rect[gui_window:window_id()]
  local cfg, sb, cached, resolved = fade_context(gui_window)
  if not rect or not cfg or cfg.popover.fade_ms <= 0 then
    return false
  end
  local rows = {}
  for row = rect.y, rect.y + rect.h - 1 do
    rows[#rows + 1] = row
  end
  local command = anim.build("popover_in", { rows = cached.text, rows_n = cached.n }, {
    anchor = hex(resolved.bg),
    fps = cfg.animation.fps,
    ms = cfg.popover.fade_ms,
    rows = rows,
  })
  return command ~= nil and sidebar.send(sb, command) or false
end

function M.invalidate_theme(window_id)
  if window_id then
    themes[window_id] = nil
    chrome[window_id] = nil
    popover_rect[window_id] = nil
  else
    themes, chrome, popover_rect = {}, {}, {}
  end
end

table.insert(state.forget_hooks, M.invalidate_theme)

---macOS shows its buttons only for `INTEGRATED_BUTTONS` with a native style; both gate the reserve.
local function chrome_for(gui_window, cfg)
  local wid = gui_window:window_id()
  if not chrome[wid] then
    local effective = util.try(function()
      return gui_window:effective_config()
    end) or {}
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
      glyphs = glyphs.resolve(cfg.glyphs, effective),
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
  local fullscreen = util.try(function()
    return gui_window:get_dimensions().is_full_screen
  end)
  return fullscreen ~= true
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
  local overrides = util.try(function()
    return gui_window:get_config_overrides()
  end) or {}
  local banded = banding[wid] == true
  if wanted == banded then
    return false
  end
  local merged = {}
  for key, value in pairs(overrides) do
    merged[key] = value
  end
  if wanted then
    local user = util.try(function()
      return gui_window:effective_config().window_padding
    end) or {}
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
  session.applying[wid] = util.now_ms()
  util.try(function()
    gui_window:set_config_overrides(merged)
  end)
  return true
end

---Remembered so the fade can be played over exactly the rows the menu took.
local function rect_for(gui_window, dims, resolved, cfg)
  local rect = popover.rect(gui_window, dims.viewport_rows, dims.cols, resolved, cfg)
  popover_rect[gui_window:window_id()] = rect
  return rect
end

local function strip_for(gui_window, cfg, dims, rail)
  local facts = chrome_for(gui_window, cfg)
  local window = util.try(function()
    return gui_window:get_dimensions()
  end) or {}
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
    -- without these the rail branch never fires and the toggle is placed off the end of the rail
    rail = rail or nil,
    rail_width = rail and cfg.rail_width or nil,
  })
  local toggle = nil
  if cfg.toggle_button and g.rows > 0 then
    toggle = { row = g.toggle_row, x = g.toggle_x, x1 = math.max(1, g.toggle_x - 1), x2 = g.toggle_x + 2 }
  end
  return { rows = g.rows, cols = g.cols, toggle = toggle, toggle_row = g.toggle_row }
end

local function theme_for(gui_window, cfg)
  local wid = gui_window:window_id()
  if not themes[wid] then
    local effective = util.try(function()
      return gui_window:effective_config()
    end)
    local palette = effective and effective.resolved_palette or {}
    local resolved = theme.resolve(cfg.theme, palette, { private = state.is_private(wid) })
    if cfg.hooks.theme then
      local ok, custom = pcall(cfg.hooks.theme, gui_window, resolved)
      if not ok then
        util.warn_once("hook-theme", "theme hook failed: %s", tostring(custom))
      elseif type(custom) == "table" then
        resolved = theme.resolve(custom, palette)
      end
    end
    themes[wid] = resolved
  end
  return themes[wid]
end

---Nil when the pane cannot report a size; the frame is then skipped rather than painted at a guess.
local function dims_of(pane)
  local d = util.try(function()
    return pane:get_dimensions()
  end)
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

---Re-renders sidebars in the window; inactive tabs refresh lazily, frames are sent only when changed.
---Rows whose painted text changed, each with its own CUP; a full frame when the cache cannot be trusted.
---@return string|nil `nil` when nothing changed
function M.payload_for(pid, result, dims, force)
  local cache = session.frames[pid]
  local stale = force
    or cache == nil
    or cache.cols ~= dims.cols
    or cache.rows ~= dims.viewport_rows
    or cache.n ~= result.rows_n
  if stale then
    return result.data
  end
  local parts = {}
  for row = 1, result.rows_n do
    if cache.text[row] ~= result.rows[row] then
      parts[#parts + 1] = ansi.cup(row, 1) .. result.rows[row]
    end
  end
  if #parts == 0 then
    return nil
  end
  return ansi.HIDE_CURSOR .. table.concat(parts) .. ansi.RESET
end

---Drops the row cache for a pane, forcing the next sync to repaint it whole.
function M.invalidate_frames(pane_id)
  if pane_id then
    session.frames[pane_id] = nil
  else
    for id in pairs(session.frames) do
      session.frames[id] = nil
    end
  end
end

---The settings page: the same bridge, the same row-diff, its own frame producer.
local function sync_settings(gui_window, cfg, resolved, opts, now)
  local _, pane = settings.find(gui_window:mux_window())
  if not pane or not sidebar.is_ready(pane) then
    return
  end
  local pid = pane:pane_id()
  local dims = dims_of(pane)
  if not dims or dims.cols == 0 then
    return
  end
  local ok, result = pcall(page.paint, {
    cols = dims.cols,
    rows = dims.viewport_rows,
    cfg = cfg,
    theme = resolved,
    glyphs = chrome_for(gui_window, cfg).glyphs,
    st = settings.page_state(gui_window:window_id()),
  })
  if not ok then
    util.warn_once("settings-render", "settings page render failed: %s", tostring(result):match "^[^\n]*")
    return
  end
  session.hits[pid] = result.hits
  session.dims[pid] = { cols = dims.cols, rows = dims.viewport_rows }
  local payload = M.payload_for(pid, result, dims, opts.force)
  if payload and sidebar.send(pane, { t = "frame", data = payload }) then
    session.frames[pid] = { cols = dims.cols, rows = dims.viewport_rows, text = result.rows, n = result.rows_n }
    session.sent_at[pid] = now
  end
end

function M.sync(gui_window, opts)
  opts = opts or {}
  local cfg = config.get()
  if cfg.debug then
    util.log("sync: window %d", gui_window:window_id())
  end
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  local items = model.build(gui_window)
  local resolved = theme_for(gui_window, cfg)
  local footer = footer_for(cfg, mux_win)
  local active_tab = mux_win:active_tab()
  local active_tab_id = active_tab and active_tab:tab_id() or nil
  local focus_index = state.has_focus(wid) and session.focus_index[wid] or nil
  local now = util.now_ms()
  geometry.sync(gui_window, active_tab_id)
  sync_settings(gui_window, cfg, resolved, opts, now)

  for _, info in ipairs(mux_win:tabs_with_info()) do
    local sb = sidebar.find(info.tab)
    if sb and sidebar.is_ready(sb) then
      local pid = sb:pane_id()
      local is_active = info.tab:tab_id() == active_tab_id
      local due = is_active or opts.force or now - (session.sent_at[pid] or 0) >= INACTIVE_REFRESH_MS
      local dims = due and dims_of(sb) or nil
      if dims then
        local rail = state.is_collapsed(wid) and cfg.collapsed == "rail" or nil
        local strip = strip_for(gui_window, cfg, dims, rail)
        -- a title or cwd that breaks one render must not stop the other sidebars in this window
        local ok, result = pcall(render.render, {
          cols = dims.cols,
          rows = dims.viewport_rows,
          strip = strip,
          items = items,
          theme = resolved,
          cfg = cfg,
          glyphs = chrome_for(gui_window, cfg).glyphs,
          hover = is_active and session.hover[wid] or nil,
          drag = is_active and session.drag[wid] or nil,
          scroll = session.scroll[wid] or 0,
          user_scrolled = session.user_scrolled[wid] == true,
          ensure_visible = not session.user_scrolled[wid] and active_tab_id or nil,
          focus_index = is_active and focus_index or nil,
          private = state.is_private(wid),
          rail = rail,
          popover = is_active and rect_for(gui_window, dims, resolved, cfg) or nil,
          footer = footer,
        })
        if not ok then
          util.warn_once("render-failed", "sidebar render failed: %s", tostring(result):match "^[^\n]*")
          result = nil
        end
        if result and is_active then
          session.scroll[wid] = result.scroll
        end
        if result then
          session.hits[pid] = result.hits
          session.dims[pid] = { cols = dims.cols, rows = dims.viewport_rows }
          local payload = M.payload_for(pid, result, dims, opts.force)
          if payload and sidebar.send(sb, { t = "frame", data = payload }) then
            session.frames[pid] = { cols = dims.cols, rows = dims.viewport_rows, text = result.rows, n = result.rows_n }
            session.sent_at[pid] = now
          end
        end
      end
    end
  end
end

return M

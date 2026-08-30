local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local render = require "vtabs.render"
local geometry = require "vtabs.geometry"
local theme = require "vtabs.theme"
local platform = require "vtabs.platform"
local glyphs = require "vtabs.glyphs"
local util = require "vtabs.util"

local M = {}

local INACTIVE_REFRESH_MS = 1000

local themes = {}
local chrome = {}
local session = state.session

---WezTerm titles the window after its active pane; under `hover = "follow"` that is the sidebar.
function M.window_title(tab, pane, tabs, panes)
  if type(tab) ~= "table" or type(pane) ~= "table" then
    return nil
  end
  local is_sidebar = sidebar.marker(pane.title) or state.sidebar_pane_id(tab.tab_id) == pane.pane_id
  if not is_sidebar then
    return nil
  end
  local title = nil
  for _, info in ipairs(panes or {}) do
    if info.pane_id ~= pane.pane_id and not sidebar.marker(info.title) then
      title = info.title
      break
    end
  end
  title = title or (tab.tab_title ~= "" and tab.tab_title) or nil
  if not title then
    return nil
  end
  local count = tabs and #tabs or 1
  if count > 1 then
    return string.format("[%d/%d] %s", (tab.tab_index or 0) + 1, count, title)
  end
  return title
end

function M.invalidate_theme(window_id)
  if window_id then
    themes[window_id] = nil
    chrome[window_id] = nil
  else
    themes, chrome = {}, {}
  end
end

table.insert(state.forget_hooks, M.invalidate_theme)

---macOS only shows its window buttons for `TITLE`/`INTEGRATED_BUTTONS`, and only keeps
---`INTEGRATED_BUTTONS` when the button style is native; both decide whether cells are reserved.
local function chrome_for(gui_window, cfg)
  local wid = gui_window:window_id()
  if not chrome[wid] then
    local effective = util.try(function()
      return gui_window:effective_config()
    end) or {}
    local decorations = tostring(effective.window_decorations or "")
    local asked = cfg.titlebar == "integrate"
      or (cfg.titlebar == "auto" and decorations:find("INTEGRATED_BUTTONS", 1, true) ~= nil)
    local native = effective.integrated_title_button_style == nil
      or effective.integrated_title_button_style == "MacOsNative"
    if platform.is_mac and asked and not native then
      util.warn_once("button-style", "integrated_title_button_style must be MacOsNative to reserve cells")
    end
    chrome[wid] = {
      integrated_buttons = asked,
      native_button_style = native,
      glyphs = glyphs.resolve(cfg.glyphs, effective),
    }
  end
  return chrome[wid]
end

local function strip_for(gui_window, cfg, dims)
  local facts = chrome_for(gui_window, cfg)
  local window = util.try(function()
    return gui_window:get_dimensions()
  end) or {}
  local g = platform.strip_geometry(dims, {
    is_mac = platform.is_mac,
    integrated_buttons = facts.integrated_buttons,
    native_button_style = facts.native_button_style,
    is_full_screen = window.is_full_screen == true,
    position = cfg.position,
    padding_top = cfg.padding.top,
    toggle_button = cfg.toggle_button,
    card_x1 = cfg.padding.left + 1,
  })
  local toggle = nil
  if cfg.toggle_button and g.rows > 0 then
    toggle = { row = g.toggle_row, x = g.toggle_x, x1 = math.max(1, g.toggle_x - 1), x2 = g.toggle_x + 2 }
  end
  return { rows = g.rows, cols = g.cols, toggle = toggle }
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

  for _, info in ipairs(mux_win:tabs_with_info()) do
    local sb = sidebar.find(info.tab)
    if sb and sidebar.is_ready(sb) then
      local pid = sb:pane_id()
      local is_active = info.tab:tab_id() == active_tab_id
      local due = is_active or opts.force or now - (session.sent_at[pid] or 0) >= INACTIVE_REFRESH_MS
      local dims = due and dims_of(sb) or nil
      if dims then
        local strip = strip_for(gui_window, cfg, dims)
        local result = render.render {
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
          footer = footer,
        }
        if is_active then
          session.scroll[wid] = result.scroll
        end
        session.hits[pid] = result.hits
        session.dims[pid] = { cols = dims.cols, rows = dims.viewport_rows, strip_rows = strip.rows }
        if opts.force or session.frames[pid] ~= result.data then
          if sidebar.send(sb, { t = "frame", data = result.data }) then
            session.frames[pid] = result.data
            session.sent_at[pid] = now
          end
        end
      end
    end
  end
end

return M

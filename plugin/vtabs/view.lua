local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local render = require "vtabs.render"
local geometry = require "vtabs.geometry"
local theme = require "vtabs.theme"
local util = require "vtabs.util"

local M = {}

local INACTIVE_REFRESH_MS = 1000

local themes = {}
local session = state.session

function M.invalidate_theme(window_id)
  if window_id then
    themes[window_id] = nil
  else
    themes = {}
  end
end

local function theme_for(gui_window, cfg)
  local wid = gui_window:window_id()
  if not themes[wid] then
    local effective = util.try(function()
      return gui_window:effective_config()
    end)
    local palette = effective and effective.resolved_palette or {}
    local resolved = theme.resolve(cfg.theme, palette)
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
    return d.cols, d.viewport_rows
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
  geometry.correct(gui_window)
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  local items = model.build(gui_window)
  local resolved = theme_for(gui_window, cfg)
  local footer = footer_for(cfg, mux_win)
  local active_tab = mux_win:active_tab()
  local active_tab_id = active_tab and active_tab:tab_id() or nil
  local focus_index = state.has_focus(wid) and session.focus_index[wid] or nil
  local now = util.now_ms()

  for _, info in ipairs(mux_win:tabs_with_info()) do
    local sb = sidebar.find(info.tab)
    if sb and sidebar.is_ready(sb) then
      local pid = sb:pane_id()
      local is_active = info.tab:tab_id() == active_tab_id
      local due = is_active or opts.force or now - (session.sent_at[pid] or 0) >= INACTIVE_REFRESH_MS
      local cols, rows
      if due then
        cols, rows = dims_of(sb)
      end
      if cols then
        local result = render.render {
          cols = cols,
          rows = rows,
          items = items,
          theme = resolved,
          cfg = cfg,
          glyphs = cfg.glyphs,
          hover = is_active and session.hover[wid] or nil,
          drag = is_active and session.drag[wid] or nil,
          scroll = session.scroll[wid] or 0,
          ensure_visible = not session.user_scrolled[wid] and active_tab_id or nil,
          focus_index = is_active and focus_index or nil,
          footer = footer,
        }
        if is_active then
          session.scroll[wid] = result.scroll
        end
        session.hits[pid] = result.hits
        session.dims[pid] = { cols = cols, rows = rows }
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

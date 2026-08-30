local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local model = require "vtabs.model"
local render = require "vtabs.render"
local theme = require "vtabs.theme"
local icons = require "vtabs.icons"
local util = require "vtabs.util"

local M = {}

local themes = {}

function M.invalidate_theme(window_id)
  themes[window_id] = nil
end

local function theme_for(gui_window, cfg)
  local wid = gui_window:window_id()
  if not themes[wid] then
    local ok, effective = pcall(function()
      return gui_window:effective_config()
    end)
    local palette = ok and effective and effective.resolved_palette or {}
    themes[wid] = theme.resolve(cfg.theme, palette)
  end
  return themes[wid]
end

local function glyphs(cfg)
  return {
    close = icons.get("close", cfg.icon_map),
    new_tab = icons.get("new_tab", cfg.icon_map),
    unseen = icons.get("unseen", cfg.icon_map),
  }
end

local function dims_of(pane, cfg)
  local ok, d = pcall(function()
    return pane:get_dimensions()
  end)
  if ok and d and d.cols and d.viewport_rows then
    return d.cols, d.viewport_rows
  end
  return cfg.width, 24
end

local function copy_items(items)
  local out = {}
  for i, item in ipairs(items) do
    local c = {}
    for k, v in pairs(item) do
      c[k] = v
    end
    out[i] = c
  end
  return out
end

local function footer_for(cfg, mux_win)
  if not cfg.hooks.footer then
    return nil
  end
  local ok, rows = pcall(cfg.hooks.footer, mux_win)
  return ok and rows or nil
end

---Re-renders every sidebar in the window; only changed frames are sent.
function M.sync(gui_window, opts)
  opts = opts or {}
  local cfg = config.get()
  local mux_win = gui_window:mux_window()
  local wid = gui_window:window_id()
  local session = state.session
  local items = model.build(gui_window)
  local resolved = theme_for(gui_window, cfg)
  local glyph = glyphs(cfg)
  local footer = footer_for(cfg, mux_win)
  local active_tab = mux_win:active_tab()
  local active_tab_id = active_tab and active_tab:tab_id() or nil
  local focus_index = state.has_focus(wid) and (session.focus_index or {})[wid] or nil

  for _, info in ipairs(mux_win:tabs_with_info()) do
    local sb = sidebar.find(info.tab)
    if sb then
      local pid = sb:pane_id()
      local cols, rows = dims_of(sb, cfg)
      local is_active_sidebar = info.tab:tab_id() == active_tab_id
      local result = render.render {
        cols = cols,
        rows = rows,
        items = copy_items(items),
        theme = resolved,
        cfg = cfg,
        icons = glyph,
        hover = is_active_sidebar and session.hover[wid] or nil,
        drag = is_active_sidebar and session.drag[wid] or nil,
        scroll = session.scroll[wid] or 0,
        ensure_visible = not (session.user_scrolled and session.user_scrolled[wid]) and active_tab_id or nil,
        focus_index = is_active_sidebar and focus_index or nil,
        footer = footer,
      }
      if cfg.debug then
        local rows_desc = {}
        for row = 1, math.min(rows, 6) do
          rows_desc[#rows_desc + 1] = row .. "=" .. result.hits[row].kind .. ":" .. tostring(result.hits[row].tab_id)
        end
        util.log("sync pane %d items=%d %s", pid, #items, table.concat(rows_desc, " "))
      end
      session.scroll[wid] = result.scroll
      session.hits[pid] = result.hits
      session.dims[pid] = { cols = cols, rows = rows }
      if opts.force or session.frames[pid] ~= result.data then
        session.frames[pid] = result.data
        sb:send_text(wezterm.json_encode { t = "frame", data = result.data } .. "\n")
      end
    end
  end
end

return M

local wezterm = require "wezterm" ---@type Wezterm

local M = {}

local function to_rgb(color)
  if type(color) == "table" then
    return color
  end
  local ok, parsed = pcall(wezterm.color.parse, color)
  if not ok or not parsed then
    return nil
  end
  local r, g, b = parsed:srgba_u8()
  return { r, g, b }
end

local function shade(color, amount)
  local ok, parsed = pcall(wezterm.color.parse, color)
  if not ok or not parsed then
    return nil
  end
  local adjusted = amount < 0 and parsed:darken(-amount) or parsed:lighten(amount)
  local r, g, b = adjusted:srgba_u8()
  return { r, g, b }
end

local function first(...)
  for i = 1, select("#", ...) do
    local v = select(i, ...)
    if v ~= nil then
      local rgb = to_rgb(v)
      if rgb then
        return rgb
      end
    end
  end
  return nil
end

---Resolves the user theme against the window's color palette into rgb triples.
function M.resolve(user, palette)
  user = user or {}
  palette = palette or {}
  local tb = palette.tab_bar or {}
  local active = tb.active_tab or {}
  local hover = tb.inactive_tab_hover or {}
  local ansi = palette.ansi or {}
  local brights = palette.brights or {}
  local bg = first(user.bg, tb.background, shade(palette.background or "#1e1e2e", -0.15), "#181825")
  local fg = first(user.fg, palette.foreground, "#cdd6f4")
  local accent = first(user.accent, ansi[5], "#89b4fa")

  local t = {
    bg = bg,
    fg = fg,
    dim = first(user.dim, brights[1], "#6c7086"),
    accent = accent,
    active_bg = first(user.active_bg, active.bg_color, shade(palette.background or "#1e1e2e", 0.12), "#313244"),
    active_fg = first(user.active_fg, active.fg_color, fg),
    hover_bg = first(user.hover_bg, hover.bg_color, shade(palette.background or "#1e1e2e", 0.06), "#262637"),
    hover_fg = first(user.hover_fg, hover.fg_color, fg),
    pinned_fg = first(user.pinned_fg, fg),
    separator = first(user.separator, brights[1], "#45475a"),
    new_tab_fg = first(user.new_tab_fg, brights[1], "#6c7086"),
    close_fg = first(user.close_fg, brights[1], "#6c7086"),
    close_hover_fg = first(user.close_hover_fg, ansi[2], "#f38ba8"),
    unseen_fg = first(user.unseen_fg, ansi[4], "#f9e2af"),
    private_accent = first(user.private_accent, ansi[6], "#cba6f7"),
    drag_bg = first(user.drag_bg, ansi[5], accent),
    drag_fg = first(user.drag_fg, bg),
    focus_bg = first(user.focus_bg, brights[1], "#45475a"),
  }
  return t
end

return M

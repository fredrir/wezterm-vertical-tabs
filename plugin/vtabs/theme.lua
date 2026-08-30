local wezterm = require "wezterm" ---@type Wezterm
local util = require "vtabs.util"

local M = {}

local function parse(color)
  if type(color) == "table" then
    return color
  end
  local parsed = util.try(wezterm.color.parse, color)
  if not parsed then
    return nil
  end
  local r, g, b = parsed:srgba_u8()
  return { r, g, b }
end

local function first(...)
  for i = 1, select("#", ...) do
    local rgb = parse((select(i, ...)))
    if rgb then
      return rgb
    end
  end
  return nil
end

---Mixes `a` toward `b` by `t`; direction-correct for both light and dark schemes.
local function mix(a, b, t)
  local out = {}
  for i = 1, 3 do
    out[i] = math.floor(a[i] + (b[i] - a[i]) * t + 0.5)
  end
  return out
end

local function luminance(rgb)
  local function channel(c)
    c = c / 255
    if c <= 0.03928 then
      return c / 12.92
    end
    return ((c + 0.055) / 1.055) ^ 2.4
  end
  return 0.2126 * channel(rgb[1]) + 0.7152 * channel(rgb[2]) + 0.0722 * channel(rgb[3])
end

function M.contrast(a, b)
  local la, lb = luminance(a), luminance(b)
  local hi, lo = math.max(la, lb), math.min(la, lb)
  return (hi + 0.05) / (lo + 0.05)
end

---Pushes `fg` toward `target` until it reaches `min` contrast against `bg`.
local function ensure_contrast(fg, bg, target, min)
  local out = fg
  local t = 0
  while M.contrast(out, bg) < min and t < 1 do
    t = t + 0.1
    out = mix(fg, target, t)
  end
  return out
end

local MIN_ACCENT_CONTRAST = 3.0

local function scheme_accent(palette, bg)
  local cursor = first(palette.cursor_bg)
  if cursor and M.contrast(cursor, bg) >= MIN_ACCENT_CONTRAST then
    return cursor
  end
  return nil
end

local function monotone(bg, hover, active, fg)
  local lb, lh, la = luminance(bg), luminance(hover), luminance(active)
  local toward_fg = luminance(fg) > lb
  local ok = toward_fg and (lb < lh and lh < la) or (not toward_fg and (lb > lh and lh > la))
  return ok
end

---Resolves the user theme against the window's color palette into rgb triples.
function M.resolve(user, palette)
  user = user or {}
  palette = palette or {}
  local ansi = palette.ansi or {}
  local brights = palette.brights or {}
  local base_bg = first(palette.background, "#1e1e2e")
  local fg = first(user.fg, palette.foreground, "#cdd6f4")

  local elevation = tonumber(user.elevation) or 0
  local bg = elevation > 0 and mix(base_bg, fg, elevation) or base_bg
  local hover_bg = mix(bg, fg, 0.08)
  local active_bg = mix(bg, fg, 0.16)

  local tb = palette.tab_bar
  local use_tab_bar = user.use_scheme_tab_bar
  if use_tab_bar == nil then
    use_tab_bar = "auto"
  end
  if tb and use_tab_bar ~= false then
    local sbg = first(tb.background)
    local shover = first((tb.inactive_tab_hover or {}).bg_color)
    local sactive = first((tb.active_tab or {}).bg_color)
    if sbg and shover and sactive and (use_tab_bar == true or monotone(sbg, shover, sactive, fg)) then
      bg, hover_bg, active_bg = sbg, shover, sactive
    end
  end
  bg = first(user.bg) or bg
  hover_bg = first(user.hover_bg) or hover_bg
  active_bg = first(user.active_bg) or active_bg

  local dim = first(user.dim) or ensure_contrast(mix(fg, bg, 0.45), bg, fg, 3.0)
  local accent = first(user.accent) or scheme_accent(palette, bg) or first(ansi[5], "#89b4fa")

  return {
    bg = bg,
    fg = fg,
    dim = dim,
    accent = accent,
    active_bg = active_bg,
    active_fg = first(user.active_fg) or fg,
    hover_bg = hover_bg,
    hover_fg = first(user.hover_fg) or fg,
    focus_bg = first(user.focus_bg) or mix(bg, accent, 0.25),
    pinned_fg = first(user.pinned_fg) or dim,
    separator = first(user.separator) or mix(bg, fg, 0.18),
    new_tab_fg = first(user.new_tab_fg) or dim,
    close_fg = first(user.close_fg) or dim,
    close_hover_fg = first(user.close_hover_fg, ansi[2], "#f38ba8"),
    unseen_fg = first(user.unseen_fg, ansi[4], "#f9e2af"),
    private_accent = first(user.private_accent, ansi[6], "#cba6f7"),
    drag_bg = first(user.drag_bg) or mix(bg, accent, 0.35),
    drag_fg = first(user.drag_fg) or fg,
    scroll_fg = first(user.scroll_fg) or mix(bg, fg, 0.3),
    _ = brights,
  }
end

return M

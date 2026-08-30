local wezterm = require "wezterm" ---@type Wezterm
local util = require "vtabs.util"

local M = {}

local ACCENT_MIN = 3.0
local ACCENT_VS_FG_MIN = 1.2
local QUIET_TITLE_MIN = 5.0
-- Matches the shipped `theme.elevation`; a caller passing `{}` gets the page the plugin paints.
local DEFAULT_ELEVATION = 0.06

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
function M.mix(a, b, t)
  local out = {}
  for i = 1, 3 do
    out[i] = math.floor(a[i] + (b[i] - a[i]) * t + 0.5)
  end
  return out
end

function M.luminance(rgb)
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
  local la, lb = M.luminance(a), M.luminance(b)
  local hi, lo = math.max(la, lb), math.min(la, lb)
  return (hi + 0.05) / (lo + 0.05)
end

local mix, luminance = M.mix, M.luminance

---Pushes `fg` toward `target` until it clears `min` against `ref`; never mixes past `target`.
local function ensure_contrast(fg, ref, target, min)
  min = math.min(min, M.contrast(target, ref))
  local out = fg
  for i = 1, 10 do
    if M.contrast(out, ref) >= min then
      return out
    end
    out = mix(fg, target, i / 10)
  end
  return out
end

---The popover surface, lowered in 0.01 steps until its text is no harder to read than the body.
local function raise(bg, fg, lift)
  local ceiling = math.min(4.5, 0.95 * M.contrast(fg, bg))
  local out = lift(0.09)
  for step = 9, 1, -1 do
    out = lift(step / 100)
    if M.contrast(fg, out) >= ceiling then
      return out
    end
  end
  return out
end

---Smallest fade that pushes the scrimmed layer under 2.6 against the page; a target, not a constant.
local function scrim_for(bg, fg)
  local pct = 30
  while pct < 70 and M.contrast(mix(fg, bg, pct / 100), bg) > 2.6 do
    pct = pct + 5
  end
  return pct / 100
end

local WHITE, BLACK = { 255, 255, 255 }, { 0, 0, 0 }

local function accent_candidate(color, bg, fg)
  local rgb = first(color)
  if rgb and M.contrast(rgb, bg) >= ACCENT_MIN and M.contrast(rgb, fg) >= ACCENT_VS_FG_MIN then
    return rgb
  end
  return nil
end

---The page colour as a hex string, from a config table: what `colors.split` must match to vanish.
function M.page(config)
  local colors = config and config.colors or {}
  if type(colors.background) == "string" then
    return colors.background
  end
  return "#1e1e2e"
end

---Resolves the user theme against the window's palette into rgb triples; `opts.private` recolours.
function M.resolve(user, palette, opts)
  user = user or {}
  palette = palette or {}
  opts = opts or {}
  local ansi = palette.ansi or {}
  if user.use_scheme_tab_bar ~= nil then
    util.warn_once("use_scheme_tab_bar", "theme.use_scheme_tab_bar is deprecated and ignored")
  end

  local base_bg = first(palette.background, "#1e1e2e")
  local fg = first(user.fg, palette.foreground, "#cdd6f4")
  local bg = first(user.bg) or mix(base_bg, fg, tonumber(user.elevation) or DEFAULT_ELEVATION)

  -- A 6% darken on a light scheme reads far louder than a 6% lighten on near-black.
  local k = luminance(bg) < 0.5 and 1.0 or 0.6
  local function lift(t)
    return mix(bg, fg, t * k)
  end

  local private_accent = first(user.private_accent, ansi[6], "#cba6f7")
  local accent = first(user.accent)
    or accent_candidate(palette.cursor_bg, bg, fg)
    or accent_candidate((palette.tab_bar and palette.tab_bar.active_tab or {}).bg_color, bg, fg)
    or first(ansi[5], "#89b4fa")
  if opts.private then
    accent = private_accent
  end
  accent = ensure_contrast(accent, bg, fg, ACCENT_MIN)

  local hover_bg = first(user.hover_bg) or lift(0.06)
  local active_bg = first(user.active_bg) or mix(lift(0.12), accent, 0.12)
  local meta_fg = first(user.meta_fg) or ensure_contrast(mix(fg, bg, 0.48), active_bg, fg, 3.5)
  local title_active = first(user.title_active, user.active_title_fg) or ensure_contrast(accent, active_bg, fg, 4.5)
  local raised = first(user.surface_raised) or raise(bg, fg, lift)
  local scrim = tonumber(user.scrim) or scrim_for(bg, fg)
  local scroll_fg = first(user.scroll_fg) or ensure_contrast(lift(0.22), bg, fg, 2.0)
  local border = first(user.border) or ensure_contrast(lift(0.18), bg, fg, 2.5)
  local border_idle = first(user.border_idle) or ensure_contrast(lift(0.14), bg, fg, 2.0)
  local unseen = first(ansi[4])

  -- A saturated fill is the one selection construction that clears 4.5 on every scheme; the ink is
  -- pure black or white, a deliberate exception to the no-absolutes rule, confined to that one row.
  local sel_bg = first(user.popover_sel_bg) or accent
  local sel_fg = first(user.popover_sel_fg)
    or (M.contrast(WHITE, sel_bg) >= M.contrast(BLACK, sel_bg) and WHITE or BLACK)

  return {
    bg = bg,
    fg = fg,
    dim = first(user.dim) or meta_fg,
    accent = accent,
    title_idle = first(user.title_idle) or (M.contrast(fg, bg) >= QUIET_TITLE_MIN and mix(fg, bg, 0.12) or fg),
    meta_fg = meta_fg,
    active_bg = active_bg,
    active_fg = first(user.active_fg) or fg,
    hover_bg = hover_bg,
    hover_fg = first(user.hover_fg) or fg,
    focus_bg = first(user.focus_bg) or mix(bg, accent, 0.25),
    pinned_fg = first(user.pinned_fg) or meta_fg,
    separator = first(user.separator) or lift(0.10),
    border = border,
    border_idle = border_idle,
    -- a luminance-only step off border_idle is invisible on a 1-cell stroke, so the hover takes hue
    ghost_border_hover = first(user.ghost_border_hover) or ensure_contrast(mix(border, accent, 0.5), bg, accent, 2.8),
    new_tab_fg = first(user.new_tab_fg) or mix(fg, bg, 0.30),
    close_fg = first(user.close_fg) or ensure_contrast(mix(fg, bg, 0.55), active_bg, fg, 3.0),
    close_hover_fg = ensure_contrast(first(user.close_hover_fg, ansi[2], "#f38ba8"), active_bg, fg, 3.0),
    unseen_fg = first(user.unseen_fg) or (unseen and M.contrast(unseen, bg) >= ACCENT_MIN and unseen) or accent,
    private_accent = private_accent,
    drag_bg = first(user.drag_bg) or mix(bg, accent, 0.35),
    drag_fg = first(user.drag_fg) or fg,
    scroll_fg = scroll_fg,
    scroll_idle_fg = first(user.scroll_idle_fg) or mix(scroll_fg, bg, 0.55),
    title_active = title_active,
    active_title_fg = title_active,
    -- Render draws the accent bar only when the tinted title is not distinct enough on its own.
    title_active_contrast = M.contrast(title_active, active_bg),
    content_bg = base_bg,
    surface_raised = raised,
    scrim = scrim,
    disabled_fg = first(user.disabled_fg) or mix(meta_fg, raised, 0.45),
    popover_sel_bg = sel_bg,
    popover_sel_fg = sel_fg,
    popover_sel_hint = first(user.popover_sel_hint) or ensure_contrast(mix(sel_fg, sel_bg, 0.40), sel_bg, sel_fg, 3.0),
  }
end

return M

local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local backend = require "vtabs.backend"
local sidebar = require "vtabs.sidebar"
local theme = require "vtabs.theme"
local platform = require "vtabs.platform"
local util = require "vtabs.util"

local M = {}

-- One render is ~90 ms at retina size and `window-resized` fires per frame of a drag, so the gate is
-- a rate limit as well as a debounce: a burst gets one image, the drag's end gets the next.
local MIN_GAP_MS = 250
local MAX_SIDE = 16384
local MAX_PIXELS = 8192 * 8192

local current = {}
local last_at = {}

function M.enabled(cfg)
  return cfg.frame == "zen" or (type(cfg.frame) == "table" and cfg.frame.zen == true)
end

local function options(cfg)
  return type(cfg.frame) == "table" and cfg.frame or {}
end

function M.margin(cfg)
  return math.max(math.floor(tonumber(options(cfg).margin) or 8), 0)
end

---Air inside the card, between the border and the first cell. An inner gap larger than the outer
---frame reads inside-out, so the margin is the ceiling.
function M.inset(cfg)
  local want = math.floor(tonumber(options(cfg).inset) or 6)
  return math.min(math.max(want, 0), M.margin(cfg))
end

---The renderer runs on *this* machine. Only the plain string form of `backend.path` can name it,
---and only where that string is meant for a local domain: the table and function forms are keyed by
---remote host or domain, so `resolve_path` would hand a local execve an ssh host's path.
---`spawn_args` is worse still -- it falls back to the bootstrap shell script.
function M.renderer(cfg)
  local path = cfg.backend.path
  if type(path) == "string" then
    if backend.is_local("local", nil) then
      return path
    end
    return nil
  end
  if path ~= nil then
    util.warn_once("frame-path", 'frame = "zen" needs backend.path as a plain local string')
    return nil
  end
  local root = backend.root
  if type(root) ~= "string" then
    return nil
  end
  return root .. (platform.is_windows and "\\bin\\wez-vtabs.exe" or "/bin/wez-vtabs")
end

---Cells across the whole tab. A cell *count* from the mux is safe -- it is pixel rectangles that
---must never size a local allocation -- and it is only ever used as a divisor here.
local function tab_cols(gui_window)
  local tab = util.active_tab(gui_window)
  local infos = tab and util.try(function()
    return tab:panes_with_info()
  end)
  if type(infos) ~= "table" or #infos == 0 then
    return nil
  end
  local cols = 0
  for _, info in ipairs(infos) do
    if info.is_zoomed then
      return nil
    end
    cols = math.max(cols, (info.left or 0) + (info.width or 0))
  end
  return cols > 0 and cols or nil
end

local function whole(value)
  local n = tonumber(value)
  return n ~= nil and n == n and n ~= math.huge and n ~= -math.huge and math.floor(n) or nil
end

---The card the terminal appears to sit on, in device pixels.
---Everything with a pixel in it comes from the GUI's own measurement of its own window, and the
---sidebar's share comes from our own width invariant. The mux contributes one cell count, as a
---divisor. A pane's pixel rect is mux-reported and never sizes anything here.
function M.rect(gui_window, cfg)
  local dims = util.try(function()
    return gui_window:get_dimensions()
  end)
  if type(dims) ~= "table" then
    return nil
  end
  local w, h = whole(dims.pixel_width), whole(dims.pixel_height)
  if not w or not h or w < 1 or h < 1 or w > MAX_SIDE or h > MAX_SIDE or w * h > MAX_PIXELS then
    return nil
  end
  local cols = tab_cols(gui_window)
  if not cols then
    return nil
  end
  local m = M.margin(cfg)
  local inset = M.inset(cfg)
  -- The grid is what `window_padding` leaves, and the padding owner writes `margin + inset`.
  local grid = w - (m + inset) * 2
  if grid < 1 then
    return nil
  end
  local cell_w = grid / cols
  local sidebar_cols = require("vtabs.geometry").desired(gui_window:window_id())
  local tab = util.active_tab(gui_window)
  if not tab or not sidebar.find(tab) then
    sidebar_cols = 0
  end
  -- One column of divider between the two panes, which the frame tint then covers.
  local taken = math.floor(math.max(sidebar_cols, 0) * cell_w + (sidebar_cols > 0 and cell_w or 0))
  local right = cfg.position == "right"
  -- The card is the pane's text rect grown outward by `inset`, so the outer gutter stays `margin`
  -- on every side while the first glyph is lifted off the stroke.
  local x = right and m or m + taken
  local cw = grid - taken + inset * 2
  local ch = h - m * 2
  if cw < 1 or ch < 1 or x + cw > w or m + ch > h then
    return nil
  end
  return {
    w = w,
    h = h,
    x = x,
    y = m,
    cw = math.floor(cw),
    ch = math.floor(ch),
    radius = math.max(whole(options(cfg).radius) or 8, 0),
  }
end

local function hex(rgb)
  return string.format("#%02x%02x%02x", rgb[1], rgb[2], rgb[3])
end

---Frame tint, card colour and border, from the same palette the sidebar paints itself from.
function M.colours(gui_window, cfg)
  local effective = util.try(function()
    return gui_window:effective_config()
  end) or {}
  local resolved = theme.resolve(cfg.theme, effective.resolved_palette or {}, {
    private = state.is_private(gui_window:window_id()),
  })
  local want = options(cfg).border
  local border = nil
  if want ~= false then
    border = type(want) == "string" and want or hex(resolved.border_idle)
  end
  return { fill = hex(resolved.bg), card = hex(resolved.content_bg), border = border }
end

local function key_of(rect)
  return table.concat({ rect.w, rect.h, rect.x, rect.y, rect.cw, rect.ch, rect.radius }, "x")
end

local function dir_for()
  for _, name in ipairs { "XDG_RUNTIME_DIR", "TMPDIR" } do
    local value = os.getenv(name)
    if type(value) == "string" and value ~= "" then
      return value .. "/wez-vtabs"
    end
  end
  return "/tmp/wez-vtabs"
end

local function fnv1a(s)
  local hash = 2166136261
  for i = 1, #s do
    hash = (hash ~ s:byte(i)) * 16777619 % 4294967296
  end
  return string.format("%08x", hash)
end

local process_key = nil

---Window ids restart per GUI process, so two wezterms both holding window 0 at one size would
---otherwise race for the same file under a shared `/tmp`.
local function process_id()
  if not process_key then
    local pid = util.try(function()
      return wezterm.procinfo.pid()
    end)
    process_key = tostring(tonumber(pid) and math.floor(pid) or util.random_token():sub(1, 8))
  end
  return process_key
end

function M.path_for(window_id, rect, paint)
  -- The geometry and the colours are both in the name: wezterm's path+mtime cache can never serve a
  -- stale image, and neither can our own `readable()` reuse after a theme change.
  paint = paint or {}
  local tint = fnv1a(table.concat({ paint.fill or "", paint.card or "", paint.border or "" }, "|"))
  return string.format("%s/frame-%s-%d-%s-%s.png", dir_for(), process_id(), window_id, key_of(rect), tint)
end

---A background of the user's own is a deliberate choice, and transparency composites *through* the
---frame rather than over it. Either one declines the frame with one warning, never a silent change.
function M.refuses(gui_window)
  local effective = util.try(function()
    return gui_window:effective_config()
  end) or {}
  -- `background` is a Vec in wezterm's config, so an unset one arrives as an empty table, not nil.
  -- The only layer the frame tolerates is the exact file it wrote itself.
  local theirs = effective.background
  if type(theirs) == "table" and #theirs > 0 then
    local first = theirs[1] or {}
    local file = type(first.source) == "table" and first.source.File or nil
    -- `install` writes exactly one layer, so a second is theirs however ours got to the front.
    if #theirs > 1 or file == nil or file ~= current[gui_window:window_id()] then
      return "background"
    end
  end
  if effective.window_background_image ~= nil then
    return "window_background_image"
  end
  if tonumber(effective.window_background_opacity or 1) ~= 1 then
    return "window_background_opacity"
  end
  if tonumber(effective.text_background_opacity or 1) ~= 1 then
    return "text_background_opacity"
  end
  return nil
end

---Writes one PNG. Returns its path, or nil once the reason has been logged.
function M.render(gui_window, cfg, rect, paint)
  local exe = M.renderer(cfg)
  if not exe then
    util.warn_once("frame-exe", 'frame = "zen" cannot find a local wez-vtabs to render with')
    return nil
  end
  paint = paint or M.colours(gui_window, cfg)
  local path = M.path_for(gui_window:window_id(), rect, paint)
  local args = {
    exe,
    "frame",
    "--w",
    tostring(rect.w),
    "--h",
    tostring(rect.h),
    "--card",
    tostring(rect.x),
    tostring(rect.y),
    tostring(rect.cw),
    tostring(rect.ch),
    "--radius",
    tostring(rect.radius),
    "--fill",
    paint.fill,
    "--card-fill",
    paint.card,
    "--out",
    path,
  }
  if paint.border then
    args[#args + 1] = "--border"
    args[#args + 1] = paint.border
  end
  -- Asynchronous: `run_child_process` blocks the GUI thread, and this runs on a resize.
  local spawn = wezterm.background_child_process or wezterm.run_child_process
  if util.try(spawn, args) == false then
    util.warn_once("frame-render", "frame renderer failed; the window background is left alone")
    return nil
  end
  return path
end

local function readable(path)
  local f = io.open(path, "rb")
  if not f then
    return false
  end
  f:close()
  return true
end

---Writes back only the keys the frame owns. `window_padding` is not one of them: `apply_padding`
---composes `margin + inset` into the base config and `apply_titlebar_band` holds the override, so a
---write from here would reset the band's `top` on the next resize and put the traffic lights back
---on the user's shell.
function M.install(gui_window, path, cfg, paint)
  local overrides = util.try(function()
    return gui_window:get_config_overrides()
  end) or {}
  local merged = {}
  for key, value in pairs(overrides) do
    merged[key] = value
  end
  merged.background = {
    {
      source = { File = path },
      repeat_x = "NoRepeat",
      repeat_y = "NoRepeat",
      vertical_align = "Top",
      horizontal_align = "Left",
      width = "100%",
      height = "100%",
    },
  }
  local colors = {}
  for key, value in pairs(merged.colors or {}) do
    colors[key] = value
  end
  -- `split` is window-global, so it also draws inside the content pane: the card colour makes the
  -- user's own splits vanish into the card, and the frame margin already hides the sidebar seam.
  colors.split = (paint or M.colours(gui_window, cfg)).card
  merged.colors = colors
  state.session.applying[gui_window:window_id()] = util.now_ms()
  util.try(function()
    gui_window:set_config_overrides(merged)
  end)
end

local function forget(window_id)
  if current[window_id] then
    os.remove(current[window_id])
  end
  current[window_id] = nil
  last_at[window_id] = nil
end

M.forget_window = forget
table.insert(state.forget_hooks, forget)

---Regenerates the frame when the card has moved; cheap and idempotent when it has not.
---@return boolean true when a new image was installed
function M.sync(gui_window)
  local cfg = config.get()
  if not M.enabled(cfg) then
    return false
  end
  local clash = M.refuses(gui_window)
  if clash then
    util.warn_once("frame-clash", 'frame = "zen" declines: you set %s', clash)
    return false
  end
  local rect = M.rect(gui_window, cfg)
  if not rect then
    return false
  end
  local wid = gui_window:window_id()
  -- Resolved once and handed down, so a poll costs one `effective_config` read rather than three.
  local paint = M.colours(gui_window, cfg)
  local path = M.path_for(wid, rect, paint)
  if current[wid] == path then
    return false
  end
  -- The render is asynchronous, so the image is installed on the poll that finds it on disk. That
  -- also makes the rate limit safe: a burst asks once and the last rect wins.
  if readable(path) then
    local previous = current[wid]
    current[wid] = path
    M.install(gui_window, path, cfg, paint)
    if previous and previous ~= path then
      os.remove(previous)
    end
    return true
  end
  local now = util.now_ms()
  if last_at[wid] and now - last_at[wid] < MIN_GAP_MS then
    return false
  end
  last_at[wid] = now
  M.render(gui_window, cfg, rect, paint)
  return false
end

return M

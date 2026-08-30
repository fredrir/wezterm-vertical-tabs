local util = require "vtabs.util"
local icons = require "vtabs.icons"

local M = {}

M.defaults = {
  width = 28,
  position = "left", -- "left" | "right"
  hide_native_tab_bar = true,
  poll_ms = 500,
  padding = { top = 0, left = 1, right = 1 }, -- the top strip owns the rows above the first card
  row_gap = 1,
  tab_height = "card", -- "card": 2 painted rows | "row": 1 row, same as meta = false
  meta = "auto", -- "auto" | "cwd" | "process" | false
  new_tab_button = "ghost", -- "ghost" | "row" | false
  new_tab_label = "New tab",
  close_button = "hover", -- "hover" | "always" | "never"
  corners = "chamfer", -- "chamfer" | "square"
  titlebar = "auto", -- "auto" | "integrate" | "plain"
  toggle_button = true,
  confirm_close = true,
  debug = false,
  show_index = false,
  pinned_style = "dense", -- "dense" | "compact" | "full"
  separator = "gap", -- "rule" | "gap" | "none"
  scroll_indicator = "auto", -- "auto" | "always" | "never"
  wheel = "scroll", -- "scroll" | "switch"
  tear_off = true, -- drop on the sidebar's inner edge to move the tab to a new window
  hover = "follow", -- "follow": sidebar holds focus while hovered | "press": only press → release
  window_title = true, -- title the window after the content pane while the sidebar is active
  hover_timeout_ms = 6000,
  double_click_ms = 400,
  ellipsis = "…",
  icons = true,
  icon_map = {},
  title = nil, -- fun(tab: MuxTab, pane: Pane): string
  domain = "CurrentPaneDomain",
  adopt = "auto", -- "auto": take over a backend pane where we spawn them | true: any domain | false: never
  skip_close_confirmation = true,
  private = {
    env = { HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" },
  },
  keys = {},
  theme = {
    elevation = 0, -- mix the sidebar bg toward fg; 0.06 is the pre-P1 raised look
  },
  hooks = {
    filter = nil, -- fun(tab: MuxTab, window: MuxWindow): boolean
    footer = nil, -- fun(window: MuxWindow): (string|FooterEntry)[]
    theme = nil, -- fun(window: Window, theme: table): table
    route = nil, -- reserved for Spaces: fun(meta: TabMeta): string|nil
  },
  backend = {
    path = nil,
    repo = "fredrir/wezterm-vertical-tabs",
    version = nil,
    build = true,
    uservar = "vtabs",
  },
}

local VALID = {
  position = { left = true, right = true },
  close_button = { hover = true, always = true, never = true },
  pinned_style = { dense = true, compact = true, full = true },
  separator = { rule = true, gap = true, none = true },
  wheel = { scroll = true, switch = true },
  hover = { follow = true, press = true },
  adopt = { auto = true, [true] = true, [false] = true },
  tab_height = { card = true, row = true },
  meta = { auto = true, cwd = true, process = true, [false] = true },
  new_tab_button = { ghost = true, row = true, [false] = true },
  corners = { chamfer = true, square = true },
  scroll_indicator = { auto = true, always = true, never = true },
  titlebar = { auto = true, integrate = true, plain = true },
}

---Booleans users already write, normalised before the VALID lookup so they never warn.
local ALIAS = {
  new_tab_button = { [true] = "ghost" },
  scroll_indicator = { [true] = "auto", [false] = "never" },
  meta = { [true] = "auto" },
  tab_height = { [2] = "card", [1] = "row", [true] = "card", [false] = "row" },
}

local current = nil

function M.setup(opts)
  local cfg = util.merge(M.defaults, opts or {})
  for key, aliases in pairs(ALIAS) do
    local mapped = aliases[cfg[key]]
    if mapped ~= nil then
      cfg[key] = mapped
    end
  end
  for key, allowed in pairs(VALID) do
    if not allowed[cfg[key]] then
      util.warn("invalid %s=%s, using default", key, tostring(cfg[key]))
      cfg[key] = M.defaults[key]
    end
  end
  if type(cfg.width) ~= "number" or cfg.width < 8 then
    util.warn "width must be >= 8, using default"
    cfg.width = M.defaults.width
  end
  if type(cfg.row_gap) ~= "number" or cfg.row_gap < 0 then
    util.warn "row_gap must be a number >= 0, using default"
    cfg.row_gap = M.defaults.row_gap
  end
  if type(cfg.toggle_button) ~= "boolean" then
    util.warn "toggle_button must be a boolean, using default"
    cfg.toggle_button = M.defaults.toggle_button
  end
  if cfg.tab_height == "row" then
    cfg.meta = false
  elseif cfg.meta == false then
    cfg.tab_height = "row"
  end
  if cfg.hover == "press" and cfg.close_button == "hover" then
    cfg.close_button = "always"
  end
  if cfg.tear_off == "edge" then
    cfg.tear_off = true
  elseif cfg.tear_off == "outside" then
    util.warn 'tear_off="outside" is not supported, using edge'
    cfg.tear_off = true
  end
  cfg.glyphs = icons.resolve(cfg.icon_map)
  current = cfg
  return cfg
end

function M.get()
  return current or M.setup {}
end

return M

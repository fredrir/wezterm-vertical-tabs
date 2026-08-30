local util = require "vtabs.util"
local icons = require "vtabs.icons"

local M = {}

M.defaults = {
  width = 28,
  position = "left", -- "left" | "right"
  hide_native_tab_bar = true,
  poll_ms = 500,
  padding = { top = 1, left = 1, right = 1 },
  row_gap = 0,
  new_tab_button = true,
  new_tab_label = "New Tab",
  close_button = "hover", -- "hover" | "always" | "never"
  confirm_close = true,
  debug = false,
  show_index = false,
  pinned_style = "compact", -- "compact" (no close button, pin glyph) | "full"
  separator = "rule", -- "rule" | "gap" | "none"
  scroll_indicator = true,
  wheel = "scroll", -- "scroll" | "switch"
  tear_off = true, -- drop on the sidebar's inner edge to move the tab to a new window
  hover = "follow", -- "follow": sidebar holds focus while hovered | "press": only press → release
  hover_timeout_ms = 6000,
  double_click_ms = 400,
  ellipsis = "…",
  icons = true,
  icon_map = {},
  title = nil, -- fun(tab: MuxTab, pane: Pane): string
  domain = "CurrentPaneDomain",
  skip_close_confirmation = true,
  private = {
    env = { HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" },
  },
  keys = {},
  theme = {
    use_scheme_tab_bar = "auto", -- "auto" | true | false
    elevation = 0, -- mix the sidebar bg toward fg; 0.06 is the pre-1.0 raised look
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
  pinned_style = { compact = true, full = true },
  separator = { rule = true, gap = true, none = true },
  wheel = { scroll = true, switch = true },
  hover = { follow = true, press = true },
}

local current = nil

function M.setup(opts)
  local cfg = util.merge(M.defaults, opts or {})
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

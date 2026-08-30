local util = require "vtabs.util"

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
  pinned_style = "compact", -- "compact" | "full"
  wheel = "scroll", -- "scroll" | "switch"
  tear_off = "edge", -- "edge" | "outside" | false
  hover_timeout_ms = 2000,
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
  theme = {},
  hooks = {
    filter = nil, -- fun(tab: MuxTab, window: MuxWindow): boolean
    footer = nil, -- fun(window: MuxWindow): string[]
  },
  backend = {
    path = nil,
    repo = "fredrir/wez-vertical-tabs",
    version = nil,
    build = true,
    uservar = "vtabs",
  },
}

local VALID = {
  position = { left = true, right = true },
  close_button = { hover = true, always = true, never = true },
  pinned_style = { compact = true, full = true },
  wheel = { scroll = true, switch = true },
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
  current = cfg
  return cfg
end

function M.get()
  return current or M.setup {}
end

return M

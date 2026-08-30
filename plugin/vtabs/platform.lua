local wezterm = require "wezterm" ---@type Wezterm

local triple = wezterm.target_triple or ""

local M = {
  triple = triple,
  is_mac = triple:find "apple%-darwin" ~= nil,
  is_windows = triple:find "windows" ~= nil,
}

M.SUPER = M.is_mac and "CMD" or "CTRL|SHIFT"
M.SUPER2 = M.is_mac and "CMD|SHIFT" or "CTRL|SHIFT|ALT"

return M

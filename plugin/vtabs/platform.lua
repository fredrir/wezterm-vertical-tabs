local wezterm = require "wezterm" ---@type Wezterm

local triple = wezterm.target_triple or ""

local M = {
  triple = triple,
  is_mac = triple:find "apple%-darwin" ~= nil,
  is_windows = triple:find "windows" ~= nil,
  is_linux = triple:find "linux" ~= nil,
}

M.SUPER = M.is_mac and "CMD" or "CTRL"
M.SUPER_SHIFT = M.SUPER .. "|SHIFT"

return M

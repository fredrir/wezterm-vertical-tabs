local wezterm = require "wezterm" ---@type Wezterm
local protocol = require "vtabs.gen.protocol"

local triple = wezterm.target_triple or ""

local M = {
  triple = triple,
  is_mac = triple:find "apple%-darwin" ~= nil,
  is_windows = triple:find "windows" ~= nil,
}

M.SUPER = M.is_mac and "CMD" or "CTRL|SHIFT"
M.SUPER2 = M.is_mac and "CMD|SHIFT" or "CTRL|SHIFT|ALT"

-- Rust's geometry owns this value; the generated mirror keeps host padding in lockstep.
M.TITLEBAR_PT = protocol.TITLEBAR_PT
-- `window_padding` takes a bare number as device pixels; the `pt` unit is what scales with the dpi
-- (wezterm config/src/units.rs:81, 166-173).
M.TITLEBAR_PAD = string.format("%dpt", M.TITLEBAR_PT)

return M

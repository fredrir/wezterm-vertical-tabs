local util = require "vtabs.util"

local M = {}

---Chrome glyphs, their ASCII fallbacks and the group they substitute with.
local CHROME = {
  chamfer_top = { "▙", " ", group = "block" },
  chamfer_bottom = { "▛", " ", group = "block" },
  scroll = { "▐", "|", group = "block" },
  active = { "▎", "|", group = "bar" },
  unseen = { "•", "*", group = "marks" },
  focus = { "›", ">", group = "marks" },
  ellipsis = { "…", "...", group = "marks" },
  meta_sep = { "  ", "  ", group = "marks" },
  toggle_left = { "«", "<", group = "toggle" },
  toggle_right = { "»", ">", group = "toggle" },
  frame_tl = { "╭", "+", group = "ghost_frame" },
  frame_tr = { "╮", "+", group = "ghost_frame" },
  frame_bl = { "╰", "+", group = "ghost_frame" },
  frame_br = { "╯", "+", group = "ghost_frame" },
  frame_dash = { "╌", "-", group = "ghost_frame" },
  frame_dash_v = { "╎", "|", group = "ghost_frame" },
  rule = { "─", "-", group = "marks" },
}

---East Asian Ambiguous: width flips with unicode_version / treat_east_asian_ambiguous_width_as_wide.
local AMBIGUOUS = {
  [0x258e] = true,
  [0x2022] = true,
  [0x2026] = true,
  [0x00b7] = true,
  [0x00ab] = true,
  [0x00bb] = true,
  [0x256d] = true,
  [0x256e] = true,
  [0x256f] = true,
  [0x2570] = true,
  [0x2500] = true,
}

local function ambiguous(s)
  if type(s) ~= "string" or utf8.len(s) ~= 1 then
    return false
  end
  return AMBIGUOUS[utf8.codepoint(s)] == true
end

local function group_members(group)
  local keys = {}
  for key, spec in pairs(CHROME) do
    if spec.group == group then
      keys[#keys + 1] = key
    end
  end
  table.sort(keys)
  return keys
end

---Resolves the chrome glyphs for one window against its effective config.
---@param base table icons.resolve output; process icons pass through untouched
---@param effective table|nil window:effective_config(), or nil for the width backstop only
function M.resolve(base, effective)
  base = base or {}
  effective = effective or {}
  local out = {}
  for key, value in pairs(base) do
    out[key] = value
  end
  for key, spec in pairs(CHROME) do
    out[key] = base[key] or spec[1]
  end
  out.corners = "chamfer"

  local by_width = false
  local function fall_back(key)
    local spec = CHROME[key]
    if spec and out[key] ~= spec[2] then
      out[key] = spec[2]
    end
  end
  local function fall_back_group(group)
    for _, key in ipairs(group_members(group)) do
      fall_back(key)
    end
  end

  if effective.custom_block_glyphs == false then
    out.corners = "square"
    fall_back_group "block"
    fall_back_group "bar"
  end

  -- only this flag selects ambiguous width; unicode_version alone never does
  local wide_ambiguous = effective.treat_east_asian_ambiguous_width_as_wide == true
  if wide_ambiguous then
    for key in pairs(CHROME) do
      if ambiguous(out[key]) then
        fall_back(key)
      end
    end
    fall_back_group "ghost_frame"
  end

  for key in pairs(CHROME) do
    if util.width(out[key]) ~= 1 then
      fall_back(key)
      by_width = true
    end
  end
  for _, key in ipairs(group_members "ghost_frame") do
    if out[key] ~= CHROME[key][1] then
      fall_back_group "ghost_frame"
      break
    end
  end
  for _, key in ipairs { "close", "pinned", "private", "new_tab", "settings", "search" } do
    if out[key] and util.width(out[key]) ~= 1 then
      out[key] = ({ close = "x", pinned = "*", private = "~", new_tab = "+", settings = "*", search = "/" })[key]
      by_width = true
    end
  end
  if out.chamfer_top == CHROME.chamfer_top[2] then
    out.corners = "square"
  end

  if by_width then
    util.warn_once("glyph-width", "some glyphs are not one cell wide; using ASCII")
  end
  return out
end

return M

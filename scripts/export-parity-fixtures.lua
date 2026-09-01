-- Exports Lua-side outputs the ported Rust crates must reproduce byte-for-byte.
-- Run from the repo root: lua scripts/export-parity-fixtures.lua
local root = arg[0]:match "^(.*)[/\\]" .. "/.."
package.path = root .. "/plugin/?.lua;" .. root .. "/plugin/tests/?.lua;" .. package.path
package.preload.wezterm = function()
  return require "wezterm_stub"
end

local theme = require "vtabs.theme"
local util = require "vtabs.util"
local icons = require "vtabs.icons"
local platform = require "vtabs.platform"
local palettes = require "palettes"

local function num(v)
  if v == math.floor(v) then
    return string.format("%d", v)
  end
  return string.format("%.17g", v)
end

local function json(v, indent)
  local pad = string.rep("  ", indent)
  if type(v) == "table" then
    if #v > 0 or next(v) == nil then
      local parts = {}
      for _, item in ipairs(v) do
        parts[#parts + 1] = json(item, indent + 1)
      end
      return "[" .. table.concat(parts, ", ") .. "]"
    end
    local keys = {}
    for k in pairs(v) do
      keys[#keys + 1] = k
    end
    table.sort(keys)
    local parts = {}
    for _, k in ipairs(keys) do
      parts[#parts + 1] = pad .. '  "' .. k .. '": ' .. json(v[k], indent + 1)
    end
    return "{\n" .. table.concat(parts, ",\n") .. "\n" .. pad .. "}"
  elseif type(v) == "number" then
    return num(v)
  elseif type(v) == "boolean" then
    return tostring(v)
  end
  return '"' .. tostring(v):gsub("\\", "\\\\"):gsub('"', '\\"') .. '"'
end

local function write(path, cases)
  local f = assert(io.open(root .. "/" .. path, "w"))
  f:write(json(cases, 0), "\n")
  f:close()
  print(("wrote %s (%d cases)"):format(path, #cases))
end

-- theme.resolve
local extra = {
  {
    name = "tab-bar accent, no cursor",
    background = "#101014",
    foreground = "#d0d0d8",
    tab_bar = { active_tab = { bg_color = "#ff8800" } },
    ansi = {},
  },
  { name = "empty palette" },
}
local OVERRIDES = {
  accent = "#ff0000",
  elevation = 0.12,
  hover_bg = "#333333",
  scrim = 0.5,
  popover_sel_fg = "#ffffff",
}
local theme_cases = {}
local function add_theme(palette, label, user, opts)
  theme_cases[#theme_cases + 1] = {
    name = palette.name .. " / " .. label,
    palette = palette,
    user = user,
    private = opts and opts.private or false,
    resolved = theme.resolve(user, palette, opts),
  }
end
for _, list in ipairs { palettes, extra } do
  for _, palette in ipairs(list) do
    add_theme(palette, "default", {}, {})
    add_theme(palette, "private", {}, { private = true })
    add_theme(palette, "overridden", OVERRIDES, {})
  end
end
write("backend/crates/vtabs-theme/tests/fixtures/resolve.json", theme_cases)

-- text/paths fixtures moved to Rust with the helpers; vtabs-view/tests/fixtures/{text,paths}.json are final

-- glyphs.resolve moved to Rust with its fixture committed; vtabs-view/tests/fixtures/glyphs.json is final

-- util.sanitize: inputs as byte arrays, JSON cannot carry the invalid sequences
local RAW = {
  "plain",
  "tab\ttitle",
  "esc\27[31mred",
  string.char(0x9b) .. "csi",
  "bad\xc3utf",
  "over\xc0\xaflong",
  "bidi\226\128\174x",
  "iso\226\129\166ok",
  "nul\0mid",
  "high\xf4\x90\x80\x80plane",
}
local sanitize_cases = {}
for _, s in ipairs(RAW) do
  local bytes = {}
  for i = 1, #s do
    bytes[#bytes + 1] = string.byte(s, i)
  end
  sanitize_cases[#sanitize_cases + 1] = { bytes = bytes, out = util.sanitize(s) }
end
write("backend/crates/vtabs-core/tests/fixtures/sanitize.json", sanitize_cases)

-- platform.strip_geometry
local geom_cases = {}
local DIMS = {
  { name = "retina mac", cols = 120, viewport_rows = 40, pixel_width = 1680, pixel_height = 1120, dpi = 144 },
  { name = "1x mac", cols = 100, viewport_rows = 30, pixel_width = 800, pixel_height = 480, dpi = 72 },
  { name = "linux 96dpi", cols = 120, viewport_rows = 40, pixel_width = 960, pixel_height = 640, dpi = 96 },
  { name = "no dpi", cols = 80, viewport_rows = 24, pixel_width = 640, pixel_height = 384 },
  { name = "zero cols", cols = 0, viewport_rows = 0 },
}
local GOPTS = {
  { name = "mac reserve", is_mac = true, integrated_buttons = true, native_button_style = true, position = "left" },
  {
    name = "mac reserve toggle",
    is_mac = true,
    integrated_buttons = true,
    native_button_style = true,
    position = "left",
    toggle_button = true,
  },
  {
    name = "mac fullscreen",
    is_mac = true,
    integrated_buttons = true,
    native_button_style = true,
    position = "left",
    is_full_screen = true,
    toggle_button = true,
  },
  { name = "plain", toggle_button = true, padding_top = 2 },
  { name = "rail", rail = true, rail_width = 5, toggle_button = true },
  {
    name = "mac rail",
    is_mac = true,
    integrated_buttons = true,
    native_button_style = true,
    position = "left",
    rail = true,
    rail_width = 5,
    toggle_button = true,
  },
  {
    name = "preview",
    is_mac = true,
    integrated_buttons = true,
    native_button_style = true,
    position = "left",
    preview = true,
    card_x1 = 3,
  },
}
for _, d in ipairs(DIMS) do
  for _, o in ipairs(GOPTS) do
    geom_cases[#geom_cases + 1] = {
      name = d.name .. " / " .. o.name,
      dims = d,
      opts = o,
      out = platform.strip_geometry(d, o),
    }
  end
end
write("backend/crates/vtabs-core/tests/fixtures/geom.json", geom_cases)

-- icons.resolve mechanics only: values under the stub are the ASCII fallbacks, never production
local icon_cases = {}
for _, m in ipairs {
  {},
  { nvim = "N", ["^cargo%-"] = "R", ["git.*"] = "G" },
} do
  icon_cases[#icon_cases + 1] = { icon_map = m, resolved = icons.resolve(m) }
end
write("backend/crates/vtabs-core/tests/fixtures/icons_resolve.json", icon_cases)

local M = {}

---Settings-page options that project onto WezTerm's window-global configuration. This table is
---generated from the same Rust descriptors that decide locking and live apply mode.
M.HOST_KEYS = {}
local KEYS, seen_keys = {}, {}
for _, option in ipairs(require "vtabs.gen.schema") do
  if option.host_key then
    M.HOST_KEYS[option.key] = option.host_key
    for _, path in ipairs(option.policy_paths or {}) do
      M.HOST_KEYS[path] = option.host_key
    end
    if not seen_keys[option.host_key] then
      KEYS[#KEYS + 1] = option.host_key
      seen_keys[option.host_key] = true
    end
  end
end

local function copy(value)
  if type(value) ~= "table" then
    return value
  end
  local out = {}
  for key, child in pairs(value) do
    out[key] = copy(child)
  end
  return out
end

---The host's values before the plugin writes any of its own. A non-nil entry means the settings
---page must leave that WezTerm key alone.
function M.capture(host)
  local out = {}
  for _, key in ipairs(KEYS) do
    if key == "colors_split" then
      out[key] = host.colors and host.colors.split or nil
    else
      out[key] = host[key]
    end
  end
  return out
end

---Host keys the user's wezterm.lua already owns, in descriptor order. `array` preserves an empty
---list on the JSON wire without making this module depend on the wire encoder.
function M.owned_keys(host, array)
  local out = (array or function(value)
    return value or {}
  end)()
  for _, key in ipairs(KEYS) do
    if host[key] ~= nil then
      out[#out + 1] = key
    end
  end
  return out
end

local function frame_padding(cfg)
  local frame = require "vtabs.frame"
  if not frame.enabled(cfg) then
    return nil
  end
  local pad = frame.margin(cfg) + frame.inset(cfg)
  return { left = pad, right = pad, top = pad, bottom = pad }
end

---The padding both boot-time config and a live settings edit use. Zen's margin and inset own all
---four sides; otherwise edge-to-edge keeps WezTerm's far-side and optional half-cell bands.
function M.window_padding(cfg)
  local framed = frame_padding(cfg)
  if framed then
    return framed
  end
  if not cfg.edge_to_edge then
    return nil
  end
  local band = cfg.edge_to_edge == "sides" and "0.5cell" or 0
  local padding = { left = 0, right = 0, top = band, bottom = band }
  padding[cfg.position == "left" and "right" or "left"] = "1cell"
  return padding
end

local function page_background(source)
  source = source or {}
  local colors = source.colors or {}
  local palette = source.resolved_palette or {}
  return type(colors.background) == "string" and colors.background
    or (type(palette.background) == "string" and palette.background)
    or "#1e1e2e"
end

local function split(cfg, source)
  local want = cfg.theme and cfg.theme.split
  if want == nil or want == "auto" then
    return nil
  end
  return want == "hidden" and page_background(source) or want
end

---One projection for both entry points. `colors_split` is intentionally the ownership key used by
---config.host_config; `apply_*` translates it to the nested `colors.split` WezTerm field.
function M.project(cfg, source)
  return {
    window_padding = M.window_padding(cfg),
    pane_focus_follows_mouse = cfg.hover == "follow" and true or nil,
    inactive_pane_hsb = cfg.dim_inactive_panes == false and { brightness = 1.0, saturation = 1.0, hue = 1.0 } or nil,
    colors_split = split(cfg, source),
  }
end

---Applies plugin-owned host keys while wezterm.lua is being evaluated. The captured host values
---decide ownership; this function never replaces something the user set first.
function M.apply_boot(host, cfg, captured)
  captured = captured or M.capture(host)
  local projected = M.project(cfg, host)
  for _, key in ipairs { "window_padding", "pane_focus_follows_mouse", "inactive_pane_hsb" } do
    if captured[key] == nil and projected[key] ~= nil then
      host[key] = copy(projected[key])
    end
  end
  if captured.colors_split == nil and projected.colors_split ~= nil then
    host.colors = host.colors or {}
    host.colors.split = projected.colors_split
  end
  return host
end

---Applies the WezTerm-facing part of one settings edit. The whole projection is shared with boot,
---but only the host key affected by this row is written to the window override table. Removing a
---projected value needs a config reload: nil would otherwise expose the plugin value baked into
---the base config at startup.
function M.apply_live(gui_window, cfg, option_key)
  local host_key = M.HOST_KEYS[option_key]
  if not gui_window or not host_key or host_key == "window_decorations" then
    return false
  end
  local effective = gui_window:effective_config() or {}
  local projected = M.project(cfg, effective)
  local overrides = copy(gui_window:get_config_overrides() or {})
  if host_key == "colors_split" then
    local colors = copy(overrides.colors or {})
    colors.split = projected.colors_split
    overrides.colors = next(colors) and colors or nil
  else
    overrides[host_key] = copy(projected[host_key])
  end
  gui_window:set_config_overrides(overrides)
  return projected[host_key] == nil and "reload" or "override"
end

return M

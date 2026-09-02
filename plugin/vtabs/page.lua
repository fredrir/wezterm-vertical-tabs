local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local host_config = require "vtabs.host_config"
local settings = require "vtabs.settings"
local util = require "vtabs.util"

local M = {}

local function copy(value, seen)
  if type(value) ~= "table" then
    return value
  end
  seen = seen or {}
  if seen[value] then
    return seen[value]
  end
  local out = {}
  seen[value] = out
  for key, child in pairs(value) do
    out[key] = copy(child, seen)
  end
  return out
end

local function valid_path(path)
  if type(path) ~= "table" or #path == 0 then
    return false
  end
  for _, part in ipairs(path) do
    if type(part) ~= "string" or part == "" then
      return false
    end
  end
  return true
end

---Applies a segmented path without interpreting dots inside an open-map key.
local function set_path(root, path, value, remove)
  local node = root
  for i = 1, #path - 1 do
    local part = path[i]
    if type(node[part]) ~= "table" then
      node[part] = {}
    end
    node = node[part]
  end
  node[path[#path]] = remove and nil or copy(value)
end

local function valid_change(patch)
  local change = type(patch) == "table" and patch.change or nil
  return valid_path(patch and patch.path) and type(change) == "table" and (change.op == "set" or change.op == "remove")
end

---The only live settings action left in Lua: apply Rust's validated path change to the host-side
---config, write Rust's final JSON body, then project any affected WezTerm-owned setting.
function M.commit_effect(gui_window, ev)
  local change = type(ev) == "table" and ev.change or nil
  if not valid_path(ev and ev.path) or type(change) ~= "table" then
    return false
  end
  local remove = change.op == "remove"
  if not remove and change.op ~= "set" then
    return false
  end
  for _, patch in ipairs(ev.derived or {}) do
    if not valid_change(patch) then
      return false
    end
  end
  local previous_cfg = config.get()
  local next_cfg = copy(previous_cfg)
  set_path(next_cfg, ev.path, change.value, remove)
  for _, patch in ipairs(ev.derived or {}) do
    set_path(next_cfg, patch.path, patch.change.value, patch.change.op == "remove")
  end
  config.replace_canonical(next_cfg)

  -- Persist before touching the GUI: a disappearing window must not lose an accepted edit.
  -- Permission is the pre-commit setting: turning persistence off must persist that final choice.
  -- The destination comes from the new config so changing settings.path takes effect immediately.
  util.try(settings.save_body, next_cfg, ev.persistence_json, previous_cfg)
  local rank = { instant = 1, override = 2, reload = 3 }
  local mode = rank[ev.mode] and ev.mode or "instant"
  local function project(path)
    local live = util.try(host_config.apply_live, gui_window, next_cfg, table.concat(path, "."))
    if rank[live] and rank[live] > rank[mode] then
      mode = live
    end
  end
  project(ev.path)
  for _, patch in ipairs(ev.derived or {}) do
    project(patch.path)
  end
  if mode == "reload" then
    if type(wezterm.reload_configuration) == "function" then
      util.try(wezterm.reload_configuration)
    end
    return "reload"
  end
  return mode
end

M.set_path = set_path

return M

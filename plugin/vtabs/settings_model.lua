local config = require "vtabs.config"
local host_config = require "vtabs.host_config"
local keys = require "vtabs.keys"
local platform = require "vtabs.platform"
local schema = require "vtabs.schema"
local version = require "vtabs.version"

local M = {}

local function copy_path(path, tail)
  local out = {}
  for i, part in ipairs(path) do
    out[i] = part
  end
  if tail ~= nil then
    out[#out + 1] = tail
  end
  return out
end

local function descriptor(path)
  return schema.by_key[table.concat(path, ".")]
end

local function path_id(path)
  local out = {}
  for i, part in ipairs(path) do
    out[i] = #part .. ":" .. part
  end
  return table.concat(out, "/")
end

local function mark_opaque(opaque, path)
  if path and #path > 0 then
    opaque[path_id(path)] = copy_path(path)
  end
end

local function table_shape(value)
  local count, last, strings = 0, 0, 0
  for key in pairs(value) do
    if type(key) == "number" and key >= 1 and key % 1 == 0 then
      count = count + 1
      last = math.max(last, key)
    elseif type(key) == "string" then
      strings = strings + 1
    else
      return "invalid"
    end
  end
  if count == 0 then
    return strings == 0 and "empty" or "map"
  end
  if strings > 0 or count ~= last then
    return "invalid"
  end
  return "list"
end

local function nested_list(path)
  return path[1] == "spaces"
    and tonumber(path[2]) ~= nil
    and path[3] == "match"
    and ({ domain = true, host = true, user = true, proc = true, cwd = true, title = true })[path[4]] == true
end

local function supports_opaque(owner, path)
  owner = owner or path
  local option = descriptor(owner)
  if option then
    if option.type == "function" or option.type == "any" or option.open then
      return true
    end
    -- A custom list entry may carry callbacks, but a function used as the entry itself is invalid.
    if option.type == "list" and #path >= #owner + 2 then
      return true
    end
  end
  return schema.is_open(table.concat(owner, "."))
end

local function mark_loss(opaque, invalid, owner, path)
  local owned = owner or path
  if supports_opaque(owned, path) then
    mark_opaque(opaque, owned)
  elseif descriptor(owned) then
    mark_opaque(invalid, owned)
  end
end

---Copies only JSON-shaped values. A function, userdata, thread, or cycle stays in Lua and locks
---the nearest settings field that contains it; Rust never receives a lossy value as editable.
local function project(value, path, owner, opaque, invalid, array, seen)
  local kind = type(value)
  if kind == "nil" then
    return nil, false
  end
  if kind == "number" and (value ~= value or value == math.huge or value == -math.huge) then
    mark_loss(opaque, invalid, owner, path)
    return nil, false
  end
  if kind == "string" or kind == "number" or kind == "boolean" then
    return value, true
  end
  if kind ~= "table" then
    mark_loss(opaque, invalid, owner, path)
    return nil, false
  end
  local first = seen[value]
  if first then
    local current_owner = owner or path
    local first_owner = first.owner or first.path
    if first.active then
      mark_loss(opaque, invalid, first_owner, first.path)
      mark_loss(opaque, invalid, current_owner, path)
      return nil, false
    end
    if supports_opaque(first_owner, first.path) and supports_opaque(current_owner, path) then
      mark_opaque(opaque, first_owner)
      mark_opaque(opaque, current_owner)
      return nil, false
    end
  end
  seen[value] = { active = true, owner = owner, path = copy_path(path) }

  local option = descriptor(path)
  -- Descriptor lists need an explicit array tag even when empty. Dense arrays nested inside open
  -- values (for example `spaces[].match.cwd`) have no descriptor of their own, but can still be
  -- recognised without mistaking an empty Lua map for an empty list.
  local shape = table_shape(value)
  local list = (option and option.type == "list") or shape == "list" or (shape == "empty" and nested_list(path))
  if shape == "invalid" or (option and option.type == "list" and shape ~= "list" and shape ~= "empty") then
    mark_loss(opaque, invalid, owner or path, path)
    seen[value].active = false
    return nil, false
  end
  local out = list and array() or {}
  if list then
    local complete = true
    for i = 1, #value do
      local child, ok = project(value[i], copy_path(path, tostring(i)), owner or path, opaque, invalid, array, seen)
      if ok then
        out[#out + 1] = child
      else
        complete = false
      end
    end
    if not complete then
      seen[value].active = false
      return nil, false
    end
  else
    for key, child_value in pairs(value) do
      if type(key) == "string" then
        local child_path = copy_path(path, key)
        local child_option = descriptor(child_path)
        local child_owner = owner
        if child_option and not child_option.container then
          child_owner = child_path
        elseif option and option.open then
          child_owner = child_path
        end
        local child, ok = project(child_value, child_path, child_owner, opaque, invalid, array, seen)
        if ok then
          out[key] = child
        end
      end
    end
  end
  seen[value].active = false
  return out, true
end

local function paths(values, array)
  local out = array()
  table.sort(values, function(a, b)
    return path_id(a) < path_id(b)
  end)
  for _, path in ipairs(values) do
    local encoded = array()
    for _, part in ipairs(path) do
      encoded[#encoded + 1] = part
    end
    out[#out + 1] = encoded
  end
  return out
end

---Projects any Lua settings table into Rust's JSON-shaped value, supported opaque owners, and
---typed paths that must default because their Lua values cannot cross JSON safely.
function M.project(value, array)
  array = array or function(item)
    return item or {}
  end
  local opaque, invalid = {}, {}
  local values = project(value, {}, nil, opaque, invalid, array, {})
  local opaque_paths, invalid_paths = {}, {}
  for _, path in pairs(opaque) do
    opaque_paths[#opaque_paths + 1] = path
  end
  for _, path in pairs(invalid) do
    invalid_paths[#invalid_paths + 1] = path
  end
  return values, opaque_paths, invalid_paths
end

function M.paths(values, array)
  return paths(values, array or function(item)
    return item or {}
  end)
end

local function value_at(root, path)
  local node = root
  for _, part in ipairs(path) do
    if type(node) ~= "table" then
      return nil, false
    end
    node = node[part]
  end
  return node, true
end

local function set_at(root, path, value)
  local node = root
  for i = 1, #path - 1 do
    local part = path[i]
    if type(node[part]) ~= "table" then
      node[part] = {}
    end
    node = node[part]
  end
  node[path[#path]] = value
end

---Restores config-as-code values by reference after a Rust JSON round trip. This deliberately
---preserves functions and cyclic/shared Lua tables instead of manufacturing lossy stand-ins.
function M.restore(target, original, opaque_paths)
  for _, path in ipairs(opaque_paths or {}) do
    local value, found = value_at(original, path)
    if found and #path > 0 then
      set_at(target, path, value)
    end
  end
  return target
end

---The settings pane's one host projection. Descriptors, validation, controls, editing and final
---persistence/clipboard bodies belong to Rust; Lua supplies only facts Rust cannot observe.
function M.body(cfg, array)
  cfg = cfg or config.get()
  array = array or function(value)
    return value or {}
  end
  local values, opaque_paths = M.project(cfg, array)
  local host_values = host_config.owned_keys(config.host_config, array)
  local defaults = project(keys.defaults(), { "keys" }, { "keys" }, {}, {}, array, {})
  local explicit = {}
  for _, path in ipairs(config.explicit_paths or {}) do
    local option = descriptor(path)
    -- Structural containers only group leaf descriptors. Marking one as a prefix lock would make
    -- `{ padding = { top = 2 } }` incorrectly lock every padding sibling. Open/Any values such as
    -- frame, settings and keys are real whole-value options and deliberately keep their lock.
    if not (option and option.container) then
      explicit[#explicit + 1] = path
    end
  end
  return {
    values = values,
    explicit = paths(explicit, array),
    host_values = host_values,
    opaque = paths(opaque_paths, array),
    key_defaults = defaults,
    is_macos = platform.is_mac == true,
    version = version,
  }
end

---Applies only renderer-produced effects. Unknown events are left for the settings-pane bridge.
function M.effect(gui_window, ev)
  if type(ev) ~= "table" then
    return false
  end
  if ev.t == "settings_commit" then
    require("vtabs.page").commit_effect(gui_window, ev)
    return true
  end
  if ev.t == "settings_copy" then
    if gui_window and type(ev.lua) == "string" then
      gui_window:copy_to_clipboard(ev.lua)
    end
    return true
  end
  if ev.t == "intent" and ev.a == "close_settings" then
    require("vtabs.settings").close(gui_window)
    return true
  end
  return false
end

return M

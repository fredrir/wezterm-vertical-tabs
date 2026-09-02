local util = require "vtabs.util"
local schema = require "vtabs.schema"

local M = {}

M.schema = schema
M.defaults = schema.defaults()

local current = nil

local function in_enum(option, value)
  for _, allowed in ipairs(option.enum) do
    if value == allowed then
      return true
    end
  end
  return false
end

---A list entry is either one of the option's named ids or a caller-supplied `{ id, ... }` entry.
---A list *of* tables only has its shape checked here; the option's own validator judges each entry.
local function list_ok(option, value)
  if type(value) ~= "table" then
    return false
  end
  local count, last = 0, 0
  for key in pairs(value) do
    if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
      return false
    end
    count = count + 1
    last = math.max(last, key)
  end
  if count ~= last then
    return false
  end
  for i = 1, count do
    local entry = value[i]
    if type(entry) == "table" then
      if option.of ~= "table" and type(entry.id) ~= "string" then
        return false
      end
    elseif option.of ~= "enum" or not in_enum(option, entry) then
      return false
    end
  end
  return true
end

local function type_ok(option, value)
  local kind = option.type
  if kind == "any" then
    return true
  end
  if kind == "enum" then
    return in_enum(option, value)
  end
  if kind == "list" then
    return list_ok(option, value)
  end
  return type(value) == kind
end

local function range_ok(option, value)
  if value ~= value or value == math.huge or value == -math.huge then
    return false
  end
  if option.integer and value ~= math.floor(value) then
    return false
  end
  if option.min and value < option.min then
    return false
  end
  return not (option.max and value > option.max)
end

local function reason(option, value)
  if option.type == "enum" then
    return string.format("invalid %s=%s, using default", option.key, tostring(value))
  end
  if option.type == "list" then
    local of = option.enum and table.concat(option.enum, ", ") or (option.of or "table") .. " entries"
    return string.format("%s must be a list of %s, using default", option.key, of)
  end
  if option.type == "number" then
    local kind = option.integer and "whole number" or "number"
    if option.min and option.max then
      return string.format("%s must be a %s %s-%s, using default", option.key, kind, option.min, option.max)
    end
    if option.min then
      return string.format("%s must be a %s >= %s, using default", option.key, kind, option.min)
    end
  end
  return string.format("%s must be a %s, using default", option.key, option.type)
end

local function validate(cfg)
  for _, option in ipairs(schema.options) do
    local value = schema.get(cfg, option.key)
    if value ~= nil then
      if option.alias and option.alias[value] ~= nil then
        value = option.alias[value]
        schema.set(cfg, option.key, value)
      end
      if not type_ok(option, value) or (option.type == "number" and not range_ok(option, value)) then
        util.warn("%s", reason(option, value))
        schema.set(cfg, option.key, schema.get(M.defaults, option.key))
      end
    end
  end
end

---Warns about keys the schema does not know, so a typo is not silently ignored.
local function check_unknown(opts, prefix)
  for key, value in pairs(opts) do
    if type(key) == "string" then
      local path = prefix and (prefix .. "." .. key) or key
      local option = schema.by_key[path]
      if option then
        if type(value) == "table" and not option.open then
          check_unknown(value, path)
        end
      elseif not schema.is_open(path) then
        util.warn("unknown option %s", path)
      end
    end
  end
end

---Every dotted key the user named in `opts`. The page renders these LOCKED: config-as-code wins,
---and `merge` loses the distinction the moment it runs.
local function explicit_keys(opts, prefix, parts, out, paths, seen)
  out = out or {}
  parts = parts or {}
  paths = paths or {}
  seen = seen or {}
  if seen[opts] then
    return out, paths
  end
  seen[opts] = true
  for key, value in pairs(opts) do
    if type(key) == "string" then
      local path = prefix and (prefix .. "." .. key) or key
      local segments = {}
      for i, part in ipairs(parts) do
        segments[i] = part
      end
      segments[#segments + 1] = key
      out[path] = true
      paths[#paths + 1] = segments
      -- open containers recurse too: the schema cannot validate their children, but the user still
      -- named them, and naming them is the whole question the page asks
      if type(value) == "table" then
        explicit_keys(value, path, segments, out, paths, seen)
      end
    end
  end
  seen[opts] = nil
  return out, paths
end

---The exact segmented paths authored in wezterm.lua, before a merge loses their ownership.
function M.explicit_keys(opts)
  return explicit_keys(opts or {})
end

---`defaults <- stored <- opts`: the settings file may move a default, but never something the
---user wrote in `wezterm.lua`.
function M.setup(opts, stored)
  opts = opts or {}
  check_unknown(opts)
  M.explicit, M.explicit_paths = M.explicit_keys(opts)
  local cfg = util.merge(M.defaults, stored or {})
  cfg = util.merge(cfg, opts)
  validate(cfg)

  -- `tab_height` decides the pad rows and `meta` whether there is a second content line; they are
  -- independent, so neither key rewrites the other any more.

  M.normalise(cfg)
  current = cfg
  return cfg
end

---Accepts a Rust-normalized serializable config while retaining Lua's config-as-code ownership.
---Opaque values have already been restored by the caller; policy validation stays in Rust.
function M.adopt_normalized(opts, cfg)
  M.explicit, M.explicit_paths = M.explicit_keys(opts)
  current = cfg
  return cfg
end

---Bootstrap/fallback mirror of Rust's cross-key policy. The current-capability live settings path
---arrives already canonical and uses `replace_canonical`; this remains for a missing/older binary.
function M.normalise(cfg)
  if cfg.popover.width ~= "auto" and type(cfg.popover.width) ~= "number" then
    util.warn 'popover.width must be "auto" or a number, using auto'
    cfg.popover.width = "auto"
  end

  -- press mode never hovers a background row, and without the highlight no row hovers at all, so a
  -- hover-only close button would never appear.
  if (cfg.hover == "press" or cfg.hover_highlight == false) and cfg.close_button == "hover" then
    cfg.close_button = "always"
  end
  if cfg.tear_off == "outside" then
    util.warn 'tear_off="outside" is not supported, using edge'
    cfg.tear_off = true
  end
  return cfg
end

function M.get()
  return current or M.setup {}
end

---Swaps values already canonicalized by the Rust settings document. Current-capability settings
---commits must not run a second Lua policy pass that can drift from persistence.
function M.replace_canonical(tbl)
  current = tbl
  return current
end

---What the host set on the WezTerm config before this plugin ran; the page shows these read-only.
M.host_config = {}
M.explicit = {}
M.explicit_paths = {}

---A framed sidebar keeps its last column for the content edge.
function M.framed(cfg)
  return type(cfg.frame) == "table" or cfg.frame == true
end

return M

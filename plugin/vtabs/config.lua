local util = require "vtabs.util"
local icons = require "vtabs.icons"
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
local function list_ok(option, value)
  if type(value) ~= "table" then
    return false
  end
  for _, entry in ipairs(value) do
    if type(entry) == "table" then
      if type(entry.id) ~= "string" then
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
    return string.format("%s must be a list of %s, using default", option.key, table.concat(option.enum, ", "))
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

function M.setup(opts)
  opts = opts or {}
  check_unknown(opts)
  local cfg = util.merge(M.defaults, opts)
  validate(cfg)

  -- The wider gutter belongs on the side touching the window edge, so an untouched pair mirrors.
  local pad, shipped = cfg.padding, M.defaults.padding
  if cfg.position == "right" and pad.left == shipped.left and pad.right == shipped.right then
    cfg.padding = { top = pad.top, bottom = pad.bottom, left = shipped.right, right = shipped.left }
  end
  if cfg.popover.width ~= "auto" and type(cfg.popover.width) ~= "number" then
    util.warn 'popover.width must be "auto" or a number, using auto'
    cfg.popover.width = "auto"
  end

  -- press mode never hovers a background row, so a hover-only close button would never appear.
  if cfg.hover == "press" and cfg.close_button == "hover" then
    cfg.close_button = "always"
  end
  if cfg.tear_off == "outside" then
    util.warn 'tear_off="outside" is not supported, using edge'
    cfg.tear_off = true
  end

  cfg.glyphs = icons.resolve(cfg.icon_map)
  current = cfg
  return cfg
end

function M.get()
  return current or M.setup {}
end

return M

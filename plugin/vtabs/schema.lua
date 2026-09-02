-- Descriptor facts are generated from vtabs-engine; this module keeps only generic dotted-key and default helpers.
local M = { options = require "vtabs.gen.schema" }
M.schema_id = M.options.schema_id

M.by_key = {}
for _, option in ipairs(M.options) do
  M.by_key[option.key] = option
end

---Walks a dotted key, creating tables on the way when `build` is set.
function M.at(tbl, key, build)
  local node = tbl
  local last = nil
  for part in key:gmatch "[^.]+" do
    if last then
      if node[last] == nil then
        if not build then
          return nil
        end
        node[last] = {}
      end
      node = node[last]
    end
    last = part
  end
  return node, last
end

function M.get(tbl, key)
  local node, last = M.at(tbl, key, false)
  return node and node[last]
end

function M.set(tbl, key, value)
  local node, last = M.at(tbl, key, true)
  node[last] = value
end

local function copy(value)
  if type(value) ~= "table" then
    return value
  end
  local out = {}
  for k, v in pairs(value) do
    out[k] = copy(v)
  end
  return out
end

---The default table, built from the schema so there is one source of truth.
function M.defaults()
  local out = {}
  for _, option in ipairs(M.options) do
    if option.default ~= nil then
      M.set(out, option.key, copy(option.default))
    end
  end
  return out
end

---True when `key` is inside an `open` container, whose children the schema does not enumerate.
function M.is_open(key)
  local prefix = nil
  for part in key:gmatch "[^.]+" do
    prefix = prefix and (prefix .. "." .. part) or part
    local option = M.by_key[prefix]
    if option and option.open and prefix ~= key then
      return true
    end
  end
  return false
end

return M

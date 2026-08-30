-- Regenerates the options table in docs/configuration.md from plugin/vtabs/schema.lua.
-- `lua scripts/gen-docs.lua` rewrites the file; `--check` exits 1 when it is out of date.
local here = arg[0]:match "^(.*)[/\\]" or "."
package.path = here .. "/../plugin/?.lua;" .. package.path

local schema = require "vtabs.schema"

local DOC = here .. "/../docs/configuration.md"
local START = "<!-- options:start -->"
local STOP = "<!-- options:end -->"

local function render(value)
  local kind = type(value)
  if kind == "string" then
    return string.format("%q", value)
  end
  if kind == "table" then
    return "{}"
  end
  return tostring(value)
end

---`shown` is the finished cell, backticks and all; computed defaults get them added.
local function default_of(option)
  if option.shown then
    return option.shown
  end
  return "`" .. (option.default == nil and "nil" or render(option.default)) .. "`"
end

local function width(s)
  local n = 0
  for _ in s:gmatch "[^\128-\191]" do
    n = n + 1
  end
  return n
end

local function pad(s, cols)
  return s .. string.rep(" ", math.max(cols - width(s), 0))
end

---What the option accepts, from the schema rather than prose.
local function values_of(option)
  if option.values then
    return option.values
  end
  -- a bare `|` would end the markdown cell
  if option.type == "enum" then
    local out = {}
    for _, allowed in ipairs(option.enum) do
      out[#out + 1] = "`" .. render(allowed) .. "`"
    end
    return table.concat(out, " \\| ")
  end
  if option.type == "number" then
    if option.min and option.max then
      return string.format("`%s`-`%s`", option.min, option.max)
    end
    return option.min and string.format("number >= `%s`", option.min) or "number"
  end
  if option.type == "boolean" then
    return "`true` \\| `false`"
  end
  return option.type
end

local function table_lines()
  local rows = {}
  for _, option in ipairs(schema.options) do
    if option.docs ~= false then
      rows[#rows + 1] = { "`" .. option.key .. "`", default_of(option), values_of(option) }
    end
  end
  local w1, w2 = width "option", width "default"
  for _, row in ipairs(rows) do
    w1, w2 = math.max(w1, width(row[1])), math.max(w2, width(row[2]))
  end
  local out = {
    string.format("| %s | %s | values |", pad("option", w1), pad("default", w2)),
    string.format("| %s | %s | ------ |", string.rep("-", w1), string.rep("-", w2)),
  }
  for _, row in ipairs(rows) do
    out[#out + 1] = string.format("| %s | %s | %s |", pad(row[1], w1), pad(row[2], w2), row[3])
  end
  return table.concat(out, "\n")
end

local function read(path)
  local f = assert(io.open(path, "r"), "cannot read " .. path)
  local body = f:read "a"
  f:close()
  return body
end

local function quote(s)
  return (s:gsub("[%^%$%(%)%%%.%[%]%*%+%-%?]", "%%%0"))
end

local body = read(DOC)
local head, tail = body:match("^(.-" .. quote(START) .. "\n).-(\n" .. quote(STOP) .. ".*)$")
if not head then
  io.stderr:write("markers " .. START .. " / " .. STOP .. " not found in " .. DOC .. "\n")
  os.exit(1)
end
local wanted = head .. table_lines() .. tail

if arg[1] == "--check" then
  if wanted ~= body then
    io.stderr:write "docs/configuration.md is stale; run `just docs`\n"
    os.exit(1)
  end
  print "docs up to date"
  os.exit(0)
end

local f = assert(io.open(DOC, "w"))
f:write(wanted)
f:close()
print "wrote docs/configuration.md"

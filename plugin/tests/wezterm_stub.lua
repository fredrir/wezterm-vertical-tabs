local M = {}

M.target_triple = os.getenv "VTABS_TEST_TRIPLE" or "aarch64-apple-darwin"

local function wide(code)
  return (code >= 0x1100 and code <= 0x115f)
    or (code >= 0x2e80 and code <= 0xa4cf)
    or (code >= 0xac00 and code <= 0xd7a3)
    or (code >= 0xf900 and code <= 0xfaff)
    or (code >= 0xff00 and code <= 0xff60)
    or (code >= 0x1f300 and code <= 0x1faff)
end

function M.column_width(s)
  local w = 0
  for _, code in utf8.codes(s) do
    if code >= 32 and code ~= 127 and not (code >= 0x80 and code <= 0x9f) then
      w = w + (wide(code) and 2 or 1)
    end
  end
  return w
end

M.executable_dir = "/usr/local/bin"
function M.hostname()
  return "macie"
end
function M.run_child_process(args)
  if type(args) == "table" and args[1] == "test" then
    return false, "", ""
  end
  return true, "", ""
end

M.nerdfonts = {}
M.home_dir = "/tmp"
M.GLOBAL = {}

local function rgb_from_hex(hex)
  local r, g, b = hex:match "^#(%x%x)(%x%x)(%x%x)$"
  return tonumber(r, 16), tonumber(g, 16), tonumber(b, 16)
end

local Color = {}
Color.__index = Color

local function color(r, g, b)
  return setmetatable({ r = r, g = g, b = b }, Color)
end

function Color:srgba_u8()
  return self.r, self.g, self.b, 255
end

function Color:lighten(f)
  local function up(c)
    return math.min(255, math.floor(c + (255 - c) * f))
  end
  return color(up(self.r), up(self.g), up(self.b))
end

function Color:darken(f)
  local function down(c)
    return math.floor(c * (1 - f))
  end
  return color(down(self.r), down(self.g), down(self.b))
end

M.color = {
  parse = function(s)
    if type(s) ~= "string" then
      return nil
    end
    local r, g, b = rgb_from_hex(s)
    if not r then
      error("bad color " .. s)
    end
    return color(r, g, b)
  end,
}

M.action = setmetatable({}, {
  __index = function(_, name)
    return function(arg)
      return { action = name, arg = arg }
    end
  end,
})

function M.action_callback(fn)
  return { callback = fn }
end

M.log = {}
function M.log_info(msg)
  M.log[#M.log + 1] = msg
end
M.log_warn = M.log_info
M.log_error = M.log_info

M.handlers = {}
function M.on(name, fn)
  M.handlers[name] = M.handlers[name] or {}
  table.insert(M.handlers[name], fn)
end

M.time = {
  now = function()
    return {
      format = function()
        return tostring(os.time()) .. ".000"
      end,
    }
  end,
}

M.mux = {
  all_windows = function()
    return {}
  end,
}
M.plugin = {
  list = function()
    return {}
  end,
}

local function encode(v)
  local t = type(v)
  if t == "table" then
    local parts = {}
    for k, val in pairs(v) do
      parts[#parts + 1] = string.format("%q:%s", tostring(k), encode(val))
    end
    return "{" .. table.concat(parts, ",") .. "}"
  elseif t == "string" then
    return string.format("%q", v)
  end
  return tostring(v)
end
M.json_encode = encode

local function decode(s, i)
  i = s:find("%S", i)
  local c = s:sub(i, i)
  if c == "{" then
    local out = {}
    i = i + 1
    while true do
      i = s:find("%S", i)
      if s:sub(i, i) == "}" then
        return out, i + 1
      end
      local key
      key, i = decode(s, i)
      i = s:find(":", i) + 1
      out[key], i = decode(s, i)
      i = s:find("%S", i)
      if s:sub(i, i) == "," then
        i = i + 1
      end
    end
  elseif c == "[" then
    local out = {}
    i = i + 1
    while true do
      i = s:find("%S", i)
      if s:sub(i, i) == "]" then
        return out, i + 1
      end
      out[#out + 1], i = decode(s, i)
      i = s:find("%S", i)
      if s:sub(i, i) == "," then
        i = i + 1
      end
    end
  elseif c == '"' then
    local j = i + 1
    local buf = {}
    while s:sub(j, j) ~= '"' do
      if s:sub(j, j) == "\\" then
        j = j + 1
      end
      buf[#buf + 1] = s:sub(j, j)
      j = j + 1
    end
    return table.concat(buf), j + 1
  elseif s:sub(i, i + 3) == "true" then
    return true, i + 4
  elseif s:sub(i, i + 4) == "false" then
    return false, i + 5
  elseif s:sub(i, i + 3) == "null" then
    return nil, i + 4
  end
  local num = s:match("^-?%d+%.?%d*[eE]?[-+]?%d*", i)
  return tonumber(num), i + #num
end

function M.json_parse(s)
  local value = decode(s, 1)
  return value
end

return M

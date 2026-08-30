local M = {}

M.target_triple = "aarch64-apple-darwin"
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

return M

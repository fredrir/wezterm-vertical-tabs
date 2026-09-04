local M = {
  GLOBAL = {},
  executable_dir = "/usr/bin",
  home_dir = os.getenv "HOME",
  log = {},
  target_triple = os.getenv "VTABS_TEST_TRIPLE" or "x86_64-unknown-linux-gnu",
}

function M.hostname()
  return "test-host.example"
end

function M.log_info(message)
  M.log[#M.log + 1] = message
end
M.log_warn = M.log_info
M.log_error = M.log_info

M.time = {
  now = function()
    return {
      format = function()
        return "1700000000.000"
      end,
    }
  end,
  call_after = function(_, fn)
    fn()
  end,
}

M.procinfo = {
  pid = function()
    return nil
  end,
}
M.mux = {
  all_windows = function()
    return {}
  end,
  get_pane = function()
    return nil
  end,
}
M.plugin = {
  list = function()
    return {}
  end,
}

local function shell_quote(value)
  return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

local function execute(command)
  local ok, _, code = os.execute(command)
  return ok == true or code == 0, "", ok and "" or "command failed"
end

function M.run_child_process(argv)
  if type(argv) ~= "table" then
    return false, "", "argv must be a table"
  end
  if argv[1] == "test" and argv[2] == "-L" and type(argv[3]) == "string" and argv[4] == nil then
    return execute("test -L " .. shell_quote(argv[3]))
  end
  if argv[1] == "mkdir" and argv[2] == "-m" and argv[3] == "700" then
    if argv[4] == "-p" and type(argv[5]) == "string" and argv[6] == nil then
      return execute("mkdir -m 700 -p " .. shell_quote(argv[5]))
    elseif type(argv[4]) == "string" and argv[5] == nil then
      return execute("mkdir -m 700 " .. shell_quote(argv[4]))
    end
  elseif argv[1] == "chmod" and argv[2] == "600" and type(argv[3]) == "string" and argv[4] == nil then
    return execute("chmod 600 " .. shell_quote(argv[3]))
  elseif argv[1] == "rmdir" and type(argv[2]) == "string" and argv[3] == nil then
    return execute("rmdir " .. shell_quote(argv[2]))
  end
  return false, "", "disabled by the headless test adapter"
end

function M.background_child_process()
  return false
end

function M.sleep_ms() end
function M.reload_configuration() end
function M.add_to_config_reload_watch_list() end
function M.on() end

M.action = setmetatable({}, {
  __index = function(_, name)
    return function(argument)
      return { name = name, argument = argument }
    end
  end,
})

function M.action_callback(callback)
  return { callback = callback }
end

local Color = {}
Color.__index = Color

function Color:srgba_u8()
  return self.r, self.g, self.b, 255
end

function M.color_from_rgb(r, g, b)
  return setmetatable({ r = r, g = g, b = b }, Color)
end

M.color = {
  parse = function(value)
    if type(value) ~= "string" then
      error "colour must be a string"
    end
    local r, g, b = value:match "^#(%x%x)(%x%x)(%x%x)$"
    if not r then
      error("unsupported test colour: " .. value)
    end
    return M.color_from_rgb(tonumber(r, 16), tonumber(g, 16), tonumber(b, 16))
  end,
}

local ESCAPES = {
  ['"'] = '\\"',
  ["\\"] = "\\\\",
  ["\b"] = "\\b",
  ["\f"] = "\\f",
  ["\n"] = "\\n",
  ["\r"] = "\\r",
  ["\t"] = "\\t",
}

local function quote(value)
  return '"'
    .. value:gsub('[%z\1-\31"\\]', function(char)
      return ESCAPES[char] or string.format("\\u%04x", char:byte())
    end)
    .. '"'
end

local function encode(value, seen)
  local kind = type(value)
  if kind == "nil" then
    return "null"
  elseif kind == "boolean" then
    return tostring(value)
  elseif kind == "number" then
    if value ~= value or value == math.huge or value == -math.huge then
      return "null"
    end
    if value == math.floor(value) and math.abs(value) < 2 ^ 53 then
      return string.format("%d", value)
    end
    return string.format("%.17g", value)
  elseif kind == "string" then
    return quote(value)
  elseif kind ~= "table" then
    error("cannot JSON encode " .. kind)
  end

  if seen[value] then
    error "cannot JSON encode a cyclic table"
  end
  seen[value] = true

  local count, largest, array = 0, 0, true
  for key in pairs(value) do
    if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
      array = false
    else
      count = count + 1
      largest = math.max(largest, key)
    end
  end
  array = array and count > 0 and count == largest

  local parts = {}
  if array then
    for index = 1, largest do
      parts[index] = encode(value[index], seen)
    end
  else
    local keys = {}
    for key in pairs(value) do
      if type(key) ~= "string" then
        error "JSON object keys must be strings"
      end
      keys[#keys + 1] = key
    end
    table.sort(keys)
    for _, key in ipairs(keys) do
      parts[#parts + 1] = quote(key) .. ":" .. encode(value[key], seen)
    end
  end
  seen[value] = nil
  return (array and "[" or "{") .. table.concat(parts, ",") .. (array and "]" or "}")
end

function M.json_encode(value)
  return encode(value, {})
end

local function json_error(position, message)
  error(string.format("invalid JSON at byte %d: %s", position or 0, message), 0)
end

local function skip_space(source, position)
  local found = source:find("%S", position)
  return found or (#source + 1)
end

local function decode_string(source, position)
  local out = {}
  position = position + 1
  while position <= #source do
    local char = source:sub(position, position)
    if char == '"' then
      return table.concat(out), position + 1
    elseif char == "\\" then
      local escaped = source:sub(position + 1, position + 1)
      local replacements = {
        ['"'] = '"',
        ["\\"] = "\\",
        ["/"] = "/",
        b = "\b",
        f = "\f",
        n = "\n",
        r = "\r",
        t = "\t",
      }
      if replacements[escaped] then
        out[#out + 1] = replacements[escaped]
        position = position + 2
      elseif escaped == "u" then
        local hex = source:sub(position + 2, position + 5)
        local codepoint = #hex == 4 and tonumber(hex, 16) or nil
        if not codepoint then
          json_error(position, "bad unicode escape")
        end
        out[#out + 1] = utf8.char(codepoint)
        position = position + 6
      else
        json_error(position, "bad escape")
      end
    else
      if char:byte() < 32 then
        json_error(position, "control character in string")
      end
      out[#out + 1] = char
      position = position + 1
    end
  end
  json_error(position, "unterminated string")
end

local decode

local function decode_array(source, position)
  local out = {}
  position = skip_space(source, position + 1)
  if source:sub(position, position) == "]" then
    return out, position + 1
  end
  while true do
    local value
    value, position = decode(source, position)
    out[#out + 1] = value
    position = skip_space(source, position)
    local delimiter = source:sub(position, position)
    if delimiter == "]" then
      return out, position + 1
    elseif delimiter ~= "," then
      json_error(position, "expected ',' or ']'")
    end
    position = skip_space(source, position + 1)
  end
end

local function decode_object(source, position)
  local out = {}
  position = skip_space(source, position + 1)
  if source:sub(position, position) == "}" then
    return out, position + 1
  end
  while true do
    if source:sub(position, position) ~= '"' then
      json_error(position, "expected object key")
    end
    local key
    key, position = decode_string(source, position)
    position = skip_space(source, position)
    if source:sub(position, position) ~= ":" then
      json_error(position, "expected ':'")
    end
    out[key], position = decode(source, skip_space(source, position + 1))
    position = skip_space(source, position)
    local delimiter = source:sub(position, position)
    if delimiter == "}" then
      return out, position + 1
    elseif delimiter ~= "," then
      json_error(position, "expected ',' or '}'")
    end
    position = skip_space(source, position + 1)
  end
end

decode = function(source, position)
  position = skip_space(source, position)
  local char = source:sub(position, position)
  if char == '"' then
    return decode_string(source, position)
  elseif char == "{" then
    return decode_object(source, position)
  elseif char == "[" then
    return decode_array(source, position)
  elseif source:sub(position, position + 3) == "true" then
    return true, position + 4
  elseif source:sub(position, position + 4) == "false" then
    return false, position + 5
  elseif source:sub(position, position + 3) == "null" then
    return nil, position + 4
  end
  local token = source:match("^-?%d+%.?%d*[eE]?[-+]?%d*", position)
  local number = token and tonumber(token) or nil
  if not number then
    json_error(position, "expected value")
  end
  return number, position + #token
end

function M.json_parse(source)
  if type(source) ~= "string" then
    error "JSON source must be a string"
  end
  local value, position = decode(source, 1)
  if skip_space(source, position) <= #source then
    json_error(position, "trailing content")
  end
  return value
end

return M

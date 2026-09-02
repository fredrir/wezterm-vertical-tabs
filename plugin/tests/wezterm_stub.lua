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
-- Paths the fake `test -L` should report as symlinks, so the refusal can be pinned.
M.symlinks = {}
-- Every argv run, oldest first; `M.cli` answers `wezterm cli` argvs, nil means the CLI is unusable.
M.spawned = {}
M.cli = nil
M.normalizer = nil
M.chmod_ok = true
function M.run_child_process(args)
  M.spawned[#M.spawned + 1] = args
  if type(args) == "table" and args[1] == "test" then
    if args[2] == "-x" then
      local file = io.open(args[3], "rb")
      if file then
        file:close()
        return true, "", ""
      end
      return false, "", ""
    end
    return M.symlinks[args[3]] == true, "", ""
  end
  if type(args) == "table" and args[1] == "mkdir" then
    local recursive = args[4] == "-p"
    local path = recursive and args[5] or args[4]
    local ok = os.execute(string.format("mkdir -m %q %s %q", args[3], recursive and "-p" or "", path))
    return ok == true or ok == 0, "", ""
  end
  if type(args) == "table" and args[1] == "chmod" then
    if not M.chmod_ok then
      return false, "", "refused"
    end
    local ok = os.execute(string.format("chmod %q %q", args[2], args[3]))
    return ok == true or ok == 0, "", ""
  end
  if type(args) == "table" and args[1] == "rmdir" then
    local ok = os.execute(string.format("rmdir %q", args[2]))
    return ok == true or ok == 0, "", ""
  end
  if type(args) == "table" and args[2] == "settings" and args[3] == "normalize" then
    if M.normalizer then
      return M.normalizer(args)
    end
    return false, "", "not installed"
  end
  require("support.async").yield "child"
  if type(args) == "table" and args[2] == "cli" then
    return M.cli ~= nil and M.cli(args) == true, "", ""
  end
  return true, "", ""
end

function M.sleep_ms()
  require("support.async").yield "sleep"
end

-- nil pid keeps `own_socket` false, so every suite stays on the activation fallbacks by default.
M.pid = nil
M.procinfo = {
  pid = function()
    return M.pid
  end,
}

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
M.reloads = 0
M.reload_watch = {}
function M.reload_configuration()
  M.reloads = M.reloads + 1
end

function M.add_to_config_reload_watch_list(path)
  M.reload_watch[#M.reload_watch + 1] = path
end

function M.on(name, fn)
  M.handlers[name] = M.handlers[name] or {}
  table.insert(M.handlers[name], fn)
end

M.timers = {}
M.time = {
  now = function()
    return {
      format = function()
        return tostring(os.time()) .. ".000"
      end,
    }
  end,
  call_after = function(secs, fn)
    M.timers[#M.timers + 1] = { secs = secs, fn = fn }
  end,
}

---Runs every scheduled callback in order, as the executor would once their delays had elapsed.
function M.fire_timers()
  local due = M.timers
  M.timers = {}
  for _, timer in ipairs(due) do
    timer.fn()
  end
end

M.panes = {}
M.windows = {}

M.mux = {
  all_windows = function()
    return M.windows
  end,
  get_pane = function(pane_id)
    return M.panes[pane_id]
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

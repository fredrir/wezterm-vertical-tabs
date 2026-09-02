local wezterm = require "wezterm" ---@type Wezterm
local platform = require "vtabs.platform"

local M = {}

local function is_list(t)
  return type(t) == "table" and #t > 0 and next(t, #t) == nil
end

---Deep-merges `override` into a copy of `base`; lists are replaced, not merged.
function M.merge(base, override)
  local out = {}
  for k, v in pairs(base or {}) do
    out[k] = v
  end
  for k, v in pairs(override or {}) do
    if type(v) == "table" and type(out[k]) == "table" and not is_list(v) and not is_list(out[k]) then
      out[k] = M.merge(out[k], v)
    else
      out[k] = v
    end
  end
  return out
end

---Calls `fn(...)` and returns its result, or nil when it errors.
---Read through a seam, because Lua cannot set the process environment a test needs to fake.
function M.getenv(name)
  return os.getenv(name)
end

-- WezTerm reports a window, tab or pane that left mid-call with these texts; nothing structured.
local GONE = { "not found in mux", "is not valid" }

function M.window_gone(err)
  local text = tostring(err)
  for _, needle in ipairs(GONE) do
    if text:find(needle, 1, true) then
      return true
    end
  end
  return false
end

function M.try(fn, ...)
  local ok, value = pcall(fn, ...)
  if ok then
    return value
  end
  return nil
end

function M.now_ms()
  local t = M.try(function()
    return tonumber(wezterm.time.now():format "%s%.3f")
  end)
  if t then
    return math.floor(t * 1000)
  end
  return os.time() * 1000
end

function M.width(s)
  if wezterm.column_width then
    return wezterm.column_width(s)
  end
  return utf8.len(s) or #s
end

---Removes control characters so foreign titles cannot inject escapes into the sidebar.
local function seq_len(b)
  if b < 0x80 then
    return 1
  elseif b >= 0xc2 and b <= 0xdf then
    return 2
  elseif b >= 0xe0 and b <= 0xef then
    return 3
  elseif b >= 0xf0 and b <= 0xf4 then
    return 4
  end
  return 0
end

---Rejects overlongs, surrogate halves, past-U+10FFFF and truncated tails.
local function well_formed(s, i, len)
  local b1, b2 = string.byte(s, i), string.byte(s, i + 1)
  if len >= 2 then
    if not b2 or b2 < 0x80 or b2 > 0xbf then
      return false
    end
    if (b1 == 0xe0 and b2 < 0xa0) or (b1 == 0xed and b2 > 0x9f) then
      return false
    end
    if (b1 == 0xf0 and b2 < 0x90) or (b1 == 0xf4 and b2 > 0x8f) then
      return false
    end
  end
  for k = 2, len - 1 do
    local b = string.byte(s, i + k)
    if not b or b < 0x80 or b > 0xbf then
      return false
    end
  end
  return true
end

---Returns valid UTF-8 with no control characters, whatever bytes went in: a remote OSC 7 cwd
---reaches us undecoded, and one raw 0x9b would otherwise make every width call in render raise.
function M.sanitize(s)
  if type(s) ~= "string" then
    return ""
  end
  local out, i, n = {}, 1, #s
  while i <= n do
    local b = string.byte(s, i)
    local len = seq_len(b)
    if len == 0 or not well_formed(s, i, len) then
      i = i + 1
    elseif len == 1 then
      if b >= 0x20 and b ~= 0x7f then
        out[#out + 1] = string.char(b)
      end
      i = i + 1
    else
      local b2, b3 = string.byte(s, i + 1), string.byte(s, i + 2)
      -- U+202A-202E reorder the glyphs around them; the U+2066-2069 isolates are legitimate
      local bidi_override = b == 0xe2 and b2 == 0x80 and b3 and b3 >= 0xaa and b3 <= 0xae
      if not (b == 0xc2 and b2 <= 0x9f) and not bidi_override then
        out[#out + 1] = s:sub(i, i + len - 1)
      end
      i = i + len
    end
  end
  return table.concat(out)
end

function M.basename(path)
  if not path then
    return nil
  end
  return path:match "([^/\\]+)[/\\]*$" or path
end

function M.contains(list, value)
  for _, v in ipairs(list or {}) do
    if v == value then
      return true
    end
  end
  return false
end

---Splits a list into two by predicate, preserving order.
function M.partition(list, pred)
  local yes, no = {}, {}
  for _, v in ipairs(list) do
    if pred(v) then
      yes[#yes + 1] = v
    else
      no[#no + 1] = v
    end
  end
  return yes, no
end

function M.sorted_keys(t)
  local keys = {}
  for k in pairs(t) do
    keys[#keys + 1] = k
  end
  table.sort(keys, function(a, b)
    return tostring(a) < tostring(b)
  end)
  return keys
end

---FNV-1a of `s` as eight hex digits: a stable, cheap fingerprint for cache keys and colour slots.
function M.fnv1a(s)
  local hash = 2166136261
  for i = 1, #s do
    hash = (hash ~ s:byte(i)) * 16777619 % 4294967296
  end
  return string.format("%08x", hash)
end

function M.active_tab(gui_window)
  return M.try(function()
    return gui_window:mux_window():active_tab()
  end)
end

function M.log(fmt, ...)
  wezterm.log_info("vtabs: " .. string.format(fmt, ...))
end

function M.warn(fmt, ...)
  wezterm.log_warn("vtabs: " .. string.format(fmt, ...))
end

local warned = {}

---Warns once per key; user hooks that keep failing should not spam the log.
function M.warn_once(key, fmt, ...)
  if warned[key] then
    return
  end
  warned[key] = true
  M.warn(fmt, ...)
end

-- Reseeding per call made tokens minted in the same millisecond identical; the pointer adds entropy.
math.randomseed(os.time() + math.floor(os.clock() * 1000000) + (tonumber(tostring({}):match "%x+$", 16) or 0))

local function urandom(bytes)
  local f = io.open("/dev/urandom", "rb")
  if not f then
    return nil
  end
  local raw = f:read(bytes)
  f:close()
  if type(raw) ~= "string" or #raw < bytes then
    return nil
  end
  return (raw:gsub(".", function(c)
    return string.format("%02x", c:byte())
  end))
end

local B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local B64_VALUE = {}
for i = 1, #B64 do
  B64_VALUE[B64:sub(i, i)] = i - 1
end

---Decodes standard base64 (padding optional); nil for anything malformed. WezTerm has no Lua helper.
function M.base64_decode(s)
  if type(s) ~= "string" or not s:match "^[A-Za-z0-9+/]*=?=?$" then
    return nil
  end
  local body = (s:gsub("=+$", ""))
  if #body % 4 == 1 then
    return nil
  end
  local out, acc, bits = {}, 0, 0
  for i = 1, #body do
    acc = ((acc << 6) | B64_VALUE[body:sub(i, i)]) & 0xffffff
    bits = bits + 6
    if bits >= 8 then
      bits = bits - 8
      out[#out + 1] = string.char((acc >> bits) & 0xff)
    end
  end
  return table.concat(out)
end

---True when `path` is a symlink: a file we would otherwise write through without knowing where to.
function M.is_symlink(path)
  if platform.is_windows then
    return false
  end
  return M.try(wezterm.run_child_process, { "test", "-L", path }) == true
end

---Writes `body` to `path` atomically and readable only by its owner: a temp file beside it,
---chmod 600, then rename. `dir` is created 0700 if the first open fails. Returns true on success.
function M.write_private(path, body, dir, tag)
  tag = tag or "file"
  local tmp = path .. "." .. M.random_token():sub(1, 8) .. ".tmp"
  local f = io.open(tmp, "w")
  if not f and dir then
    M.try(wezterm.run_child_process, { "mkdir", "-m", "700", "-p", dir })
    f = io.open(tmp, "w")
  end
  if not f then
    M.warn_once(tag .. "-file", "cannot write %s", path)
    return false
  end
  if not platform.is_windows and M.try(wezterm.run_child_process, { "chmod", "600", tmp }) ~= true then
    M.warn_once(tag .. "-chmod", "cannot restrict %s to 0600", path)
  end
  f:write(body)
  f:close()
  if os.rename(tmp, path) then
    return true
  end
  os.remove(tmp)
  M.warn_once(tag .. "-rename", "cannot replace %s", path)
  return false
end

function M.random_token()
  local token = urandom(16)
  if token then
    return token
  end
  local parts = {}
  for _ = 1, 4 do
    parts[#parts + 1] = string.format("%08x", math.random(0, 0x7fffffff))
  end
  return table.concat(parts)
end

return M

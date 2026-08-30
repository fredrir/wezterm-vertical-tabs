local wezterm = require "wezterm" ---@type Wezterm

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

---Truncates `s` to `max` display columns, appending `ellipsis` when cut.
function M.truncate(s, max, ellipsis)
  ellipsis = ellipsis or "…"
  if max <= 0 then
    return ""
  end
  if M.width(s) <= max then
    return s
  end
  local budget = max - M.width(ellipsis)
  if budget <= 0 then
    return ""
  end
  local offsets = {}
  for pos in utf8.codes(s) do
    offsets[#offsets + 1] = pos
  end
  offsets[#offsets + 1] = #s + 1
  local lo, hi = 1, #offsets
  while lo < hi do
    local mid = (lo + hi + 1) // 2
    if M.width(s:sub(1, offsets[mid] - 1)) <= budget then
      lo = mid
    else
      hi = mid - 1
    end
  end
  return s:sub(1, offsets[lo] - 1) .. ellipsis
end

---Elides middle path components to fit `budget`; the basename is the informative part, so it
---survives until nothing else can go. `util.truncate` cuts from the right and is kept for titles.
function M.shorten_path(path, budget, ellipsis)
  ellipsis = ellipsis or "…"
  if type(path) ~= "string" or path == "" or budget <= 0 then
    return ""
  end
  if M.width(path) <= budget then
    return path
  end
  local sep = (path:find("\\", 1, true) and not path:find("/", 1, true)) and "\\" or "/"
  local parts = {}
  for part in path:gmatch "[^/\\]+" do
    parts[#parts + 1] = part
  end
  local lead = ""
  if parts[1] and parts[1]:match "^%a:$" then
    lead = table.remove(parts, 1) .. sep
  elseif path:sub(1, 1) == "/" or path:sub(1, 1) == "\\" then
    lead = sep
  end
  if #parts <= 1 then
    return lead .. M.truncate(parts[1] or "", budget - M.width(lead), ellipsis)
  end
  local function joined()
    return lead .. table.concat(parts, sep)
  end
  -- Leftmost first, and never the marker or the basename.
  local from = (parts[1] == "~" or parts[1] == "..") and 2 or 1
  for i = from, #parts - 1 do
    if M.width(joined()) <= budget then
      break
    end
    parts[i] = M.truncate(parts[i], 1, "")
  end
  local out = joined()
  if M.width(out) <= budget then
    return out
  end
  local base = parts[#parts]
  local room = budget - M.width(ellipsis) - 1
  if room < 1 then
    return M.truncate(base, budget, ellipsis)
  end
  return ellipsis .. sep .. M.truncate(base, room, ellipsis)
end

function M.pad_right(s, cols)
  local w = M.width(s)
  if w >= cols then
    return s
  end
  return s .. string.rep(" ", cols - w)
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
      if not (b == 0xc2 and string.byte(s, i + 1) <= 0x9f) then
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

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

function M.now_ms()
  local ok, t = pcall(function()
    return tonumber(wezterm.time.now():format "%s%.3f")
  end)
  if ok and t then
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
  if M.width(s) <= max then
    return s
  end
  local budget = max - M.width(ellipsis)
  if budget <= 0 then
    return ""
  end
  local out = ""
  for _, code in utf8.codes(s) do
    local ch = utf8.char(code)
    if M.width(out .. ch) > budget then
      break
    end
    out = out .. ch
  end
  return out .. ellipsis
end

function M.pad_right(s, cols)
  local w = M.width(s)
  if w >= cols then
    return s
  end
  return s .. string.rep(" ", cols - w)
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

function M.index_of(list, value)
  for i, v in ipairs(list or {}) do
    if v == value then
      return i
    end
  end
  return nil
end

function M.log(fmt, ...)
  wezterm.log_info("vtabs: " .. string.format(fmt, ...))
end

function M.warn(fmt, ...)
  wezterm.log_warn("vtabs: " .. string.format(fmt, ...))
end

return M

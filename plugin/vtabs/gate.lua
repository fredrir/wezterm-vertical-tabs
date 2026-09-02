local wezterm = require "wezterm" ---@type Wezterm
local store = require "vtabs.store"
local util = require "vtabs.util"

---One mutation of a window's pane tree at a time. Every handler is its own coroutine and every
---mux call it awaits is a place another handler runs, so a split, close, adjust or move takes the
---window's gate first; a nested call from the holder's own coroutine runs inline.
local M = {}

M.STALE_MS = 5000
M.POLL_MS = 10
M.POLL_MAX_MS = 80

local scope = store.scope "gate"
local holder = scope.window()
local newest = scope.window()

local function mine(wid)
  local held = holder[wid]
  return held ~= nil and held.co == coroutine.running()
end

local function stale(held, now)
  return now - held.at > M.STALE_MS
end

---Waits its turn, polling with backoff; a holder silent past STALE_MS is evicted with a warning,
---never waited on forever. A `ticket` waiter gives up as soon as a newer ticket of its name exists.
local function acquire(wid, name, ticket)
  local wait = M.POLL_MS
  while true do
    if ticket and newest[wid][name] ~= ticket then
      return nil
    end
    local held = holder[wid]
    if held == nil then
      break
    end
    local now = util.now_ms()
    if stale(held, now) then
      util.warn("gate: %s held window %d for %d ms; released", held.name, wid, now - held.at)
      break
    end
    if not coroutine.isyieldable() then
      util.warn_once("gate-main", "gate: %s ran unguarded on the main thread", name)
      break
    end
    wezterm.sleep_ms(wait)
    wait = math.min(wait * 2, M.POLL_MAX_MS)
  end
  local hold = { co = coroutine.running(), at = util.now_ms(), name = name }
  holder[wid] = hold
  return hold
end

---An evicted holder that resumes later must not clear its successor.
local function release(wid, hold)
  if holder[wid] == hold then
    holder[wid] = nil
  end
end

local function run(wid, name, fn, ticket, ...)
  if mine(wid) then
    return fn(...)
  end
  local hold = acquire(wid, name, ticket)
  if hold == nil then
    return nil, "superseded"
  end
  local res = table.pack(pcall(fn, ...))
  release(wid, hold)
  if not res[1] then
    error(res[2], 0)
  end
  return table.unpack(res, 2, res.n)
end

---Runs `fn` under the window's gate and returns its results; re-entrant for the holder's coroutine.
function M.run(wid, name, fn, ...)
  return run(wid, name, fn, nil, ...)
end

---Like `run`, but only the newest waiting call of `name` survives: a key held down queues one
---switch, not one per repeat. A call that lost returns `nil, "superseded"`.
function M.latest(wid, name, fn, ...)
  local ticket = {}
  newest[wid] = newest[wid] or {}
  newest[wid][name] = ticket
  return run(wid, name, fn, ticket, ...)
end

---Runs `fn` when the gate is free or already this coroutine's; otherwise `nil, "busy"` at once.
function M.try(wid, name, fn, ...)
  local held = holder[wid]
  if held ~= nil and not mine(wid) and not stale(held, util.now_ms()) then
    return nil, "busy"
  end
  return M.run(wid, name, fn, ...)
end

function M.held(wid)
  return holder[wid] ~= nil
end

return M

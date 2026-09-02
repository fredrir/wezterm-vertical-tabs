local wezterm = require "wezterm" ---@type Wezterm
local store = require "vtabs.store"
local mux = require "vtabs.mux"
local util = require "vtabs.util"

---A frame sent to a pane on a mux domain crosses the link on the GUI thread: `send_text` on a
---client pane blocks until the server answers (wezterm-client/src/pane/clientpane.rs
---`PaneWriter::write`, `block_on`). While the client is rebuilding its mirror from a storm of
---`TabResized`, that round trip has deadlocked the GUI: the main thread parked in `send_text`, the
---mux client thread parked, the server idle (macOS hang reports of 2026-09-02 23:28 and
---2026-09-03 00:07, both right after hundreds of server-side pane resizes in a second).
---
---Server-side pane resizes are the storm's visible edge: a `resize` report from a mux pane marks
---its domain busy. Nothing is published and no adjust is sent while a domain is busy, and the
---frames that must go all the same are held, in order, until the domain has been quiet for
---`QUIET_MS`.
local M = {}

M.QUIET_MS = 300

local scope = store.scope "link"
local busy_at = scope.process()
local held = scope.pane()
local armed = false

---The pane's own answer is a local lookup; the place recorded at attach stands in for a pane gone.
local function domain_of(pane)
  local domain = mux.domain(pane)
  if domain then
    return domain
  end
  local place = store.pane_domain[pane:pane_id()]
  return place and place:match "^([^@]*)@" or "local"
end

function M.activity(pane)
  local domain = domain_of(pane)
  if domain ~= "local" then
    busy_at[domain] = util.now_ms()
  end
end

function M.busy(domain)
  return domain ~= "local" and util.now_ms() - (busy_at[domain] or 0) < M.QUIET_MS
end

function M.busy_any()
  local now = util.now_ms()
  for _, at in pairs(busy_at) do
    if now - at < M.QUIET_MS then
      return true
    end
  end
  return false
end

local flush

local function arm()
  if armed then
    return
  end
  armed = true
  wezterm.time.call_after(M.QUIET_MS / 1000, function()
    armed = false
    flush()
  end)
end

flush = function()
  local waiting = false
  for pid, queue in pairs(held) do
    if M.busy(domain_of(queue.pane)) then
      waiting = true
    else
      held[pid] = nil
      pcall(function()
        queue.pane:send_text(table.concat(queue.texts))
      end)
    end
  end
  if waiting then
    arm()
  end
end

---Holds `text` for `pane` when its domain is busy; false when it may cross the link now.
function M.defer(pane, text)
  if not M.busy(domain_of(pane)) then
    return false
  end
  local pid = pane:pane_id()
  local queue = held[pid]
  if not queue then
    queue = { pane = pane, texts = {} }
    held[pid] = queue
  end
  queue.texts[#queue.texts + 1] = text
  arm()
  return true
end

function M.flush()
  flush()
end

function M.reset()
  for domain in pairs(busy_at) do
    busy_at[domain] = nil
  end
  for pid in pairs(held) do
    held[pid] = nil
  end
end

return M

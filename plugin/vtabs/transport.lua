local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local store = require "vtabs.store"
local backend = require "vtabs.backend"
local mux = require "vtabs.mux"
local protocol = require "vtabs.gen.protocol"
local util = require "vtabs.util"

---Frames for a backend on a mux server of this machine, without crossing the link: `send_text` on
---a client pane blocks the GUI thread on a server round trip (link.lua), so once a backend has
---announced an inbox directory its frames are files in it, one per `send_raw` batch, renamed into
---place in sequence. The handshake is a probe as message 1 and a barrier on stdin; the pane
---switches only on the backend's `transport_ready` for that session, and whatever was sent
---meanwhile waits in order. A refusal, a timeout, a full queue or a failed write puts the pane
---back on stdin, and a lost message makes the wire resend everything (docs/plans/inbox-transport.md).
local M = {}

M.QUEUE_MAX = 64
M.NEGOTIATE_MS = 2000
local NAME_MAX = protocol.INBOX_SESSION_MAX_BYTES
local SEQ_FORMAT = "%s/%0" .. protocol.INBOX_SEQ_DIGITS .. "d"

---Every touch of the disk goes through here, so a test runs the transport on a fake.
M.fs = { open = io.open, rename = os.rename, remove = os.remove }

---Declared through `store`, so a forgotten pane takes its session with it.
local scope = store.scope "transport"
local sessions = scope.pane()

-- required late: identity routes its frames through this module
local function identity()
  return require "vtabs.sidebar_identity"
end

---Frames to a pane on a unix domain of this machine may take the inbox; everything else keeps
---`send_text`. The place recorded at attach names the host a domain was proxied to, if any.
function M.eligible(pane)
  if config.get().backend.inbox == false then
    return false
  end
  local domain = mux.domain(pane)
  if type(domain) ~= "string" or domain == "local" then
    return false
  end
  local place = store.pane_domain[pane:pane_id()]
  local host = place and place:match "^[^@]*@(.*)$" or nil
  return backend.is_local(domain, host ~= "" and host or nil)
end

function M.state(pane)
  local session = pane and sessions[pane:pane_id()] or nil
  return session and session.state or "off"
end

---The pane's session, for probes and tests.
function M.inspect(pane)
  local session = pane and sessions[pane:pane_id()] or nil
  if not session then
    return { state = "off" }
  end
  return {
    state = session.state,
    session = session.id,
    dir = session.dir,
    next_seq = session.next_seq,
    queued = #session.queue,
  }
end

---One message: written whole to `<seq>.tmp`, then renamed into place, so the backend never reads a
---partial file. A message is delivered iff the rename happened; a failed one is never replayed.
local function put(session, text)
  local seq = session.next_seq
  session.next_seq = seq + 1
  local base = string.format(SEQ_FORMAT, session.dir, seq)
  local tmp = base .. ".tmp"
  local f = M.fs.open(tmp, "wb")
  if not f then
    return false
  end
  local written = pcall(function()
    assert(f:write(text))
    assert(f:flush())
  end)
  local closed = pcall(function()
    assert(f:close())
  end)
  if written and closed and M.fs.rename(tmp, base .. ".msg") then
    return true
  end
  M.fs.remove(tmp)
  return false
end

local function drop(session)
  local pid = session.pane:pane_id()
  if sessions[pid] == session then
    sessions[pid] = nil
  end
end

---Back to stdin: whatever waited goes there in order, then the backend is told when it could still
---be on its way to the directory, so a barrier still in flight cannot leave it reading one nobody
---writes to.
local function abandon(session, tell)
  drop(session)
  local queue = session.queue
  session.queue = {}
  if #queue > 0 then
    identity().type_text(session.pane, table.concat(queue))
  end
  if tell then
    identity().send_stdin(session.pane, { t = "transport_stop", session = session.id })
  end
end

---The negotiation is bounded: an answer that has not come inside `NEGOTIATE_MS` is not waited on.
local function arm(session, ms)
  wezterm.time.call_after(ms / 1000, function()
    if sessions[session.pane:pane_id()] ~= session or session.state ~= "negotiating" then
      return
    end
    local left = M.NEGOTIATE_MS - (util.now_ms() - session.at)
    if left > 0 then
      arm(session, left)
      return
    end
    util.log("inbox %s: pane %d did not answer in time; frames stay on stdin", session.id, session.pane:pane_id())
    abandon(session, true)
  end)
end

---Only ever a directory right under the root: no separator, no dot, nothing the shell expands.
local function safe_name(name)
  return type(name) == "string" and #name <= NAME_MAX and name:match "^[%w%-]+$" ~= nil
end

---Starts negotiating the inbox a `ready` announced: the probe is message 1, then the barrier goes
---on stdin behind the `auth` that preceded it, so the backend scans with the token it now knows.
---A `ready` reset the backend, so a session this pane already had is dropped first, queue and all:
---the wire republishes everything after a `ready`.
function M.offer(pane, ready)
  local pid = pane:pane_id()
  sessions[pid] = nil
  local announced = type(ready) == "table" and type(ready.transport) == "table" and ready.transport.inbox or nil
  if announced == nil or not M.eligible(pane) then
    return false
  end
  local root = util.runtime_dir()
  if not root then
    return false
  end
  if not safe_name(announced) then
    util.warn_once("inbox-name-" .. pid, "pane %d announced an unusable inbox; frames stay on stdin", pid)
    return false
  end
  local session = {
    pane = pane,
    id = announced,
    dir = root .. "/" .. announced,
    state = "negotiating",
    next_seq = 1,
    queue = {},
    at = util.now_ms(),
  }
  sessions[pid] = session
  local probe = identity().frame(pane, wezterm.json_encode { t = "transport_probe", session = announced })
  if not probe or not put(session, probe) then
    drop(session)
    return false
  end
  if not identity().send_stdin(pane, { t = "transport_barrier", session = announced }) then
    drop(session)
    return false
  end
  -- the barrier may have been answered on the spot; only a session still waiting needs the clock
  if sessions[pid] == session and session.state == "negotiating" then
    arm(session, M.NEGOTIATE_MS)
  end
  return true
end

---One batch into the inbox. A failure loses that batch for good, stops the transport, and has the
---wire resend every section, since the backend's view is no longer known.
function M.write(pane, text)
  local pid = pane:pane_id()
  local session = sessions[pid]
  if not session or session.state ~= "active" then
    return false
  end
  if put(session, text) then
    return true
  end
  util.warn("inbox %s: write failed; pane %d back on stdin", session.id, pid)
  abandon(session, true)
  require("vtabs.wire").reset_pane(pid)
  return false
end

---Holds a batch while the inbox is negotiated; past the cap the negotiation is given up and the
---queue typed, in order, so nothing waits on an answer that may not come.
function M.enqueue(pane, text)
  local session = sessions[pane:pane_id()]
  if not session or session.state ~= "negotiating" then
    return false
  end
  session.queue[#session.queue + 1] = text
  if #session.queue > M.QUEUE_MAX then
    util.log("inbox %s: %d frames waiting on pane %d; back on stdin", session.id, #session.queue, pane:pane_id())
    abandon(session, true)
  end
  return true
end

---`transport_ready` for the session being negotiated: from here every batch is a file, the ones
---that waited first.
function M.accept(pane, ev)
  local pid = pane:pane_id()
  local session = sessions[pid]
  if not session or session.state ~= "negotiating" or type(ev) ~= "table" or ev.session ~= session.id then
    return false
  end
  session.state = "active"
  -- a write that fails takes the rest of the queue to stdin with it
  while #session.queue > 0 do
    if not M.write(pane, table.remove(session.queue, 1)) then
      return false
    end
  end
  return true
end

---`transport_refused` for a session of this pane: the backend reads stdin only, so the queue goes
---there in order. A refusal of a live session is a loss, and the wire resends everything.
function M.refuse(pane, ev)
  local pid = pane:pane_id()
  local session = sessions[pid]
  if not session or type(ev) ~= "table" or ev.session ~= session.id then
    return false
  end
  local was_active = session.state == "active"
  abandon(session, false)
  if was_active then
    require("vtabs.wire").reset_pane(pid)
  end
  return true
end

---Leaves the transport on purpose: the backend drains what is already in the directory, then reads
---stdin only.
function M.stop(pane)
  local session = sessions[pane:pane_id()]
  if not session then
    return false
  end
  abandon(session, true)
  return true
end

---The pane is closing, or already gone: nothing is sent, nothing is kept.
function M.forget(pane_id)
  sessions[pane_id] = nil
end

return M

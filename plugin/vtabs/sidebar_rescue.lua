local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local store = require "vtabs.store"
local platform = require "vtabs.platform"
local mux = require "vtabs.mux"
local util = require "vtabs.util"
local identity = require "vtabs.sidebar_identity"

---Putting a drifted session right: pings and the restart of a silent backend, a pane split into the
---sidebar's column walked back to the content side, and the pins, windows and tabs a mux left behind.
local M = {}

local PING_AFTER_MS = 8000
local DEAD_AFTER_MS = 20000
local PRUNE_MS = 30000
local PIN_GRACE_MS = 3000

---The GUI overwrites WEZTERM_UNIX_SOCKET with its own gui-sock-<pid> (wezterm-gui/src/main.rs:413-415).
local function own_socket()
  local socket = util.getenv "WEZTERM_UNIX_SOCKET"
  local pid = util.try(function()
    return wezterm.procinfo.pid()
  end)
  if type(socket) ~= "string" or not pid then
    return false
  end
  return socket:match("gui%-sock%-" .. tostring(pid) .. "$") ~= nil
end

---Runs `wezterm cli` against the GUI's own socket, so no tab or pane has to be activated first.
---False when the CLI is not usable here, which is the caller's cue to fall back.
local function cli(args, key, unavailable)
  local dir = wezterm.executable_dir
  if type(dir) == "string" and own_socket() then
    local argv = { dir .. (platform.is_windows and "\\wezterm.exe" or "/wezterm"), "cli", "--no-auto-start" }
    for _, arg in ipairs(args) do
      argv[#argv + 1] = arg
    end
    if util.try(wezterm.run_child_process, argv) == true then
      return true
    end
  end
  util.warn_once(key, "%s", unavailable)
  return false
end

function M.cli_kill(pane_id)
  local args = { "kill-pane", "--pane-id", tostring(pane_id) }
  return cli(args, "cli-kill", "wezterm cli kill-pane unavailable; closing sidebars by activation")
end

---Moves a pane under `target`, splitting it downwards. `--move-pane-id` relocates an existing pane.
local function cli_move(pane_id, target_id)
  local args = { "split-pane", "--move-pane-id", tostring(pane_id), "--pane-id", tostring(target_id), "--bottom" }
  return cli(args, "cli-move", "wezterm cli split-pane --move-pane-id unavailable; a split sidebar is left as is")
end

---Panes in the sidebar's column band. The band is the width the sidebar is *meant* to have, never
---the one it currently reports: a `SplitHorizontal` halves the sidebar's own box, which drags its
---observed edge left of the pane that just landed beside it, so the one split shape that most needs
---rescuing is the one an observed edge cannot see.
---`panes_with_info` reports the unzoomed layout (`mux/src/tab.rs:88`), so zoom does not enter it.
local function intruders(tab, sb, position, band)
  local infos = mux.panes_with_info(tab)
  if type(infos) ~= "table" or not band or band < 1 then
    return {}
  end
  local tab_cols = 0
  local ours = false
  for _, info in ipairs(infos) do
    tab_cols = math.max(tab_cols, (info.left or 0) + (info.width or 0))
    ours = ours or info.pane:pane_id() == sb:pane_id()
  end
  if not ours then
    return {}
  end
  local out = {}
  for _, info in ipairs(infos) do
    local left = info.left or 0
    local inside
    if position == "right" then
      inside = left + (info.width or 0) >= tab_cols - band
    else
      inside = left <= band
    end
    if info.pane:pane_id() ~= sb:pane_id() and inside then
      out[#out + 1] = info.pane
    end
  end
  return out
end

---The destination is a real shell of this tab: never an overlay, never a pane of this backend's own.
local function may_host(pane)
  return not identity.is_backend(pane) and not identity.is_settings(pane) and not identity.is_overlay(pane)
end

---WezTerm splits whichever pane is active, and under `hover = "follow"` that is often the sidebar,
---which leaves a shell in a column too narrow to use. Move it to the content side instead.
function M.rescue_splits(gui_window, tab)
  local content, sb = identity.classify(tab)
  -- A pane that only claims the role by its title must never decide that another one moves.
  if not sb or not identity.is_ready(sb) or #content < 2 then
    return false
  end
  local geometry = require "vtabs.geometry"
  local stuck = intruders(tab, sb, config.get().position, geometry.desired(gui_window:window_id()))
  if #stuck == 0 then
    return false
  end
  local inside = {}
  for _, pane in ipairs(stuck) do
    inside[pane:pane_id()] = true
  end
  local host = nil
  for _, pane in ipairs(content) do
    if not inside[pane:pane_id()] and may_host(pane) then
      host = pane
      break
    end
  end
  if not host then
    return false
  end
  local moved = false
  for _, pane in ipairs(stuck) do
    moved = cli_move(pane:pane_id(), host:pane_id()) or moved
  end
  if moved then
    identity.forget_split(tab:tab_id())
    util.try(geometry.correct, gui_window)
  end
  return moved
end

---Pings idle sidebars; replaces one whose backend stopped answering.
function M.check_liveness(gui_window, tab, sb, now)
  local pid = sb:pane_id()
  local seen = store.seen[pid] or now
  store.seen[pid] = seen
  local idle = now - seen
  if idle > DEAD_AFTER_MS then
    util.warn("sidebar %d unresponsive, restarting", pid)
    -- required here: the poll that calls this lives in attach, so a load-time require would cycle
    require("vtabs.sidebar_attach").detach(gui_window, tab)
    return false
  end
  if idle > PING_AFTER_MS and (store.pinged[pid] or 0) < seen then
    store.pinged[pid] = now
    identity.send(sb, { t = "ping", n = now })
  end
  return true
end

function M.record_closed_tabs(wid, seen, private)
  local previous = store.known_tabs[wid] or {}
  for tab_id in pairs(previous) do
    if not seen[tab_id] then
      local meta = store.tab_meta[tab_id]
      if meta and not store.moving[tab_id] and not private then
        state.push_closed(meta)
      end
      local pid = state.sidebar_pane_id(tab_id)
      if pid then
        state.forget_pane(pid)
      end
      state.forget_tab(tab_id)
    end
  end
  store.known_tabs[wid] = seen
end

local pins_deadline = nil

---Tab ids only mean something while the mux that minted them lives; a surviving backend pane proves it.
function M.resolve_pins(tabs, now)
  if not state.pins_pending() then
    return
  end
  for _, info in ipairs(tabs) do
    for _, p in ipairs(info.tab:panes()) do
      if identity.has_marker(p) then
        state.restore_pins()
        pins_deadline = nil
        return
      end
    end
  end
  pins_deadline = pins_deadline or now + PIN_GRACE_MS
  if now > pins_deadline then
    state.discard_pins()
    pins_deadline = nil
  end
end

local pruned_at = 0

function M.prune_windows(now)
  if now - pruned_at < PRUNE_MS then
    return
  end
  pruned_at = now
  local windows = mux.all_windows()
  if type(windows) ~= "table" or #windows == 0 then
    return
  end
  local live = {}
  for _, w in ipairs(windows) do
    local id = mux.window_id(w)
    if id then
      live[id] = true
    end
  end
  state.forget_windows_except(live)
end

return M

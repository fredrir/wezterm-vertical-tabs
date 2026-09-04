local wezterm = require "wezterm" ---@type Wezterm
local config = require "vtabs.config"
local state = require "vtabs.state"
local store = require "vtabs.store"
local sidebar = require "vtabs.sidebar"
local view = require "vtabs.view"
local geometry = require "vtabs.geometry"
local actions = require "vtabs.actions"
local popover = require "vtabs.popover"
local protocol = require "vtabs.gen.protocol"
local mux = require "vtabs.mux"
local link = require "vtabs.link"
local theme_bridge = require "vtabs.theme_bridge"
local transport = require "vtabs.transport"
local util = require "vtabs.util"

local M = {}

local DRAG_TIMEOUT_MS = 3000
local READY_KEYS = { t = true, cols = true, rows = true, pane = true, transport = true, n = true }

local function whole(value, positive)
  return type(value) == "number" and value == math.floor(value) and value >= (positive and 1 or 0) and value or nil
end

local function current_ready(ev)
  for key in pairs(ev) do
    if READY_KEYS[key] ~= true then
      return nil
    end
  end
  if ev.n ~= nil and not whole(ev.n, false) then
    return nil
  end
  if ev.transport ~= nil then
    if type(ev.transport) ~= "table" or type(ev.transport.inbox) ~= "string" then
      return nil
    end
    for key in pairs(ev.transport) do
      if key ~= "inbox" then
        return nil
      end
    end
  end
  return whole(ev.pane, false), whole(ev.cols, true), whole(ev.rows, true)
end

---Runs the strip button under the pointer, through the one dispatch table every other caller reads.
---A `hooked` button is the user's to point elsewhere, so its hook gets first refusal; the rest are
---the strip's own controls. A table entry in `strip_actions` carries its own callback.
local function strip_action(gui_window, id)
  if id == nil then
    return
  end
  local button = nil
  for _, entry in ipairs(actions.strip) do
    button = entry.id == id and entry or button
  end
  if button and not button.hooked then
    return actions.run(button.action, gui_window)
  end
  local cfg = config.get()
  local hook = (cfg.hooks or {})[id]
  if type(hook) == "function" then
    return util.try(hook, gui_window)
  end
  if button and button.action then
    return actions.run(button.action, gui_window)
  end
  for _, entry in ipairs(cfg.strip_actions or {}) do
    if type(entry) == "table" and (entry.id or "custom") == id and type(entry.on_click) == "function" then
      return util.try(entry.on_click, gui_window)
    end
  end
end
local scope = store.scope "input"

local function blur(gui_window)
  actions.blur_sidebar(gui_window)
end

local INTENT = {}

function INTENT.press_card(gui_window, pane, ev)
  local wid = gui_window:window_id()
  local now = util.now_ms()
  store.drag[wid] = nil
  state.set_focus(wid, false)
  local focused = actions.activate_tab(gui_window, ev.tab_id, "sidebar")
  store.drag[wid] = {
    tab_id = ev.tab_id,
    origin_x = ev.x,
    origin_y = ev.y,
    pane_id = focused and focused:pane_id() or pane:pane_id(),
    active = false,
    began = now,
    at = now,
  }
end

function INTENT.drag_to(gui_window, _, ev)
  local drag = store.drag[gui_window:window_id()]
  if not drag then
    return
  end
  drag.at = util.now_ms()
  drag.active = true
  drag.over_index = ev.slot
  drag.outside = ev.outside == true
end

function INTENT.drag_end(gui_window, _, ev)
  local wid = gui_window:window_id()
  local drag = store.drag[wid]
  store.drag[wid] = nil
  if not drag then
    return
  end
  if ev.outside or drag.outside then
    actions.tear_off(gui_window, drag.tab_id)
  elseif ev.slot or drag.over_index then
    actions.move_tab_to_slot(gui_window, drag.tab_id, ev.slot or drag.over_index)
  end
end

function INTENT.request_close(gui_window, _, ev)
  actions.request_close(gui_window, ev.tab_id, ev.row, ev.col, ev.from_key == true)
end

function INTENT.toggle_pin(gui_window, _, ev)
  actions.toggle_pin(gui_window, ev.tab_id)
end

function INTENT.open_menu(gui_window, _, ev)
  popover.open(gui_window, ev.tab_id, ev.row, ev.col)
end

function INTENT.new_tab(gui_window)
  actions.new_tab(gui_window)
end

function INTENT.strip(gui_window, _, ev)
  strip_action(gui_window, ev.button_id)
end

function INTENT.set_scroll(gui_window, _, ev)
  local wid = gui_window:window_id()
  store.scroll[wid] = ev.top or 0
  store.user_scrolled[wid] = ev.user == true or nil
end

function INTENT.wheel_tab(gui_window, _, ev)
  actions.activate_relative(gui_window, ev.dy or 1)
end

function INTENT.set_focus_index(gui_window, _, ev)
  store.focus_index[gui_window:window_id()] = ev.index
end

function INTENT.activate_tab(gui_window, _, ev)
  actions.activate_tab(gui_window, ev.tab_id)
  blur(gui_window)
end

function INTENT.blur_sidebar(gui_window)
  blur(gui_window)
end

function INTENT.menu_pick(gui_window, _, ev)
  popover.run(gui_window, ev.item_id)
end

function INTENT.switch_space(gui_window, _, ev)
  if type(ev.space_id) == "string" then
    actions.switch_space(gui_window, ev.space_id)
  end
end

function INTENT.menu_back(gui_window)
  popover.back(gui_window)
end

function INTENT.menu_closed(gui_window)
  popover.close(gui_window)
end

function INTENT.rename_commit(gui_window, _, ev)
  local pop = popover.get(gui_window:window_id())
  if pop and pop.level == "rename" then
    pop.buffer = tostring(ev.text or "")
    popover.commit_rename(gui_window)
  end
end

---Footer entries can be id-less closures, so the wire addresses them by index in the sent model.
function INTENT.footer(gui_window, _, ev)
  local hook = (config.get().hooks or {}).footer
  if type(hook) ~= "function" then
    return
  end
  local ok, rows = pcall(hook, gui_window:mux_window())
  local entry = ok and type(rows) == "table" and rows[ev.index] or nil
  if type(entry) == "table" and type(entry.on_click) == "function" then
    pcall(entry.on_click, gui_window, entry)
  end
end

function INTENT.rename_tab(gui_window, _, ev)
  actions.rename_tab(gui_window, ev.tab_id)
end

function INTENT.move_tab(gui_window, _, ev)
  actions.move_tab_to_slot(gui_window, ev.tab_id, ev.slot)
  store.focus_index[gui_window:window_id()] = ev.focus_index
end

function INTENT.set_rail_reserve(gui_window, pane, ev)
  local active = util.active_tab(gui_window)
  local current = active and sidebar.find(active) or nil
  if not current or current:pane_id() ~= pane:pane_id() then
    return
  end
  geometry.apply_rail_reserve(gui_window, ev.cols)
end

M.INTENT = INTENT

local QUIET_INTENT = { set_scroll = true, drag_to = true }

local MENU_INTENT = {
  menu_pick = true,
  menu_back = true,
  menu_closed = true,
  rename_commit = true,
}

---A menu level the backend could not draw: a refused confirm falls through to WezTerm's own
---overlay for a close, and any other refused level simply closes, so the pane never deadlocks.
local function on_menu_refused(gui_window, ev)
  local wid = gui_window:window_id()
  local pop = popover.get(wid)
  popover.close(gui_window)
  if pop and ev.level == "confirm" and pop.confirm == "close" then
    actions.close_with_overlay(gui_window, pop.tab_id)
  end
  view.sync(gui_window)
end

---The outcome of a `kill`, `rescue` or `adjust` the backend ran on its server. A moved pane
---re-splits the tab and the width is corrected around it; an adjust answers with the width it
---left the pane at, which releases the wait even when no resize followed it; a failure is said
---once and the next poll goes on.
local function on_cli(gui_window, pane, ev)
  if ev.ok ~= true then
    util.warn_once(
      "cli-" .. tostring(ev.op) .. "-" .. pane:pane_id(),
      "backend %s failed: %s",
      tostring(ev.op),
      tostring(ev.detail)
    )
    if ev.op == "adjust" then
      geometry.abandon(gui_window:window_id())
    end
    return
  end
  if ev.op == "adjust" and tonumber(ev.cols) then
    geometry.landed(gui_window:window_id(), pane:pane_id(), tonumber(ev.cols), nil, true)
    geometry.correct(gui_window)
  elseif ev.op == "rescue" then
    local tab = mux.tab_of(pane)
    if tab then
      sidebar.forget_split(tab:tab_id())
    end
    util.try(geometry.correct, gui_window, true)
  end
end

local function on_theme_resolved(gui_window, ev)
  if theme_bridge.accept(gui_window, ev) then
    view.sync(gui_window)
  end
end

---Window-global space/theme state has one stable identity-based writer. Background panes still
---answer hooks and paint their own committed state, but their delayed results cannot replace the
---designated pane's host projection.
local function is_shared_result_authority(gui_window, pane)
  local tab = mux.tab_of(pane)
  local pane_window = tab and mux.call(tab, "window") or nil
  local wid = gui_window:window_id()
  return mux.window_id(pane_window) == wid and require("vtabs.wire").is_policy_authority(wid, pane:pane_id())
end

---The backend's answer to the inbox handshake, for the session Lua is negotiating.
local TRANSPORT = { transport_ready = transport.accept, transport_refused = transport.refuse }

---An inbox message the backend never received is one publish lost: the wire resends every section
---and the caller publishes. Any other drop is the backend refusing a bound, said once.
local function on_dropped(pane, ev)
  local pid = pane:pane_id()
  if ev.what == "message" then
    util.log("backend %d lost inbox message %s; resending", pid, tostring(ev.seq))
    require("vtabs.wire").reset_pane(pid)
    return true
  end
  if ev.what == "sync" then
    require("vtabs.wire").reset_pane(pid)
    if ev.reason == "busy" then
      util.log("backend %d is finishing its pending publication; marked for retry", pid)
      return false
    end
  end
  util.warn_once(
    string.format("backend-drop-%d-%s-%s", pid, tostring(ev.what), tostring(ev.reason)),
    "backend dropped %s: %s",
    tostring(ev.what),
    tostring(ev.reason)
  )
  return false
end

---Typed backend intent; the popover-open guard prevents click-through dismiss.
local function on_intent(gui_window, pane, ev)
  if protocol.INTENT_NAMES[ev.a] ~= true then
    return
  end
  local handler = INTENT[ev.a]
  if not handler then
    return
  end
  local wid = gui_window:window_id()
  if popover.get(wid) and not MENU_INTENT[ev.a] then
    popover.close(gui_window)
  end
  handler(gui_window, pane, ev)
  if not QUIET_INTENT[ev.a] then
    view.sync(gui_window)
  end
end

local FORWARD_MAX_RAW = protocol.FORWARDED_KEY_MAX_ENCODED_BYTES
local FORWARD_MAX_BYTES = protocol.FORWARDED_KEY_MAX_BYTES
local FORWARD_BURST = 20
local FORWARD_PER_SEC = 60
local PASTE_MAX_RAW = protocol.PASTE_MAX_ENCODED_BYTES
local PASTE_MAX_BYTES = protocol.PASTE_MAX_BYTES
local BUDGET_TTL_MS = 60000
local budget = scope.pane()

---Token bucket per source pane, charged by size, so one big paste borrows against the next second.
local function affordable(pid, now, cost)
  local b = budget[pid]
  if not b then
    b = { tokens = FORWARD_BURST, at = now }
    budget[pid] = b
  end
  b.tokens = math.min(FORWARD_BURST, b.tokens + (now - b.at) * FORWARD_PER_SEC / 1000)
  b.at = now
  if b.tokens < 1 then
    return false
  end
  b.tokens = b.tokens - cost
  return true
end

-- ESC-prefixed key shapes: CSI (params, intermediates, final 0x40-0x7e), SS3, and the alt-key form.
local KEY_SHAPES = { "^\27%[[\48-\63]*[\32-\47]*[\64-\126]$", "^\27O.$", "^\27.$" }
local PASTE_BRACKETS = { "\27[200~", "\27[201~" }

---`text` when it is structurally one key press, else nil.
function M.safe_key_bytes(text)
  if type(text) ~= "string" or text == "" or #text > FORWARD_MAX_BYTES then
    return nil
  end
  for _, bracket in ipairs(PASTE_BRACKETS) do
    if text:find(bracket, 1, true) then
      return nil
    end
  end
  if utf8.len(text) == 1 then
    return text
  end
  for _, shape in ipairs(KEY_SHAPES) do
    if text:find(shape) then
      return text
    end
  end
  return nil
end

local function decoded_key(ev)
  if type(ev.raw) == "string" then
    return #ev.raw <= FORWARD_MAX_RAW and util.base64_decode(ev.raw) or nil
  end
  local key = ev.key
  if type(key) ~= "string" or utf8.len(key) ~= 1 then
    return nil
  end
  if util.contains(ev.mods, "ctrl") or util.contains(ev.mods, "alt") then
    return nil
  end
  local code = utf8.codepoint(key)
  return code >= 32 and code ~= 127 and key or nil
end

---The content pane a hovered sidebar may hand input to, or nil when any part of that claim fails.
local function handover_target(gui_window, pane, cfg)
  local wid = gui_window:window_id()
  local tab = mux.tab_of(pane)
  local active = util.active_tab(gui_window)
  if not sidebar.is_ready(pane) or not tab or not active or tab:tab_id() ~= active:tab_id() then
    return nil
  end
  if cfg.hover == "press" and not store.drag[wid] then
    return nil
  end
  local content = sidebar.content_pane(tab)
  if not content or content:pane_id() == pane:pane_id() then
    return nil
  end
  if sidebar.is_backend(content) or sidebar.is_settings(content) or sidebar.is_overlay(content) then
    return nil
  end
  local domain = mux.domain(pane)
  if domain == nil or domain ~= mux.domain(content) then
    return nil
  end
  return content
end

---Focus follows the handover whether or not bytes went with it, so no second key is lost the same way.
local function hand_over(gui_window, content, deliver)
  if deliver then
    pcall(deliver)
  end
  content:activate()
  state.set_focus(gui_window:window_id(), false)
end

---A key at a sidebar that is not in keyboard mode is the user typing at their shell: hand it over.
---`delivered` means the backend already typed the bytes into the content pane through its own
---server, so only the focus is left to move.
local function forward_key(gui_window, pane, ev, cfg)
  local content = handover_target(gui_window, pane, cfg)
  if not content then
    return
  end
  if ev.delivered == true then
    hand_over(gui_window, content, nil)
    return
  end
  local text = M.safe_key_bytes(decoded_key(ev))
  local pid = pane:pane_id()
  if text and not affordable(pid, util.now_ms(), 1) then
    if cfg.debug then
      util.log("key forward over budget on pane %d", pid)
    end
    text = nil
  end
  hand_over(gui_window, content, text and function()
    content:send_text(text)
  end)
end

---A bracketed paste captured by the backend, delivered whole so the shell brackets it once.
local function forward_paste(gui_window, pane, ev, cfg)
  local content = handover_target(gui_window, pane, cfg)
  if not content then
    return
  end
  local text = nil
  if type(ev.data) == "string" and #ev.data <= PASTE_MAX_RAW then
    text = util.base64_decode(ev.data)
  end
  if text == "" or (text and #text > PASTE_MAX_BYTES) then
    text = nil
  end
  if text and not affordable(pane:pane_id(), util.now_ms(), 1 + #text // 1024) then
    text = nil
  end
  hand_over(gui_window, content, text and function()
    content:paste(text)
  end)
end

function M.key(gui_window, pane, ev)
  local wid = gui_window:window_id()
  if state.has_focus(wid) then
    return
  end
  forward_key(gui_window, pane, ev, config.get())
  view.sync(gui_window)
end

-- A backend answers an auth it cannot read by publishing the token it holds; the plugin frames
-- its next one with that, this many times per pane, so two plugins on one mux cannot trade a
-- pane back and forth forever.
local TOKEN_AUTHS_MAX = 5

---The token var of a pane this process mapped but has not authenticated: the backend the mux
---kept has published the session it holds, which a GUI that just attached had no user vars for.
---Framing the auth with it is what the backend asked for; the value itself is never trusted, and
---one the plugin minted is the echo `is_ready` reads for itself.
local function on_token(_, pane, value)
  local pid = pane:pane_id()
  local token = state.token_for(pid)
  if type(value) ~= "string" or value == "" or token == nil or value == token then
    return
  end
  if store.ready[pid] or store.given_up[pid] or store.quitting[pid] or state.pane_for_token(value) ~= nil then
    return
  end
  local tries = (store.token_auths[pid] or 0) + 1
  if tries > TOKEN_AUTHS_MAX then
    return
  end
  store.token_auths[pid] = tries
  if config.get().debug then
    util.log("pane %d holds another session; re-authenticating with it", pid)
  end
  sidebar.auth(pane, value)
end

---Entry point for the `user-var-changed` event; only registered sidebar panes are trusted.
function M.handle(gui_window, pane, name, value)
  local cfg = config.get()
  if name == sidebar.TOKEN_VAR then
    return on_token(gui_window, pane, value)
  end
  if name ~= cfg.backend.uservar or not sidebar.is_ready(pane) then
    return
  end
  local ok, ev = pcall(wezterm.json_parse, value)
  if cfg.debug then
    util.log("event %s parsed=%s", tostring(value), tostring(ok))
  end
  if not ok or type(ev) ~= "table" then
    return
  end
  store.seen[pane:pane_id()] = util.now_ms()
  if cfg.debug then
    util.log("handle: %s from pane %d", tostring(ev.t), pane:pane_id())
  end

  if not sidebar.is_settings(pane) then
    local tab = mux.tab_of(pane)
    local current = tab and sidebar.find(tab) or nil
    if not current or current:pane_id() ~= pane:pane_id() then
      return
    end
  end

  local function accept_ready()
    local server, cols, rows = current_ready(ev)
    if not server or not cols or not rows then
      sidebar.refuse_ready(pane)
      return false
    end
    local pid = pane:pane_id()
    -- the backend's own id on its server: what `kill` by id names through another backend there
    store.server_pane[pid] = server
    local wire = require "vtabs.wire"
    wire.reset_pane(pid)
    wire.policy_ready(gui_window:window_id(), pid)
    sidebar.auth(pane)
    transport.offer(pane, ev)
    return true
  end

  if sidebar.is_settings(pane) then
    -- A retired or duplicate settings process may deliver buffered user vars after a replacement
    -- has authenticated. Only the pane currently present in this window's settings tab may mutate
    -- configuration, run a theme hook, or update the host theme projection.
    local _, current = require("vtabs.settings").find(gui_window:mux_window())
    if not current or current:pane_id() ~= pane:pane_id() then
      return
    end
    if ev.t == "ready" then
      if not accept_ready() then
        return
      end
      view.sync(gui_window)
    elseif require("vtabs.settings_model").effect(gui_window, ev) then
      view.sync(gui_window)
    elseif ev.t == "theme_hook_request" then
      theme_bridge.answer_hook(gui_window, pane, ev)
    elseif ev.t == "space_route_hook_request" then
      require("vtabs.spaces").answer_hook(gui_window, pane, ev)
    elseif ev.t == "spaces_resolved" then
      local wid = gui_window:window_id()
      if is_shared_result_authority(gui_window, pane) and require("vtabs.spaces").accept(ev, wid) then
        view.sync(gui_window)
      end
    elseif ev.t == "theme_resolved" then
      if is_shared_result_authority(gui_window, pane) then
        on_theme_resolved(gui_window, ev)
      end
    elseif TRANSPORT[ev.t] then
      TRANSPORT[ev.t](pane, ev)
    elseif ev.t == "dropped" then
      if on_dropped(pane, ev) then
        view.sync(gui_window)
      end
    end
    return
  end
  if ev.t == "ready" then
    if not accept_ready() then
      return
    end
    sidebar.ensure(gui_window)
    view.sync(gui_window)
  elseif ev.t == "resize" then
    -- The sidebar reporting its own size is the one per-frame signal a divider drag gives, and on
    -- a mux domain the server's own word on the width: the answer to the adjust in flight, and
    -- the width the next correction reads, over a mirror that can lag. Nothing is published for
    -- it: the pane has already repainted itself at the new size, and a publish per report is a
    -- publication per frame across every tab. From a mux pane it is a server-side resize, the
    -- visible edge of the mirror rebuild storm nothing may cross the link into, and one more
    -- frame of the burst no adjust is computed inside.
    local wid = gui_window:window_id()
    link.activity(pane)
    if mux.domain(pane) ~= "local" then
      geometry.on_server_resize(wid)
    end
    geometry.landed(wid, pane:pane_id(), tonumber(ev.cols), tonumber(ev.rows))
    geometry.correct(gui_window)
  elseif ev.t == "theme_hook_request" then
    theme_bridge.answer_hook(gui_window, pane, ev)
  elseif ev.t == "space_route_hook_request" then
    require("vtabs.spaces").answer_hook(gui_window, pane, ev)
  elseif ev.t == "spaces_resolved" then
    local wid = gui_window:window_id()
    if is_shared_result_authority(gui_window, pane) and require("vtabs.spaces").accept(ev, wid) then
      view.sync(gui_window)
    end
  elseif ev.t == "theme_resolved" then
    if is_shared_result_authority(gui_window, pane) then
      on_theme_resolved(gui_window, ev)
    end
  elseif TRANSPORT[ev.t] then
    TRANSPORT[ev.t](pane, ev)
  elseif ev.t == "dropped" then
    if on_dropped(pane, ev) then
      view.sync(gui_window)
    end
  elseif ev.t == "intent" then
    on_intent(gui_window, pane, ev)
  elseif ev.t == "cli" then
    on_cli(gui_window, pane, ev)
  elseif ev.t == "menu_refused" then
    on_menu_refused(gui_window, ev)
  elseif ev.t == "key" then
    M.key(gui_window, pane, ev)
  elseif ev.t == "paste" then
    forward_paste(gui_window, pane, ev, cfg)
    view.sync(gui_window)
  end
end

---Expires a stale drag; called from the status poll.
function M.tick(gui_window)
  local cfg = config.get()
  local wid = gui_window:window_id()
  local now = util.now_ms()
  local drag = store.drag[wid]
  if drag and now - drag.at > DRAG_TIMEOUT_MS then
    store.drag[wid] = nil
    if cfg.hover == "press" then
      blur(gui_window)
    end
  end
  for pid, b in pairs(budget) do
    if now - b.at > BUDGET_TTL_MS then
      budget[pid] = nil
    end
  end
  local active = util.active_tab(gui_window)
  local active_id = active and active:tab_id() or nil
  if store.last_active[wid] ~= active_id then
    store.last_active[wid] = active_id
    store.user_scrolled[wid] = nil
  end
  -- `hover` or a tab's content can change under a backend delivering keys on the server; a
  -- same-token auth moves the keys back to the plugin, or out to the server, without a reset.
  for _, info in ipairs(mux.call(gui_window, "mux_window"):tabs_with_info() or {}) do
    local sb = sidebar.find(info.tab)
    local pid = sb and sb:pane_id() or nil
    if pid and store.keys_mode[pid] and sidebar.is_ready(sb) then
      if (sidebar.keys_mode(sb) or "host") ~= store.keys_mode[pid] then
        sidebar.auth(sb)
      end
    end
  end
end

return M

local H = require "support.helpers"
local util = require "vtabs.util"
local config = require "vtabs.config"
local theme = require "vtabs.theme"
local state = require "vtabs.state"
local sidebar = require "vtabs.sidebar"
local actions = require "vtabs.actions"
local input = require "vtabs.input"
local geometry = require "vtabs.geometry"
local popover = require "vtabs.popover"

local test, eq, rgb, title_row, palette = H.test, H.eq, H.rgb, H.title_row, H.palette
local later, mark_ready, mouse, press_row, window = H.later, H.mark_ready, H.mouse, H.press_row, H.window
local ready_window, key_window = H.ready_window, H.key_window

test("v2 do: press_card activates the tab and arms the drag at its origin", function()
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"press_card","id":' .. win.tab_list[3].id .. ',"args":{"x":5,"y":6}}')
  eq(win.active_tab_ref, win.tab_list[3], "press activates the card's tab")
  local drag = state.session.drag[gui:window_id()]
  eq(drag.tab_id, win.tab_list[3].id)
  eq(drag.origin_x, 5)
  eq(drag.active, false, "arming is not dragging")
end)

test("v2 do: drag_to then drag_end reorders through the same action v1 uses", function()
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  local id = win.tab_list[3].id
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"press_card","id":' .. id .. ',"args":{"x":5,"y":6}}')
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"drag_to","args":{"x":5,"y":3,"slot":1,"outside":false}}')
  eq(state.session.drag[gui:window_id()].over_index, 1)
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"drag_end","args":{"slot":1}}')
  eq(state.session.drag[gui:window_id()], nil, "the drag is spent")
  eq(win.tab_list[1].id, id, "the dragged tab landed on slot 1")
end)

test("v2 do: window mirrors land in the stores and blur clears focus", function()
  local win, gui = ready_window(12)
  local sb = sidebar.find(win.tab_list[1])
  local wid = gui:window_id()
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"set_scroll","args":{"top":4,"user":true}}')
  eq(state.session.scroll[wid], 4)
  eq(state.session.user_scrolled[wid], true)
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"set_focus_index","args":{"index":2}}')
  eq(state.session.focus_index[wid], 2)
  actions.focus_sidebar(gui)
  input.handle(gui, sb, "vtabs", '{"t":"do","a":"blur_sidebar"}')
  eq(state.has_focus(wid), false)
end)

test("v2 do: popover_mouse keeps v1's arming — destructive on release, scrim closes", function()
  local win, gui, sb = H.open_popover(3)
  local tabs_before = #win.tab_list
  input.handle(
    gui,
    sb,
    "vtabs",
    '{"t":"do","a":"popover_mouse","args":{"k":"down","b":"left","x":5,"y":7,"kind":"popover","id":"close","inside":true}}'
  )
  eq(#win.tab_list, tabs_before, "a destructive item does nothing on the press")
  input.handle(
    gui,
    sb,
    "vtabs",
    '{"t":"do","a":"popover_mouse","args":{"k":"up","b":"left","x":5,"y":7,"kind":"popover","id":"close","inside":true}}'
  )
  eq(#win.tab_list, tabs_before - 1, "and acts on the matching release")

  local win2, gui2, sb2 = H.open_popover(3)
  input.handle(
    gui2,
    sb2,
    "vtabs",
    '{"t":"do","a":"popover_mouse","args":{"k":"down","b":"left","x":2,"y":2,"kind":"scrim"}}'
  )
  eq(require("vtabs.popover").get(gui2:window_id()), nil, "a scrim press closes the menu")
  eq(#win2.tab_list, 3, "and closes nothing else")
end)

test("v2 paints: a painting pane gets no frames and no fades, but stays fresh", function()
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  local pid = sb:pane_id()
  state.session.paints[pid] = true
  local frames_before = 0
  for _, sent in ipairs(sb.sent) do
    if sent:find('"t":"frame"', 1, true) then
      frames_before = frames_before + 1
    end
  end
  require("vtabs.view").sync(gui, { force = true })
  local frames_after = 0
  for _, sent in ipairs(sb.sent) do
    if sent:find('"t":"frame"', 1, true) then
      frames_after = frames_after + 1
    end
  end
  eq(frames_after, frames_before, "no frame reaches a painting pane")
  eq(require("vtabs.view").animate(gui, "expand_in"), false, "no anim either")
end)

test("v2 do: the vocabulary is total against the spec", function()
  local expected = {
    "activate_tab_by_id",
    "blur_sidebar",
    "drag_end",
    "drag_to",
    "new_tab",
    "open_menu",
    "popover_mouse",
    "press_card",
    "request_close",
    "set_focus_index",
    "set_scroll",
    "strip",
    "toggle_pin",
    "wheel_tab",
  }
  local got = {}
  for name in pairs(input.DO) do
    got[#got + 1] = name
  end
  table.sort(got)
  eq(table.concat(got, ","), table.concat(expected, ","))
end)

test("ready records the backend's protocol version, keyed by pane", function()
  local win, gui = ready_window()
  local sb = sidebar.find(win.tab_list[1])
  input.handle(gui, sb, "vtabs", '{"t":"ready","v":1,"cols":28,"rows":24}')
  eq(state.session.proto[sb:pane_id()], 1)
end)

test("press keeps the sidebar of the clicked tab focused and points the drag at it", function()
  local win, gui = ready_window()
  local sb1 = sidebar.find(win.tab_list[1])
  local drag = press_row(gui, sb1, title_row(sb1, win.tab_list[3].id))
  eq(win.active_tab_ref, win.tab_list[3])
  eq(win.tab_list[3].active, sidebar.find(win.tab_list[3]), "sidebar holds focus, not the shell")
  eq(drag.pane_id, sidebar.find(win.tab_list[3]):pane_id())
end)

test("one row of drift never arms a drag; three rows plus the dwell reorders on release", function()
  local win, gui = ready_window()
  local sb1 = sidebar.find(win.tab_list[1])
  local ids = {}
  for i, t in ipairs(win.tab_list) do
    ids[i] = t.id
  end
  local from = title_row(sb1, ids[3])
  local onto = title_row(sb1, ids[1])
  press_row(gui, sb1, from)
  local sb3 = sidebar.find(win.tab_list[3])
  mouse(gui, sb3, "drag", "left", 5, from - 1)
  eq(state.session.drag[gui:window_id()].active, false, "one row is jitter")
  mouse(gui, sb3, "up", "left", 5, from - 1)
  eq(win.tab_list[3].id, ids[3], "order untouched")

  press_row(gui, sb1, from)
  mouse(gui, sb3, "drag", "left", 5, onto)
  assert(state.session.drag[gui:window_id()].active, "three rows arms the drag")
  mouse(gui, sb3, "up", "left", 5, onto)
  eq(win.tab_list[1].id, ids[3], "dragged tab took the first slot")
end)

test("a drag that starts before the dwell elapses is jitter", function()
  local win, gui = ready_window()
  local sb1 = sidebar.find(win.tab_list[1])
  press_row(gui, sb1, title_row(sb1, win.tab_list[3].id), "hold")
  local sb3 = sidebar.find(win.tab_list[3])
  mouse(gui, sb3, "drag", "left", 5, title_row(sb1, win.tab_list[1].id))
  eq(state.session.drag[gui:window_id()].active, false)
end)

test("drag events from a pane other than the drag origin are dropped", function()
  local win, gui = ready_window()
  local sb1 = sidebar.find(win.tab_list[1])
  press_row(gui, sb1, title_row(sb1, win.tab_list[3].id))
  local sb2 = sidebar.find(win.tab_list[2])
  mouse(gui, sb2, "drag", "left", 5, title_row(sb1, win.tab_list[1].id))
  eq(state.session.drag[gui:window_id()].active, false)
end)

test("a drag whose pane has no hit map is dropped instead of dropping at slot 1", function()
  local win, gui = ready_window()
  local sb1 = sidebar.find(win.tab_list[1])
  local drag = press_row(gui, sb1, 3)
  state.session.hits[sb1:pane_id()] = nil
  mouse(gui, sb1, "drag", "left", 5, 6)
  eq(drag.active, false)
  eq(drag.over_index, nil)
end)

test("right click opens the popover on release, never while the button is held", function()
  local win, gui = ready_window()
  local sb1 = sidebar.find(win.tab_list[1])
  local before = #win.actions
  mouse(gui, sb1, "down", "right", 5, 4)
  eq(popover.get(gui:window_id()), nil, "nothing opens under a held button")
  eq(#win.actions, before, "and no overlay action is performed, ever")
  mouse(gui, sb1, "up", "right", 5, 4)
  local pop = popover.get(gui:window_id())
  assert(pop, "the release opens it")
  eq(pop.tab_id, win.tab_list[1].id)
  eq(pop.anchor_row, 4, "anchored on the row the press landed on")
  eq(#win.actions, before, "still no overlay: it is drawn inside the sidebar")
  popover.close(gui)
end)

test("hover=press restores content focus on release, hover=follow keeps the sidebar", function()
  local win, gui = ready_window()
  local tab = win.tab_list[1]
  local sb1 = sidebar.find(tab)
  press_row(gui, sb1, 3)
  eq(tab.active, sb1)
  mouse(gui, sb1, "up", "left", 5, 3)
  eq(tab.active, sb1, "follow leaves the sidebar active")
  config.setup { hover = "press", backend = { path = "/bin/wez-vtabs" } }
  press_row(gui, sb1, 3)
  mouse(gui, sb1, "up", "left", 5, 3)
  assert(tab.active ~= sb1, "press mode hands focus back")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("mouse move repaints on a row change and stays quiet inside the row", function()
  local win, gui = ready_window()
  local sb1 = sidebar.find(win.tab_list[1])
  local sent = #sb1.sent
  -- an inactive card, so hovering it actually changes the frame
  local row = title_row(sb1, win.tab_list[2].id)
  mouse(gui, sb1, "move", "none", 5, row)
  local repainted = #sb1.sent
  assert(repainted > sent, "crossing into a row repaints")
  mouse(gui, sb1, "move", "none", 6, row)
  eq(#sb1.sent, repainted, "same row, same spans, no frame")
end)

test("base64_decode round-trips and refuses malformed input", function()
  eq(util.base64_decode "bA==", "l")
  eq(util.base64_decode "G1tB", "\27[A")
  eq(util.base64_decode "", "")
  eq(util.base64_decode "bA", "l")
  eq(util.base64_decode "b*==", nil)
  eq(util.base64_decode "b", nil)
  eq(util.base64_decode(nil), nil)
end)

test("a key at a sidebar outside keyboard mode is typed into the content pane, which takes focus", function()
  local _, gui, tab, sb, content = key_window()
  sb:activate()
  local before = #content.sent
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"l","raw":"bA=="}')
  eq(#content.sent, before + 1)
  eq(content.sent[#content.sent], "l")
  eq(tab.active, content, "focus handed back to the shell")
end)

test("raw carrying an OSC or bracketed-paste introducer is dropped, focus still returns", function()
  local _, gui, tab, sb, content = key_window(2)
  local before = #content.sent
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"x","raw":"G10wOyE="}')
  eq(#content.sent, before, "OSC introducer never reaches the shell")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_window(3)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"x","raw":"G1syMDB+eA=="}')
  eq(#content2.sent, 0, "bracketed paste never reaches the shell")
end)

test("fast typing is forwarded whole; a flood is cut off at the burst budget", function()
  local _, gui, tab, sb, content = key_window()
  local real_now = util.now_ms
  local frozen = real_now()
  util.now_ms = function()
    return frozen
  end
  for _ = 1, 25 do
    input.handle(gui, sb, "vtabs", '{"t":"key","key":"a","raw":"YQ=="}')
  end
  util.now_ms = real_now
  eq(#content.sent, 20, "20 keys of burst, nothing refills within the same instant")
  eq(tab.active, content, "focus stays handed over across the dropped tail")
end)

test("a paste event is delivered whole to the content pane", function()
  local _, gui, tab, sb, content = key_window()
  input.handle(gui, sb, "vtabs", '{"t":"paste","data":"aGVsbG8gd29ybGQ="}')
  eq(content.pasted[#content.pasted], "hello world")
  eq(#content.sent, 0, "a paste is not typed key by key")
  eq(tab.active, content)
end)

test("a paste is refused when it is oversized, malformed or from a background tab", function()
  local _, gui, _, sb, content = key_window()
  input.handle(gui, sb, "vtabs", '{"t":"paste","data":"!!!!"}')
  eq(#content.pasted, 0, "malformed base64 dropped")
  local _, gui2, tab2, sb2, content2 = key_window(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"paste","dropped":"size"}')
  eq(#content2.pasted, 0, "the backend's oversize form carries nothing to paste")
  eq(tab2.active, content2, "focus still returns to the shell")
  local win3, gui3 = ready_window()
  win3.active_tab_ref = win3.tab_list[1]
  local other = win3.tab_list[2]
  input.handle(gui3, sidebar.find(other), "vtabs", '{"t":"paste","data":"aGk="}')
  eq(#sidebar.content_pane(other).pasted, 0, "background tab dropped")
end)

test("without raw only a lone printable key is forwarded", function()
  local _, gui, tab, sb, content = key_window()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"enter"}')
  eq(#content.sent, 0, "named keys send nothing")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_window(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"c","mods":["ctrl"]}')
  eq(#content2.sent, 0, "ctrl chords send nothing")
  local _, gui3, _, sb3, content3 = key_window(3)
  input.handle(gui3, sb3, "vtabs", '{"t":"key","key":"z"}')
  eq(content3.sent[1], "z")
end)

-- Verbatim backend output; the strings match backend/src/event.rs `key_events`.
test("safe_key_bytes takes one key press per shape and refuses a command line", function()
  for _, ok in ipairs { "x", "\u{e6}", "\r", "\n", "\t", "\8", "\3", "\127", "\0", "\27" } do
    eq(input.safe_key_bytes(ok), ok, "accepts " .. #ok .. " byte(s)")
  end
  for _, seq in ipairs { "\27[A", "\27[1;5D", "\27[3~", "\27OH", "\27b" } do
    eq(input.safe_key_bytes(seq), seq, "accepts an ESC-prefixed key")
  end
  for _, bad in ipairs {
    "id > /tmp/pwn\r",
    "ab",
    "\27[Ax",
    "\27]0;x\7",
    "\27[200~",
    "\27[201~",
    "\27P0q",
    string.rep("x", 17),
    "",
  } do
    eq(input.safe_key_bytes(bad), nil, "rejects " .. string.format("%q", bad))
  end
  eq(input.safe_key_bytes(nil), nil)
  eq(input.safe_key_bytes "\xff\xfe", nil, "invalid utf-8 is not one codepoint")
end)

-- Sequences backend/src/parser.rs names "unknown": F5, SS3 Z, and a CSI it does not decode.
test("a key the backend could not name is still forwarded by its raw bytes", function()
  local _, gui, tab, sb, content = key_window()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"unknown","raw":"G1sxNX4="}')
  eq(content.sent[#content.sent], "\27[15~", "F5 reaches the shell")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_window(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"unknown","raw":"G09a"}')
  eq(content2.sent[#content2.sent], "\27OZ")
  local _, gui3, _, sb3, content3 = key_window(3)
  input.handle(gui3, sb3, "vtabs", '{"t":"key","key":"unknown"}')
  eq(#content3.sent, 0, "without raw there is nothing to forward")
end)

test("a raw payload carrying a whole command line never reaches the content pane", function()
  local _, gui, tab, sb, content = key_window()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"x","raw":"aWQgPiAvdG1wL3B3bg0="}')
  eq(#content.sent, 0, "the probe payload is dropped")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_window(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"up","raw":"G1tB"}')
  eq(content2.sent[#content2.sent], "\27[A", "a real arrow key still gets through")
end)

test("backend-shaped key events reach the content pane as the exact bytes they carried", function()
  local _, gui, tab, sb, content = key_window()
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"enter","raw":"DQ=="}')
  eq(content.sent[#content.sent], "\r")
  eq(tab.active, content)
  local _, gui2, _, sb2, content2 = key_window(2)
  input.handle(gui2, sb2, "vtabs", '{"t":"key","key":"c","mods":["ctrl"],"raw":"Aw=="}')
  eq(content2.sent[#content2.sent], "\3", "ctrl chords are forwarded verbatim when raw is present")
  local _, gui3, tab3, sb3, content3 = key_window(3)
  input.handle(gui3, sb3, "vtabs", '{"t":"key","key":"escape"}')
  eq(#content3.sent, 0, "a key the backend could not capture sends nothing")
  eq(tab3.active, content3, "focus still returns to the shell")
end)

test("a key from a background tab's sidebar is never forwarded", function()
  local win, gui = ready_window()
  win.active_tab_ref = win.tab_list[1]
  local other = win.tab_list[2]
  local content = sidebar.content_pane(other)
  input.handle(gui, sidebar.find(other), "vtabs", '{"t":"key","key":"l","raw":"bA=="}')
  eq(#content.sent, 0)
end)

test("a key from a sidebar in another domain than its content pane is dropped", function()
  local _, gui, _, sb, content = key_window()
  sb.domain = "desktop"
  input.handle(gui, sb, "vtabs", '{"t":"key","key":"l","raw":"bA=="}')
  eq(#content.sent, 0)
end)

---Counts tab switches by wrapping the shared Tab metatable for the duration of `fn`.
local function count_switches(win, fn)
  local Tab = getmetatable(win.tab_list[1])
  local original = Tab.activate
  local switches = 0
  Tab.activate = function(self)
    switches = switches + 1
    return original(self)
  end
  local ok, err = pcall(fn)
  Tab.activate = original
  if not ok then
    error(err, 0)
  end
  return switches
end

test("reorder restores the active tab once for the whole batch", function()
  local win, gui = window(4)
  sidebar.ensure(gui)
  local ids = {}
  for i, t in ipairs(win.tab_list) do
    ids[i] = t.id
  end
  win.active_tab_ref = win.tab_list[1]
  local before = #win.actions
  local switches = count_switches(win, function()
    actions.reorder(gui, { ids[4], ids[3], ids[2], ids[1] })
  end)
  local moves = #win.actions - before
  assert(moves >= 2, "several tabs moved, got " .. moves)
  eq(switches, moves + 1, "one restore, not one per move")
  eq(win.active_tab_ref.id, ids[1])
end)

test("close_others restores the kept tab once, not after every close", function()
  local win, gui = window(4)
  sidebar.ensure(gui)
  win.active_tab_ref = win.tab_list[1]
  local kept = win.tab_list[1].id
  local switches = count_switches(win, function()
    actions.close_others(gui, kept)
  end)
  eq(#win.tab_list, 1)
  eq(win.active_tab_ref.id, kept)
  eq(switches, 4, "three closes plus one restore")
end)

test("is_sidebar_pane answers for any backend pane and changes nothing while it answers", function()
  local vtabs = require "vtabs.sidebar"
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  local pid = sb:pane_id()
  sb.vars.vtabs_token = state.token_for(pid)
  state.session.ready[pid] = nil
  local mapped = state.sidebar_pane_id(tab:tab_id())
  assert(vtabs.is_backend(sb), "a backend pane is skippable before anyone authenticates it")
  eq(state.session.ready[pid], nil, "answering never promotes the pane to ready")
  eq(state.sidebar_pane_id(tab:tab_id()), mapped, "no map mutation")
  assert(vtabs.is_ready(sb), "the trusted predicate is the one that promotes")
  eq(state.session.ready[pid], true)
  eq(vtabs.is_backend(sidebar.content_pane(tab)), false, "a content pane is not a backend")
  eq(vtabs.is_backend(nil), false)
end)

test("the window title names the content pane while the sidebar holds focus", function()
  local view_only = require "vtabs.view"
  -- PaneInformation carries only ids, so window_title resolves them through the mux, like wezterm.
  local win, gui = window(1)
  sidebar.ensure(gui)
  local mux_tab = win.tab_list[1]
  mark_ready(mux_tab)
  local content = sidebar.content_pane(mux_tab)
  content.title = "nvim"
  local sb = { pane_id = sidebar.find(mux_tab):pane_id() }
  local shell = { pane_id = content:pane_id(), title = "nvim" }
  local tab = { tab_id = mux_tab:tab_id(), tab_index = 1, tab_title = "" }
  eq(view_only.window_title(tab, sb, { tab }, { sb, shell }), "nvim")
  eq(view_only.window_title(tab, sb, { tab, tab }, { sb, shell }), "[2/2] nvim")
  eq(view_only.window_title(tab, shell, { tab }, { sb, shell }), nil, "wezterm's default is left alone")
  eq(view_only.window_title(tab, sb, { tab }, { sb }), nil, "no content pane, no opinion")
  eq(view_only.window_title(nil, nil, nil, nil), nil)
  eq(config.setup({}).window_title, true, "registered by default")
  eq(config.setup({ window_title = false }).window_title, false, "opt out leaves the event alone")
  config.setup { backend = { path = "/bin/wez-vtabs" } }
end)

test("an unreachable contrast gate stops at the target colour instead of mixing past it", function()
  for _, p in ipairs { palette("#2b2b2b", "#4a4a4a"), palette("#ffffff", "#cccccc") } do
    local t = theme.resolve({}, p)
    assert(theme.contrast(t.fg, t.active_bg) < 3.5, "the fixture ceiling is below the meta gate")
    eq(rgb(t.meta_fg), rgb(t.fg), "meta_fg lands on fg, never past it")
    for i = 1, 3 do
      assert(t.meta_fg[i] >= 0 and t.meta_fg[i] <= 255, "channel in range")
    end
  end
  local reachable = theme.resolve({}, palette("#1e1e2e", "#cdd6f4"))
  assert(theme.contrast(reachable.meta_fg, reachable.active_bg) >= 3.5, "a reachable gate is met")
  assert(rgb(reachable.meta_fg) ~= rgb(reachable.fg), "and meta_fg is still quieter than fg")
end)

test("a mux window resize is not a divider drag, even though the pane size arrives a poll late", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local sb = mark_ready(win.tab_list[1])
  eq(sb.cols, 28)
  -- A mux client learns the new window size before the server sends back the new pane sizes.
  win.cols = win.cols + 40
  geometry.correct(gui)
  for _, tab in ipairs(win.tab_list) do
    tab:adjust_x_size(40)
  end
  eq(sb.cols, 48, "adjust_x_size gave the sidebar half of the delta")
  eq(geometry.desired(gui:window_id()), 28, "the drift was adopted as the user's width")
  geometry.correct(gui)
  eq(sb.cols, 28)
end)

test("the two-step mux resize is corrected, and a real divider drag is still adopted", function()
  local win, gui = window(1)
  sidebar.ensure(gui)
  local tab = win.tab_list[1]
  local sb = mark_ready(tab)
  eq(geometry.correct(gui), false, "baseline")
  win:resize_mux(30)
  eq(geometry.correct(gui), false, "the window grew but the panes have not heard yet")
  win:settle_mux()
  eq(sb.cols, 43, "wezterm dealt the sidebar half the delta")
  assert(geometry.correct(gui), "the late pane size is a resize, not a drag")
  eq(sb.cols, 28)
  eq(geometry.desired(gui:window_id()), 28, "nothing was adopted")

  geometry.reset(gui:window_id())

  eq(geometry.correct(gui), false, "baseline on the settled window")
  tab:set_split(34)
  eq(geometry.correct(gui), false, "same tab width, same pixels: this one is a drag")
  later(400, function()
    eq(geometry.correct(gui), false, "taken once it settles")
  end)
  eq(geometry.desired(gui:window_id()), 34)
end)

test("a paste is charged by its size, so a second large one waits for the budget", function()
  local _, gui, tab, sb, content = key_window()
  local big = string.rep("eHh4", 21845) .. "eA=="
  -- the bucket refills from the clock, so both pastes must land in the same tick
  local real_now = util.now_ms
  local frozen = real_now()
  util.now_ms = function()
    return frozen
  end
  input.handle(gui, sb, "vtabs", string.format('{"t":"paste","data":"%s"}', big))
  eq(#content.pasted, 1, "the first 64 KiB paste goes through")
  eq(#content.pasted[1], 64 * 1024)
  input.handle(gui, sb, "vtabs", string.format('{"t":"paste","data":"%s"}', big))
  eq(#content.pasted, 1, "the second is over budget")
  util.now_ms = real_now
  eq(tab.active, content, "focus is still handed over")
end)

test("the sidebar's own resize event corrects at once, inside the observe gate", function()
  local win, gui = ready_window()
  local tab = win.tab_list[1]
  local sb = sidebar.find(tab)
  geometry.sync(gui, tab.id)
  win:resize_mux(30)
  win:settle_mux()
  eq(sb.cols, 43, "wezterm dealt the sidebar half the delta")
  eq(geometry.sync(gui, tab.id), false, "the poll gate is still closed")
  eq(sb.cols, 43)
  input.handle(gui, sb, "vtabs", '{"t":"resize","cols":43,"rows":30}')
  eq(sb.cols, 28, "the backend reporting its own size is never gated")
end)

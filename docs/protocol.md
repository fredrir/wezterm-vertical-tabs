# Sidebar backend protocol (v3)

The sidebar is a WezTerm pane running `wez-vtabs` (the Rust backend). Rust owns
layout, paint, hit-testing, the action menu and the settings screen; the
plugin owns mux facts, config and dispatch:

- **backend → Lua**: events, sent as WezTerm user vars
  (`ESC ] 1337 ; SetUserVar=vtabs=<base64(json)> BEL`), received by the plugin
  through the `user-var-changed` event.
- **Lua → backend**: session-bound control records, written to the pane's stdin with
  `pane:send_text(frame .. "\n")`. Every record is `RS VTABS <token> <json>` on one
  physical line (`RS` is byte `0x1e`). Unframed JSON and literal braces are keyboard input.

## Startup

1. Enter raw mode, alternate screen, hide cursor.
2. Fill the alternate screen with `VTABS_BG` (skipped when unset), before anything else is drawn.
3. Enable mouse reporting: `?1000h ?1002h ?1003h ?1006h`, focus reporting
   `?1004h`, bracketed paste `?2004h`; auto-wrap off `?7l`.
4. Emit user var `vtabs_role` = `sidebar` | `settings` (plain, base64 of the literal string).
5. Set the pane title marker `wez-vtabs:<nonce>` / `wez-vtabs-settings:<nonce>` (OSC 0 and OSC 2).
   The nonce is a per-process random id, never the auth token: window titles are readable by the
   whole desktop.
6. Emit event `{"t":"ready","v":3,"cols":N,"rows":M,"paints":true,"caps":["atomic_sync","typed_intents","theme_hooks","settings_document","spaces_policy","inbox_transport"],"pane":42,"transport":{"inbox":"inbox-42-9f3a1b2c"},"n":1}`.
7. Loop until stdin EOF or `quit`.



## Commands (Lua → backend)

Each JSON shape below is the payload of a framed record:

```text
\x1eVTABS <session-token> <compact-json>\n
```

The initial `auth` must use the same token in its frame and JSON payload. Once accepted, every
record must use the active session token. Token rotation is explicit: an authenticated frame using
the old token may carry `auth.token=<new-token>`; after it succeeds, the old token is immediately
invalid. Tokens are non-empty ASCII graphic strings of at most 64 bytes. Malformed frames and
commands carrying no proof of the active session are consumed without changing backend state.

| command             | shape                                                                 | effect                                                                      |
| ------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `auth`              | `{"t":"auth","token":"<hex>","caps":["typed_intents","theme_hooks","settings_document","spaces_policy"],"keys":"server"}` | echo `vtabs_token` and negotiate client capabilities; `keys:"server"` asks for forwarded keys to be delivered through the server's cli |
| `begin`             | `{"t":"begin","generation":N}`                                   | start or replay one atomic publication                                      |
| `commit`            | `{"t":"commit","generation":N}`                                  | publish the matching valid generation, or start its theme-hook round-trip   |
| `theme_hook_result` | `{"t":"theme_hook_result","generation":N,"overrides":{...}}`    | finish the matching generation after Lua runs `hooks.theme`                 |
| `space_route_hook_result` | `{"t":"space_route_hook_result","generation":N,"routes":[...]}` | finish the matching batched `hooks.route` request                           |
| `config`            | see below                                                             | raw render/behaviour knobs                                                   |
| `theme`             | see below                                                             | raw palette, user overrides and hook/private facts                           |
| `spaces`            | see below                                                             | full-window topology, raw rules, and current assignment facts                |
| `model`             | see below                                                             | raw sidebar state, or the legacy pre-built settings view                     |
| `settings`          | see below                                                             | JSON-safe host facts from which Rust builds the live settings document       |
| `menu`              | see below                                                             | the action menu, or `{"t":"menu","rev":N,"open":false}` to close      |
| `fx`                | `{"t":"fx","phase":"expand_in","ms":220,"fps":30}`           | animate this pane's rows on the backend's own clock                          |
| `notice`            | `{"t":"notice","level":"warn","text":"..."}`                 | write to the debug log only                                                  |
| `ping`              | `{"t":"ping","n":N}`                                              | reply with `pong` echoing `n`                                                |
| `clear`             | `{"t":"clear"}`                                                     | repaint from the last committed state                                        |
| `quit`              | `{"t":"quit"}`                                                      | restore the terminal and exit 0                                              |
| `kill`              | `{"t":"kill","title":"wez-vtabs:1a2b"}`                          | kill the one pane with that backend marker; answer with `cli`                |
| `rescue`            | `{"t":"rescue","band":28,"position":"left"}`                   | move other panes out of the sidebar band through the existing CLI bridge    |
| `adjust`            | `{"t":"adjust","direction":"Left","amount":3,"park":false}`     | resize this pane's split on the server, from the tab's active pane; answer with `cli` |
| `kill` by id        | `{"t":"kill","pane":17}`                                          | kill the server pane with that id; answer with `cli`                         |
| `transport_probe`   | `{"t":"transport_probe","session":"inbox-4242-9f3a"}`           | inbox message 1: proves the directory is the one the backend reads          |
| `transport_barrier` | `{"t":"transport_barrier","session":"inbox-4242-9f3a"}`         | last stdin frame; triggers one scan, then `transport_ready` or `transport_refused` |
| `transport_stop`    | `{"t":"transport_stop","session":"inbox-4242-9f3a"}`            | drain the inbox in order, remove it, read stdin only                         |

`kill`, `rescue` and `adjust` run the server's own `wezterm cli --no-auto-start`, found beside
`WEZTERM_EXECUTABLE_DIR`, against `WEZTERM_UNIX_SOCKET`, acting for `WEZTERM_PANE`: the GUI's mux
client cannot reach a mux-domain pane, and closing one by activation has aborted it.

`adjust` is the width correction on a mux domain, where the GUI's split tree is a mirror rebuilt
from the server's pane list after every pane resize: the split is resized where the tree is
authoritative, once the frames have stopped. `adjust-pane-size` walks up from the tab's active
pane, so when that pane cannot reach the sidebar's split the backend's own pane is activated for
it and the focus handed back; with `park` it stays on the sidebar, owed to the next `adjust`.

Unknown commands are ignored. Malformed JSON lines are ignored.

Changing the auth token starts a new client session. The backend clears committed and pending
sections plus menu, settings, pointer, and generation state, clears the terminal, and emits a new
`ready`; re-authenticating with the same token only refreshes negotiated client capabilities.

### Atomic publication

Clients using `atomic_sync` wrap each changed-section batch in `begin`/`commit`. A `begin` clones
the last committed state, so later generations may send only the sections that changed. The first
generation needs `config`, `theme`, and either `model` or `settings`; a client using
`spaces_policy` also needs `spaces`. `menu` is optional because the settings role has no menu. A
`settings` command stages its generated render model and mutable document together as the model
section. Nothing staged is painted or hit-tested before the matching `commit`.

A section validation or bounds failure invalidates the whole pending generation. Stale and mismatched
commits change nothing, a newer `begin` replaces an incomplete generation, and retrying the same
generation before commit restarts it from committed state. Once a route- or theme-hook request has
left the backend, that generation is frozen. The backend waits 500 ms, replays that exact request
once, then emits `dropped{reason:"timeout"}` and finishes deterministically: a missing route answer
is treated as explicit nil answers before ordinary rules run, and a missing theme answer uses the
Rust-resolved base. Lua caches each hook answer by window/generation, so the replay never invokes
user hook code twice. These rules prevent mixed revisions and reduce a normal multi-section update
to one paint. Legacy immediate behavior remains for the original sections; `spaces` itself
requires the atomic/capable path.

### config

```json
{"t":"config","rev":3,"rail_width":5,"position":"left",
 "icons":true,"icon_map":{"nvim":"󰉯"},"meta":"cwd","meta_sep":" · ",
 "glyphs":{"custom_block":true,"east_asian_wide":false},
 "double_click_ms":300,"tear_off":true,
 "wheel":"scroll","context":"popover","hover_timeout_ms":1200,"hover_highlight":true,"ellipsis":"…",
 "popover":{"width":null,"follow_pointer":true,"overflow":null},
 "render":{"meta":true,"padding":{"left":2,"right":2,"top":1,"bottom":1},"frame":false,
   "row_gap":0,"new_tab_button":true}}
```



### theme

```json
{"t":"theme","rev":7,"private":true,"hook":true,
 "scheme":{"background":"#1e1e2e","foreground":"#cdd6f4","cursor_bg":"#f5e0dc",
           "active_tab_bg":"#313244",
           "ansi":["#45475a","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#bac2de"],
           "brights":["#585b70","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#a6adc8"]},
 "overrides":{"accent":"#89b4fa","elevation":0.06}}
```

Lua supplies only the raw facts it can observe: WezTerm's effective palette, user overrides, the
window-global private flag, and whether a hook exists. Rust (`vtabs-engine::theme`) is the sole
resolver and applies all fallbacks, mixing, and contrast enforcement. Raw color inputs are
normalized to `#rgb`/`#rrggbb`; a resolved theme uses RGB triples because that is also the
renderer representation.

With `hook:false`, commit resolves and publishes immediately. With `hook:true` and a client that
advertised `theme_hooks`, the matching commit emits the full resolved DTO (abbreviated here):

```json
{"t":"theme_hook_request","generation":12,
 "theme":{"bg":[23,23,35],"fg":[205,214,244],"accent":[137,180,250]}}
```

Lua calls `hooks.theme(window, resolved_theme)`, normalizes its partial answer, and sends:

```json
{"t":"theme_hook_result","generation":12,"overrides":{"accent":"#ff8800"}}
```

Only the requested pending generation accepts the result. Rust layers it over the raw overrides
and any active-space theme, resolves again, commits once, emits `theme_resolved`, and paints once.
Stale, mismatched, or duplicate results never alter committed state. A malformed matching result
is reported and commits the same deterministic Rust base used by timeout fallback. A client
without `theme_hooks` receives that base immediately instead of blocking.

### spaces

```json
{"t":"spaces","rev":18,"window_id":3,"enabled":true,"hook":true,
 "definitions":[
   {"id":"home","name":"Home"},
   {"id":"$host","name":"Remote $host","match":{"remote":true},"theme":"auto"}],
 "tabs":[
   {"id":7,"index":1,"title":"nvim","proc":"nvim","cwd":"~/p/x",
    "domain":"local","remote":false,"space":"home","manual":false,"fingerprint":"6a09e667"}],
 "active_tab":7,"active_space":"home",
 "follow":{"tab_id":7,"space":"home"},
 "last_tabs":[{"space_id":"home","tab_id":7}],
 "dynamics":[{"id":"pi","name":"Remote pi","template":"$host","seq":1}]}
```

This is a complete immutable window census, sent identically to sidebar and settings processes.
Rust validates definitions and theme layers, matches anchored globs and rules, expands templates,
owns sticky/manual routing, bounds dynamic admission, follows the active tab, chooses visibility,
builds summaries, and selects the active space's theme. Automatic accents use the effective
palette's normal and bright ANSI slots in deterministic order. Lua only supplies facts and applies
the committed assignment/cache and mux effects. Its theme bridge converts host colour objects to
JSON-safe spellings only; representable unknown or invalid fields remain for Rust to warn about.

If `hook:true` has routing cache misses, commit pauses before theme resolution and emits one batch:

```json
{"t":"space_route_hook_request","generation":12,"window_id":3,
 "tabs":[{"tab_id":7,"window_id":3,"title":"nvim","proc":"nvim",
          "cwd":"~/p/x","domain":"local","remote":false,"space":"home","manual":false}]}
```

Lua runs `hooks.route(facts)` once for each requested tab and answers every row explicitly; an
omitted `space` is the hook's nil/no-op result:

```json
{"t":"space_route_hook_result","generation":12,
 "routes":[{"tab_id":7}]}
```

The answer must contain exactly the requested tab ids, with no missing, duplicate, or unknown row.
Rust then finishes space planning. If a theme hook is also configured, `theme_hook_request` follows
and already includes the selected space layer. Only after all required answers validate does the
runtime publish, emit `spaces_resolved` and `theme_resolved`, and paint once. Stale results are
ignored. An invalid matching batch is reported and falls back to explicit nil hook answers, so
ordinary rules still finish the generation instead of leaving the last committed view indefinitely.

A plugin with spaces configured requires `spaces_policy`. Without it, Lua warns once and presents
one unpartitioned all-tabs list; it never runs a second routing or validation implementation.

### model

Sidebar pane (`screen:"sidebar"`):

```json
{"t":"model","rev":142,"screen":"sidebar","rail":false,"active":7,
 "focus":{"on":true,"index":3},
 "scroll":{"top":4,"user":true},
 "drag":{"id":7,"active":true,"slot":3,"outside":false,
         "origin":{"x":5,"y":6,"at":1712345678901}},
 "strip":{"dpi":144,
          "chrome":{"is_mac":true,"integrated_buttons":true,"native_button_style":true,
                    "preview":false,"is_full_screen":false},
          "buttons":[{"id":"toggle"},{"id":"settings"}]},
 "footer":[{"text":"New tab"}],
 "space":"home",
 "spaces":[{"id":"home","name":"Home","icon":"󰋜","unseen":false},
           {"id":"pi","name":"pi","icon":"󰒋","unseen":true}],
 "tabs":[
   {"id":7,"index":1,"title":"nvim","pane_title":"nvim — x","proc":"nvim",
    "cwd":"~/p/x","host":"pi","user":"f","domain":"local",
    "pinned":false,"unseen":false}],
 "private":false}
```

`strip.dpi` and `strip.chrome` are raw WezTerm observations; the pane's own cells and pixel size
the backend measures for itself, and the dpi is the one number it cannot. An older Lua sends
`strip.metrics` (`cols`, `viewport_rows`, `pixel_width`, `pixel_height`, `dpi`) instead, which is
still read. Lua never sends derived strip rows, reserved columns, cell width, or toggle coordinates. `vtabs-engine::geom` derives all of
those for rendering. When its traffic-light column reserve changes, the backend returns
`intent{a:"set_rail_reserve",cols}`; Lua passes that value to `geometry.lua` solely to implement
`rail_titlebar="widen"` in the host mux.

Older clients may still send a pre-built `screen:"settings"` model. Current clients use the raw
`settings` command below instead; mixing the two clears local document ownership in favor of the
most recent command.

### settings

```json
{"t":"settings","rev":9,
 "values":{"width":28,"frame":{},"strip_actions":[],"spaces":[],
           "backend":{"env":{"A.B":"one"}}},
 "explicit":[["padding","top"]],
 "host_values":["window_padding"],
 "opaque":[["backend","path"]],
 "key_defaults":{"new_tab":{"key":"t","mods":"CTRL|SHIFT"}},
 "is_macos":false,"version":"0.1.10"}
```

Lua only projects values and facts that require the host runtime. Paths are arrays so a dynamic
key such as `A.B` remains one segment. Schema lists and known nested match lists are explicitly
encoded as JSON arrays even when empty. Non-JSON values stay in Lua only when their Function, Any,
open-map, or callback-bearing list owner supports them; those paths appear under `opaque`, render
read-only, and are excluded from persistence/copy. A non-JSON typed scalar is sent as invalid and
Rust resets it to its descriptor default instead of restoring it over the canonical answer.

`vtabs-engine::settings::SettingsDocument` owns descriptor validation, aliases, groups, fields,
widgets, edit/recorder state, resets, changed-value calculation, Lua serialization, and the final
versioned persistence JSON. The document produces the engine-owned `SettingsPresentation` used by
rendering and interaction; it is not sent back over the wire after each key. The current plugin
requires the backend's `settings_document` capability and does not rebuild the legacy model in
Lua; historical backend compatibility and version pinning remain deliberately out of scope.

### menu

```json
{"t":"menu","rev":9,"open":true,"level":"root","anchor":{"row":6,"col":12},
 "target":7,"subject":7,"selected":1,
 "items":[
   {"id":"activate","label":"Switch to tab"},
   {"id":"pin","label":"Pin tab"},
   {"id":"rename","label":"Rename…"},
   {"id":"space","label":"Move to space","hint":"▸","disabled":true},
   {"id":"tear_off","label":"Move to new window"},
   {"id":"duplicate","label":"Duplicate tab"},
   {"id":"settings","label":"Settings…"},
   {"id":"close_others","label":"Close other tabs"},
   {"id":"close","label":"Close tab","danger":true}]}
```

Current Lua sends the subject id (and `victims` on confirmation) rather than formatted header
text. Rust derives root, spaces, rename, and confirmation headers from the same raw tab record and
title/meta policy used for the card. The optional `header` field remains only as a legacy escape
hatch.

| `level`          | items                                                                                               | notes                                                                      |
| ---------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `root` (default) | `popover.items()` rows                                                                              | `danger` on `close`/`close_others`                                         |
| `confirm`        | the question in `header.title`, then Close (`danger`)/Cancel                                        | `selected:2` — Cancel is armed                                             |
| `rename`         | one `{"id":"rename_field","mode":"edit","value":"<current title>"}`                                 | Rust owns the buffer + cursor                                              |
| `spaces`         | one `{"id":"space:<id>","label":<name>,"hint":"<tab count>"}` per space, the current one `disabled` | a sub-level: Esc is `menu_back`; never narrower than the root it came from |

## Events (backend → Lua)

| event                | shape                                                                                                                        |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `ready`              | `{"t":"ready","v":3,"cols":N,"rows":M,"paints":true,"caps":["atomic_sync","typed_intents","theme_hooks","settings_document","spaces_policy","inbox_transport"],"pane":42,"transport":{"inbox":"inbox-42-9f3a1b2c"},"n":1}` |
| `resize`             | `{"t":"resize","cols":N,"rows":M,"n":2}`                                                                             |
| `theme_hook_request` | `{"t":"theme_hook_request","generation":N,"theme":{...},"n":3}` — resolved base; the generation remains unpublished  |
| `theme_resolved`     | `{"t":"theme_resolved","generation":N,"theme":{...},"n":4}` — committed effective answer for host/Zen projection     |
| `space_route_hook_request` | `{"t":"space_route_hook_request","generation":N,"window_id":W,"tabs":[...],"n":5}` — one unpublished hook batch |
| `spaces_resolved`    | `{"t":"spaces_resolved","generation":N,"window_id":W,...,"n":6}` — committed assignments, summary, visibility, theme layer |
| `settings_commit`    | source `settings_rev`, segmented set/remove patches, apply mode, and Rust's final persistence JSON                         |
| `settings_copy`      | `{"t":"settings_copy","lua":"vtabs.apply_to_config(config, {...})"}` — complete paste-ready snippet             |
| `intent`             | `{"t":"intent","a":"press_card","tab_id":7,"x":5,"y":6,"part":"title","n":9}` — variant-specific fields       |
| `do`                 | centralized compatibility downgrade, only for clients without `typed_intents`                                               |
| `key`                | `{"t":"key","key":"c","mods":["ctrl"],"raw":"Aw==","delivered":false,"n":10}` — host-forwarded keys; `delivered:true` means the bytes already reached the content pane through the server's cli and Lua only hands focus over |
| `transport_ready`    | `{"t":"transport_ready","session":"inbox-4242-9f3a","n":11}` — probe and barrier both seen; Lua may write to the inbox |
| `transport_refused`  | `{"t":"transport_refused","session":"inbox-4242-9f3a","why":"probe","n":11}` — `why` is `probe` (barrier without the probe; the offer is withdrawn), `session` (a session this backend is not negotiating; the current transport stays) or `state` (nothing offered) |
| `paste`              | `{"t":"paste","data":"<base64>","n":11}`, or `{"t":"paste","dropped":"size","n":11}` past 64 KiB               |
| `focus`              | not sent: focus out clears backend hover; focus in does nothing                                                              |
| `pong`               | `{"t":"pong","n":13,"echo":7}` — `echo` is the ping's own `n`                                                          |
| `note`               | `{"t":"note","k":"menu_refused","why":"rows","id":7,"a":"confirm","n":14}`                                   |
| `dropped`            | `{"t":"dropped","what":"model","reason":"bounds","n":15}` — pending transaction is invalidated; `{"t":"dropped","what":"message","reason":"gap","seq":7}` — one inbox message never arrived, Lua republishes |
| `cli`                | `{"t":"cli","op":"kill","ok":true,"detail":"1","n":16}` — `op` is `kill`, `rescue` or `adjust`; count or error detail |

Every `intent` is tagged by `a` and carries only that variant's fields. In particular,
`set_rail_reserve` is `{"t":"intent","a":"set_rail_reserve","cols":9}`. The legacy `do`
envelope remains a single serialization-boundary downgrade; new clients never receive it.

`theme_resolved` is cached by generation and effective value. Atomic generations each receive a
current projection answer even when two successive themes resolve identically; legacy immediate
updates suppress repeated equal answers.

`spaces_resolved` is also generation- and window-bound. Lua applies its mux-side state, then sends
one follow-up generation containing the returned fingerprints and assignments. A
`theme_resolved` from the superseded generation is ignored; the matching follow-up answer lands
the host theme without starting another semantic generation.



## Inbox transport

| Name | Value |
| --- | --- |
| Why | `pane:send_text` on a mux-domain pane blocks the GUI thread on a server round trip; frames to same-machine mux panes go through files instead |
| Eligible | `mux.domain(pane) ~= "local"` and the domain is a unix domain of this machine; `backend.inbox = true` |
| Root | `VTABS_INBOX_ROOT` from Lua: `$XDG_RUNTIME_DIR/wez-vtabs`, else `$TMPDIR/wez-vtabs`, else no transport |
| Session dir | `<root>/inbox-<pid>-<nonce>`, mode 0700, announced as `ready.transport.inbox`; absent when no root was given or it failed validation |
| `ready.pane` | the backend's own server pane id from `WEZTERM_PANE`; absent when unset |
| `auth.keys` | `"server"` only where Lua would hand a typed key over: an eligible sidebar, `hover ~= "press"`, plain content beside it; a same-token `auth` flips it live |
| Message | `<seq zero-padded 8>.msg`, written as `<seq>.tmp` then renamed; framed control records, one or more |
| Order | Lua writes the probe (seq 1) before the stdin barrier; the backend applies no inbox message before the barrier and applies messages in sequence |
| Limits | `LINE_MAX` per record, 6 × `LINE_MAX` per file; a gap older than 100 ms is one `dropped` message |
| Lost | write, close or rename failure: the frame is dropped, never replayed; Lua sends `transport_stop` |
| Wake | inotify / kqueue on the directory, scan every 1 s with a watcher, every 50 ms without |
| Cleanup | processed messages deleted; the directory removed on `quit`, `transport_stop`, new `auth`; dead siblings swept at create |

## Gesture → event (sidebar)

`vtabs-engine::interaction` resolves the mouse and focus-mode keymap against the same immutable
scene Rust painted. It emits typed `intent` variants; Lua dispatches those host operations by
name. Only keys received while focus mode is off may leave Rust as raw `key` events.

| Input                                                        | Condition                                      | Typed intent / outcome                                                                   |
| ------------------------------------------------------------ | ---------------------------------------------- | ---------------------------------------------------------------------------------------- |
| left press, card body                                        | not on the ✕/pin span                          | `press_card {tab_id,x,y,part}` — activate and arm drag                                    |
| left press, ✕ span                                           |                                                | arm locally; nothing crosses until release                                                |
| left press, pin span                                         |                                                | `toggle_pin {tab_id}`                                                                    |
| middle press, card body                                      |                                                | arm locally (same close arming as ✕)                                                      |
| right press, card body                                       | `config.context=="popover"`                    | arm locally; open on release                                                             |
| motion past the drag threshold                               | a press is armed (locally or via `model.drag`) | `drag_to {x,y,slot,outside}`                                                              |
| release, drag active                                         |                                                | `drag_end {slot?,outside}`                                                               |
| release, ✕/middle armed and still on the same card           |                                                | `request_close {tab_id,row,col?,from_key:false}`                                          |
| release, right armed and still on the same card              |                                                | `open_menu {tab_id,row,col?}`                                                             |
| double left press, off-card / space / strip                  | within `double_click_ms`                       | `new_tab`                                                                                |
| left press, strip button                                     |                                                | `strip {button_id}`                                                                      |
| left press, new-tab ghost row                                |                                                | `new_tab`                                                                                |
| left press, footer row                                       |                                                | `footer {index}`                                                                         |
| left press, a space icon                                     |                                                | `switch_space {space_id}`; a lone visible icon targets the next space                    |
| double press on the switcher row                             |                                                | nothing — never `new_tab`                                                                |
| wheel over the switcher row                                  | either wheel mode                              | `switch_space {space_id}`, no wrap, silent at the ends                                   |
| wheel                                                        | `config.wheel=="scroll"`                       | `set_scroll {top,user:true}` with optimistic local apply                                  |
| wheel                                                        | `config.wheel=="switch"`                       | `wheel_tab {dy}`                                                                         |
| release, no close/menu/drag outcome, `config.hover=="press"` |                                                | `blur_sidebar`                                                                           |
| any key                                                      | focus mode off                                 | `key {...}` for guarded Lua forwarding to the content pane                               |

Focus mode (`model.focus.on`), keyed entirely in Rust:

| Key                                         | Typed intent / outcome                                        |
| ------------------------------------------- | ------------------------------------------------------------- |
| `j`/`down`/`tab`, `k`/`up` (shift reverses) | `set_focus_index {index}`                                     |
| `home`/`g`, `end`/`G`                       | `set_focus_index {index}`                                     |
| `1`-`9`, `enter`/`space`                    | `activate_tab {tab_id}`                                       |
| `x`/`d`/`delete`                            | `request_close {tab_id,row,from_key:true}`                    |
| `p`                                         | `toggle_pin {tab_id}`                                         |
| `r`                                         | `rename_tab {tab_id}`                                         |
| `m`                                         | `open_menu {tab_id,row}`                                      |
| `J` / `K`                                   | `move_tab {tab_id,slot,focus_index}`                           |
| `]` / `[`                                   | `switch_space {space_id}` with wraparound                     |
| `n`                                         | `new_tab`                                                     |
| `escape`/`q`/ctrl-`c`                       | `blur_sidebar`                                                |
| anything else                               | consumed; focused keys are never forwarded back through Lua   |

## Gesture → event (menu)

The menu owns the pane while it is open — every pointer and key event over it is the menu's; the
sidebar's own hit map is not consulted.

| Input                                              | Condition                               | Typed intent / outcome                                             |
| -------------------------------------------------- | --------------------------------------- | ------------------------------------------------------------------ |
| left press, outside the menu's columns             | including a row it doesn't cover at all | `menu_closed`                                                      |
| left press, an enabled row                         | not `danger`                            | `menu_pick {item_id}`                                              |
| left press, a `danger` row                         |                                         | arms locally; picked on release over the same row                  |
| left release, armed danger row, still over it      |                                         | `menu_pick {item_id}`                                              |
| right press, a row outside the menu's rows (scrim) |                                         | `menu_closed`, then the click falls through to the sidebar         |
| wheel                                              |                                         | moves the selection locally; no event                              |
| enter/space                                        | selected item enabled                   | `menu_pick {item_id}`                                              |
| escape/ctrl-c, root level                          |                                         | `menu_closed`                                                      |
| escape/ctrl-c, sub-level                           |                                         | `menu_back`                                                        |
| `j`/`down`/`tab`, `k`/`up` (shift reverses)        |                                         | moves the selection locally                                       |
| a single character                                 | jump-to-letter                          | moves the selection locally                                       |
| enter, rename level                                |                                         | `rename_commit {text}`                                             |
| escape, rename level                               |                                         | `menu_back`                                                        |

Pointer drift (`Move`/`Drag`) never selects a `danger` row at the `confirm` level — a Rust-tested
invariant. A destructive item acts only on release, and only when press and release land on the
same row.

## Gesture → event (settings)

Navigation, filtering, widgets, edit buffers, the chord recorder, validation, and mutation are
Rust-local. `vtabs-engine::interaction` produces typed document actions which the runtime consumes
directly when a `settings` document is present.

| Key/click                                                              | Rust outcome                                                                           |
| ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `left`/`right` on a focused field, or a click on its `-`/`+` glyph     | step the canonical value and emit `settings_commit`                                    |
| `enter`/`space`, or a left click on a field's value                    | toggle/pick, enter edit mode, or arm the chord recorder                                |
| `r`                                                                    | restore the schema default and emit `settings_commit`                                  |
| `c`                                                                    | emit `settings_copy` from the canonical changed set                                    |
| any key while editing a buffer                                         | mutate locally; Enter validates/commits, Escape cancels                                |
| any key while a chord recorder is armed                                | commit the segmented binding path; Escape cancels                                     |
| `escape`, or `q` with no modifiers                                     | typed `close_settings` intent to Lua, because closing the host tab is a mux operation |
| `j`/`down`, `k`/`up`, `tab`, `/` + filter typing, a click on a nav tab | move focus/filter/group locally; no event                                              |

`settings_commit` carries the source `settings_rev`, `path:[segment,...]`, `change:{op:"set",value:...}` or
`change:{op:"remove"}`, optional `derived:[{path,...change}]` patches, an apply
`mode:"instant"|"override"|"reload"`, and a complete `persistence_json` string. Derived patches
are Rust's cross-field consequences (for example `hover="press"` making a hover-only close button
always visible). Lua applies that canonical patch set without rerunning policy, atomically writes
the supplied body when persistence was enabled before the commit, projects host-owned config, and
syncs once. Only the authenticated settings pane's current wire revision is accepted; delayed,
future, or revision-less effects do not mutate config, persist bytes, or project host settings.
The new config chooses the destination, so a path change writes the final body to the new path and
turning persistence off records that last choice. Lua performs no settings validation or
changed-set computation.

## Bounds

| Bound            | Value                                                              | On breach                                                                       |
| ---------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| `model.tabs`     | 200 (`MODEL_MAX_TABS`)                                             | `dropped{what:"model",reason:"bounds"}`; previous model kept                    |
| `model.fields`   | 512 (`MODEL_MAX_FIELDS`)                                           | `dropped{what:"model",reason:"bounds"}`; previous model kept                    |
| generated settings fields | 512 (`MODEL_MAX_FIELDS`)                                  | `dropped{what:"settings",reason:"bounds"}`; pending generation invalidated      |
| persisted settings body | 512 KiB (`SETTINGS_BODY_MAX_BYTES`), inclusive                | Rust rolls the edit back and emits `dropped{what:"settings",reason:"size"}`; Lua also refuses an oversized write |
| `model.spaces`   | 32 (`MODEL_MAX_SPACES`)                                          | `dropped{what:"model",reason:"bounds"}`; previous model kept                    |
| `spaces.tabs`    | 200 (`MODEL_MAX_TABS`), with unique tab ids                       | `dropped{what:"spaces",reason:"bounds"}`; pending generation invalidated        |
| `spaces.dynamics` / `last_tabs` | 32 each (`MODEL_MAX_SPACES`), with unique ids       | `dropped{what:"spaces",reason:"bounds"}`; pending generation invalidated        |
| `menu.items`     | 64 (`MENU_MAX_ITEMS`)                                              | `dropped{what:"menu",reason:"bounds"}`; previous menu kept                      |
| any JSON control payload | 1 MiB (`LINE_MAX`, `limits.rs`); framing is additional        | Lua preflights whole sections; an authenticated frame gets `dropped{what:"line",reason:"size"}` |
| stdin residual buffer | 2 MiB, or ~300ms with no `\n`                                  | a runaway frame is discarded — never replayed as key events                     |
| paste payload    | 64 KiB                                                             | `{"t":"paste","dropped":"size"}`                                                |

Every bound lives in `backend/crates/vtabs-protocol/src/limits.rs`, the one home both languages
read (`just gen-protocol` mirrors what Lua still needs).

## Versioning

| Mechanism      | Rule                                                                                                      |
| -------------- | --------------------------------------------------------------------------------------------------------- |
| `ready.v`      | protocol version (`VERSION` in `limits.rs`); exactly `3` today                                           |
| `ready.paints` | `true` once the pane's own render pipeline is live — both roles set it unconditionally                    |
| `ready.caps`   | additive backend features; clients opt in through `auth.caps` where a feature changes the dialogue       |
| Lua records    | `store.proto[pid] = ev.v`, `store.paints[pid] = ev.paints` on every `ready`                               |
| Refuse         | `ev.v != VERSION` or `paints` false → `warn_once` + the same 60s failed-domain block an unanswering backend gets |

## Input parsing rules

The backend reads stdin as a byte stream and demultiplexes:

- `RS VTABS ` at a message boundary starts a framed control record, terminated by `\n`.
- The frame token must establish or prove the current session before its JSON command can run.
- Bare `{`, complete unframed JSON, and prefix-like text are ordinary key input.
- `ESC [ <` … `M`/`m` is an SGR mouse report.
- `ESC [ I` / `ESC [ O` are focus in/out.
- Other CSI/SS3 sequences map to named keys; unknown ones are dropped.
- UTF-8 printable text maps to single-character key events.

Control records may be split across multiple reads; the parser buffers them. A framed record
without a newline is dropped after ~300ms of silence or at the residual-buffer bound: those bytes
and the rest of that physical line are discarded, never re-parsed as key events. A complete atomic
`begin`/sections/`commit` write may exceed the residual bound in aggregate because each physical
record is independently framed and bounded.
`{`, `}` and control bytes never terminate a CSI sequence; they abort it and are
parsed on their own. Consecutive `move`/`drag` mouse reports found in one read chunk are coalesced
before they reach `resolve` — only the last one is acted on.

## Environment

| Env                    | Value                                                                                                                                          |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `VTABS_USERVAR`        | user var name for events \| `vtabs`                                                                                                            |
| `VTABS_BG`             | `#rrggbb` painted before the first paint \| unset (no fill)                                                                                    |
| `VTABS_LOG`            | debug log file, set by the plugin from `backend.env`; appended, 0600, symlinks refused, key names redacted \| unset (no logging, never stderr) |
| `VTABS_PANIC_ON_READY` | `1` panics right after `ready`; debug builds only, to prove the panic hook lands in the log \| unset                                           |

## Roles

| Flag                       | Title marker                 | `vtabs_role` |
| -------------------------- | ---------------------------- | ------------ |
| `--role sidebar` (default) | `wez-vtabs:<nonce>`          | `sidebar`    |
| `--role settings`          | `wez-vtabs-settings:<nonce>` | `settings`   |

## Size changes

| Platform | Source                                   | Fallback                 |
| -------- | ---------------------------------------- | ------------------------ |
| unix     | `SIGWINCH`, wakes the loop at once       | full size poll every 2 s |
| other    | size poll every 250 ms                   | —                        |
| any      | re-read before every frame and fade tick | —                        |

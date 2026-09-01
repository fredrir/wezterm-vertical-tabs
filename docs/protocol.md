# Sidebar backend protocol (v2)

The sidebar is a WezTerm pane running `wez-vtabs` (the Rust backend). Rust owns
layout, paint, hit-testing, the action menu and the settings screen; the
plugin owns mux facts, config and dispatch:

- **backend → Lua**: events, sent as WezTerm user vars
  (`ESC ] 1337 ; SetUserVar=vtabs=<base64(json)> BEL`), received by the plugin
  through the `user-var-changed` event.
- **Lua → backend**: commands, written to the pane's stdin with
  `pane:send_text(json .. "\n")`. Every command is a single JSON object on one
  line, always starting with `{`.

## Startup

1. Enter raw mode, alternate screen, hide cursor.
2. Fill the alternate screen with `VTABS_BG` (skipped when unset), before anything else is drawn.
3. Enable mouse reporting: `?1000h ?1002h ?1003h ?1006h`, focus reporting
   `?1004h`, bracketed paste `?2004h`.
4. Emit user var `vtabs_role` = `sidebar` | `settings` (plain, base64 of the literal string).
5. Set the pane title marker `wez-vtabs:<nonce>` / `wez-vtabs-settings:<nonce>` (OSC 0 and OSC 2).
   The nonce is a per-process random id, never the auth token: window titles are readable by the
   whole desktop.
6. Emit event `{"t":"ready","v":2,"cols":N,"rows":M,"paints":true,"n":1}`.
7. Loop until stdin EOF or `quit`.

`paints:true` is unconditional: both roles paint their own pane from the moment they start.
Nothing is drawn or hit-tested until `config`, `theme` and `model` have each landed at least once.

On exit, restore everything (mouse off, cursor shown, main screen, cooked mode).

## Commands (Lua → backend)

Five messages are idempotent and full *within their domain*: `config`, `theme`, `model` and `menu`
replace the previous one whole; `fx` is a fire-and-forget hint addressed to one pane. Lua encodes
each once per window per message kind and skips a pane whose last-sent string for that kind is
unchanged (`plugin/vtabs/wire.lua`).

| command | shape | effect |
| --- | --- | --- |
| `config` | see below | render/behaviour knobs |
| `theme` | see below | resolved palette + overrides |
| `model` | see below | tabs (sidebar) or fields (settings) |
| `menu` | see below | the action menu, or `{"t":"menu","rev":n,"open":false}` to close |
| `fx` | `{"t":"fx","phase":"expand_in","ms":220,"fps":30}` | animate this pane's rows on the backend's own clock |
| `notice` | `{"t":"notice","level":"warn","text":"..."}` | written to the debug log only — not painted |
| `auth` | `{"t":"auth","token":"<hex>"}` | echo user var `vtabs_token`; the title is not touched |
| `ping` | `{"t":"ping","n":N}` | reply with `pong` echoing `n`; vary it so the reply stays unambiguous |
| `clear` | `{"t":"clear"}` | repaint the pane from the last applied `config`/`theme`/`model`/`menu` |
| `quit` | `{"t":"quit"}` | restore terminal and exit 0 |

Unknown commands are ignored. Malformed JSON lines are ignored.

### config

```json
{"t":"config","rev":3,"desired_width":28,"rail_width":5,"position":"left","collapsed":"rail",
 "icons":true,"icon_map":{"nvim":"󰉯"},"meta":"cwd","meta_sep":" · ","unseen":true,
 "glyphs":{"custom_block":true,"east_asian_wide":false},
 "animate":true,"double_click_ms":300,"tear_off":true,
 "wheel":"scroll","context":"popover","hover_timeout_ms":1200,"ellipsis":"…",
 "popover":{"width":null,"follow_pointer":true,"overflow":null},
 "render":{"meta":true,"padding":{"left":1,"right":1,"top":0,"bottom":0},"frame":false,
   "row_gap":0,"new_tab_button":true},
 "mac":{"integrated_buttons":false,"native_button_style":false,"preview":false,"is_full_screen":false}}
```

`desired_width` is the value `geometry.correct` is converging the pane toward, never the pane's
real width — Rust always lays out against its own terminal size (`cols`/`rows` from `ready`/`resize`).
`wheel == "switch"` switches tabs instead of scrolling; a right-press only arms the menu when
`context == "popover"`; `hover_timeout_ms` expires a stale hover on the runtime's own clock, no
round trip.

### theme

```json
{"t":"theme","rev":7,
 "scheme":{"background":"#1e1e2e","foreground":"#cdd6f4","cursor_bg":"#f5e0dc",
           "selection_bg":"#585b70","active_tab_bg":"#313244",
           "ansi":["#45475a","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#bac2de"],
           "brights":["#585b70","#f38ba8","#a6e3a1","#f9e2af","#89b4fa","#f5c2e7","#94e2d5","#a6adc8"]},
 "overrides":{"accent":"#89b4fa"},
 "elevation":0.06}
```

Only Lua can read `effective_config.resolved_palette`; Rust (`vtabs-theme`) resolves the ~30-entry
`Theme` with contrast enforcement (`backend/crates/vtabs-theme/src/lib.rs`). `overrides` is
`hooks.theme`'s output, pre-resolved to hex by Lua. Every colour on the wire is `#rrggbb`; Rust
parses hex only.

### model

Sidebar pane (`screen:"sidebar"`):

```json
{"t":"model","rev":142,"screen":"sidebar","rail":false,"active":7,
 "focus":{"on":true,"index":3},
 "scroll":{"top":4,"user":true},
 "drag":{"id":7,"active":true,"slot":3,"outside":false,
         "origin":{"x":5,"y":6,"at":1712345678901}},
 "strip":{"rows":1,"cols":28,"buttons":[{"id":"toggle"},{"id":"settings"}]},
 "footer":[{"text":"New tab"}],
 "space":"home",
 "spaces":[{"id":"home","name":"Home","icon":"󰋜","unseen":false},
           {"id":"pi","name":"pi","icon":"󰒋","unseen":true}],
 "tabs":[
   {"id":7,"index":1,"title":"nvim","pane_title":"nvim — x","proc":"nvim",
    "cwd":"~/p/x","host":"pi","user":"f","domain":"local",
    "panes":2,"pinned":false,"private":false,"unseen":false,"zoomed":false}],
 "private":false}
```

`spaces` is the switcher at the sidebar's foot, in order; `space` names the active one and is the
only source of that fact — Rust derives each slot's highlight from it. Both are absent when the
plugin has no spaces, and the row is not drawn. `tabs` already holds only the active space's tabs.

Settings pane (`screen:"settings"`) gets a model of its own instead of the tabs list — its own
`rev` sequence, sent only to the settings-role pane in the window:

```json
{"t":"model","rev":9,"screen":"settings",
 "groups":[{"id":"layout","label":"Layout"}],
 "fields":[{"key":"padding.top","label":"  top","group":"layout","widget":"stepper",
   "value_text":"0","changed":false,"depth":1,"help":"top",
   "locked":{"text":"wezterm.lua"},"editing":{"buffer":"12"},"armed":false}],
 "caveat":["…"],
 "version":"0.1.2"}
```

`focus`, `scroll` and `drag` are Lua-authoritative mirrors: Rust applies the gesture optimistically
and reports the intent on `do`; the next `model` is the truth, and Lua wins on disagreement.
`drag.origin` carries the point the press landed at, because the press may have happened in a
*different* `wez-vtabs` process — one tab's card-body press activates and focuses that tab's own
sidebar pane, so the drag continues in the new pane's backend. Settings `fields[].value_text` is
pre-rendered by Lua (`page.value_text`); Rust never predicts a commit, `min`/`max`-style affordances
live in the widget kind, not in a typed value.

### menu

```json
{"t":"menu","rev":9,"open":true,"level":"root","anchor":{"row":6,"col":12},"target":7,
 "selected":1,
 "header":{"title":"nvim","meta":"~/p/x"},
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

| `level` | items | notes |
| --- | --- | --- |
| `root` (default) | `popover.items()` rows | `danger` on `close`/`close_others` |
| `confirm` | the question in `header.title`, then Close (`danger`)/Cancel | `selected:2` — Cancel is armed |
| `rename` | one `{"id":"rename_field","mode":"edit","value":"<current title>"}` | Rust owns the buffer + cursor |
| `spaces` | one `{"id":"space:<id>","label":<name>,"hint":"<tab count>"}` per space, the current one `disabled` | a sub-level: Esc is `menu_back`; never narrower than the root it came from |

`selected` is adopted only when `(level, items)` changes; otherwise selection is Rust-local.
Lua decides *whether* an item needs confirming (it counts panes via mux); Rust runs the confirm
sub-level, the rename buffer, placement, wrap, scroll and selection.

## Events (backend → Lua)

All events carry `"t"` and a monotonic `"n"` (added by `App::emit`, not part of the serialized
struct), so two identical events in a row are never collapsed by WezTerm's user-var dedup.
Columns/rows are 1-based cell coordinates.

| event | shape |
| --- | --- |
| `ready` | `{"t":"ready","v":2,"cols":N,"rows":M,"paints":true,"n":1}` |
| `resize` | `{"t":"resize","cols":N,"rows":M,"n":2}` |
| `do` | `{"t":"do","a":"press_card","id":7,"args":{"x":5,"y":6,"part":"title"},"n":9}` — the gesture/verb vocabulary, see below |
| `key` | `{"t":"key","key":"c","mods":["ctrl"],"raw":"Aw==","n":10}` — unconsumed keys only, forwarded as-is |
| `paste` | `{"t":"paste","data":"<base64>","n":11}`, or `{"t":"paste","dropped":"size","n":11}` past 64 KiB |
| `focus` | `{"t":"focus","in":true,"n":12}` |
| `pong` | `{"t":"pong","n":13,"echo":7}` — `n` is the normal monotonic counter, `echo` is the ping's own `n` |
| `note` | `{"t":"note","k":"menu_refused","why":"rows","id":7,"a":"confirm","n":14}` |
| `dropped` | `{"t":"dropped","what":"model"\|"menu","reason":"bounds","n":15}` — the command was refused whole, previous state kept |

- `id` on a `do` event is a tab id (number) for tab-addressed verbs, or a string for a strip
  button (`"toggle"`, `"settings"`) and for `switch_space` (the space id); omitted for verbs with
  no target.
- `raw` is the base64 of the exact bytes the key was decoded from (≤ 16), so the plugin can forward
  them to another pane verbatim. Omitted when empty.
- A CSI/SS3 sequence the parser does not name is `"key":"unknown"` with its bytes in `raw`.
- Bracketed paste is one `paste` event, never key events.
- Key names: a single printable character (as typed), or one of
  `enter escape tab backspace delete up down left right home end pageup pagedown space`.
  Control characters map to their letter with `"ctrl"` in `mods` (`0x03` → `c` + ctrl;
  `0x00` → `space` + ctrl; `0x1c`–`0x1f` → `\ ] ^ _` + ctrl; `0x08` → `backspace`).
  A bare `ESC` that is not followed by more bytes within ~30ms is `escape`.
  An `ESC [` / `ESC O` introducer waits ~300ms for its final byte before giving up.
- `note{k:"menu_refused"}` fires when an `open`/`confirm` menu message cannot be placed: `why` is
  `"width"` (pane narrower than `MIN_RENDER_W`) or `"rows"` (too few rows); `a` is the level name
  (`root`/`confirm`/`rename`) so Lua picks its own fallback. Sent once per menu `rev`. At the
  `confirm` level Lua falls through to WezTerm's own confirm overlay; at any other level it closes
  the menu.

## Gesture → event (sidebar)

Ported from `input.lua`'s `on_down`/`on_drag`/`on_up`/`on_wheel` into `vtabs-input::resolve`
(`backend/crates/vtabs-input/src/resolve.rs`); Lua's handlers are `input.lua`'s `DO` table.

| Input | Condition | Event |
| --- | --- | --- |
| left press, card body | not on the ✕/pin span | `do{a:"press_card",id,args:{x,y,part}}` — activates the tab and arms the drag |
| left press, ✕ span | | arms locally; nothing crosses until release |
| left press, pin span | | `do{a:"toggle_pin",id}` |
| middle press, card body | | arms locally (same close arming as ✕) |
| right press, card body | `config.context=="popover"` | arms locally; opens on release |
| motion past the drag threshold | a press is armed (locally or via `model.drag`) | `do{a:"drag_to",args:{x,y,slot,outside}}` |
| release, drag active | | `do{a:"drag_end",args:{slot,outside}}` |
| release, ✕/middle armed and still on the same card | | `do{a:"request_close",id,args:{row,col}}` |
| release, right armed and still on the same card | | `do{a:"open_menu",id,args:{row,col}}` |
| double left press, off-card / space / strip | within `double_click_ms` | `do{a:"new_tab"}` |
| left press, strip button | | `do{a:"strip",id:<button id>}` |
| left press, new-tab ghost row | | `do{a:"new_tab"}` |
| left press, footer row | | `do{a:"footer",args:{index}}` — index into the sent model's footer list |
| left press, a space icon | | `do{a:"switch_space",id:<space id>}`; a lone visible icon targets the next space, so it cycles |
| double press on the switcher row | | nothing — never `new_tab` |
| wheel over the switcher row | either wheel mode | `do{a:"switch_space",id:<neighbour>}`, no wrap, silent at the ends, nothing applied early |
| wheel | `config.wheel=="scroll"` | `do{a:"set_scroll",args:{top,user:true}}` (optimistic local apply) |
| wheel | `config.wheel=="switch"` | `do{a:"wheel_tab",args:{dy}}` |
| release, no close/menu/drag outcome, `config.hover=="press"` | | `do{a:"blur_sidebar"}` |
| any other key | focus mode off | `key{...}` (Lua forwards to the content pane or drops it) |

Focus mode (`model.focus.on`), keyed entirely in Rust:

| Key | Event |
| --- | --- |
| `j`/`down`/`tab`, `k`/`up` (shift reverses) | `do{a:"set_focus_index",args:{index}}` |
| `home`/`g`, `end`/`G` | `do{a:"set_focus_index",args:{index:1\|count}}` |
| `1`-`9`, `enter`/`space` | `do{a:"activate_tab_by_id",id}` |
| `x`/`d`/`delete` | `do{a:"request_close",id,args:{row,from_key:true}}` |
| `p` | `do{a:"toggle_pin",id}` |
| `m` | `do{a:"open_menu",id,args:{row}}` |
| `escape`/`q`/ctrl-`c` | `do{a:"blur_sidebar"}` |
| anything else (`r`, `J`, `K`, …) | falls through as a plain `key` event; Lua's own focus branch handles it |

## Gesture → event (menu)

The menu owns the pane while it is open — every pointer and key event over it is the menu's; the
sidebar's own hit map is not consulted.

| Input | Condition | Event |
| --- | --- | --- |
| left press, outside the menu's columns | including a row it doesn't cover at all | `do{a:"menu_closed"}` |
| left press, an enabled row | not `danger` | `do{a:"menu_pick",args:{id}}` |
| left press, a `danger` row | | arms locally; picked on release over the same row |
| left release, armed danger row, still over it | | `do{a:"menu_pick",args:{id}}` |
| right press, a row outside the menu's rows (scrim) | | `do{a:"menu_closed"}`, then the click falls through to the sidebar |
| wheel | | moves the selection locally; no event |
| enter/space | selected item enabled | `do{a:"menu_pick",args:{id}}` |
| escape/ctrl-c, root level | | `do{a:"menu_closed"}` |
| escape/ctrl-c, sub-level | | `do{a:"menu_back"}` |
| `j`/`down`/`tab`, `k`/`up` (shift reverses) | | moves the selection locally |
| a single character | jump-to-letter | moves the selection locally |
| enter, rename level | | `do{a:"rename_commit",args:{text}}` |
| escape, rename level | | `do{a:"menu_back"}` |

Pointer drift (`Move`/`Drag`) never selects a `danger` row at the `confirm` level — a Rust-tested
invariant. A destructive item acts only on release, and only when press and release land on the
same row.

## Gesture → event (settings)

Nav, the filter and the armed-chord state are Rust-local; only a commit crosses. Ported into
`vtabs-input::resolve::settings_key`/`settings_mouse`.

| Key/click | Event |
| --- | --- |
| `left`/`right` on a focused field, or a click on its `-`/`+` glyph | `do{a:"nudge_option",args:{key,delta}}` |
| `enter`/`space`, or a left click on a field's value | `do{a:"activate_option",args:{key}}` — Lua decides commit vs. enter-edit vs. arm-chord |
| `r` | `do{a:"reset_option",args:{key}}` — delete-or-restore |
| `c` | `do{a:"settings_copy"}` |
| any key while editing a buffer | `do{a:"edit_key",args:{key}}` |
| any key while a chord recorder is armed | `do{a:"record_chord",args:{key,mods}}` |
| `escape`, or `q` with no modifiers | `do{a:"close_settings"}` — only outside a modal mode (the `q`/Escape hijack fix) |
| `j`/`down`, `k`/`up`, `tab`, `/` + filter typing, a click on a nav tab | moves focus/filter/group locally; no event |

Rust never predicts a commit: every verb above sends intent, and the next `model` is authoritative.

## Bounds

| Bound | Value | On breach |
| --- | --- | --- |
| `model.tabs` | 200 (`MODEL_MAX_TABS`) | `dropped{what:"model",reason:"bounds"}`; previous model kept |
| `model.fields` | 512 (`MODEL_MAX_FIELDS`) | `dropped{what:"model",reason:"bounds"}`; previous model kept |
| `model.spaces` | 32 (`MODEL_MAX_SPACES`, mirrored to Lua, which refuses at the cap) | `dropped{what:"model",reason:"bounds"}`; previous model kept |
| `menu.items` | 64 (`MENU_MAX_ITEMS`) | `dropped{what:"menu",reason:"bounds"}`; previous menu kept |
| any command line | 64 KiB (`LINE_MAX`, `limits.rs`) | the documented contract; the actual backstop is the stdin buffer below |
| stdin buffer | 1 MiB, or ~300ms with no `\n` | the line is discarded silently — never replayed as key events, no event emitted |
| paste payload | 64 KiB | `{"t":"paste","dropped":"size"}` |

Every bound lives in `backend/crates/vtabs-protocol/src/limits.rs`, the one home both languages
read (`just gen-protocol` mirrors what Lua still needs).

## Versioning

| Mechanism | Rule |
| --- | --- |
| `ready.v` | protocol version (`VERSION` in `limits.rs`); `2` today |
| `ready.paints` | `true` once the pane's own render pipeline is live — both roles set it unconditionally |
| Lua records | `store.proto[pid] = ev.v`, `store.paints[pid] = ev.paints` on every `ready` |
| Refuse | `ev.v < 2` or `paints` false → `warn_once` + the same 60s failed-domain block an unanswering backend gets |

## Input parsing rules

The backend reads stdin as a byte stream and demultiplexes:

- `{` at a message boundary starts a command line, terminated by `\n`.
- `ESC [ <` … `M`/`m` is an SGR mouse report.
- `ESC [ I` / `ESC [ O` are focus in/out.
- Other CSI/SS3 sequences map to named keys; unknown ones are dropped.
- UTF-8 printable text maps to single-character key events.

Command lines may be split across multiple reads; the parser must buffer.
A `{` line without a newline is dropped after ~300ms of silence or 1 MiB: those
bytes and the rest of that line are discarded, never re-parsed as key events.
`{`, `}` and control bytes never terminate a CSI sequence; they abort it and are
parsed on their own. Consecutive `move`/`drag` mouse reports found in one read chunk are coalesced
before they reach `resolve` — only the last one is acted on.

## Environment

| Env | Value |
| --- | --- |
| `VTABS_USERVAR` | user var name for events \| `vtabs` |
| `VTABS_BG` | `#rrggbb` painted before the first paint \| unset (no fill) |
| `VTABS_LOG` | debug log file, appended; 0600, symlinks refused, key names redacted \| unset (no logging, never stderr) |

## Roles

| Flag | Title marker | `vtabs_role` |
| --- | --- | --- |
| `--role sidebar` (default) | `wez-vtabs:<nonce>` | `sidebar` |
| `--role settings` | `wez-vtabs-settings:<nonce>` | `settings` |

The role changes nothing on the wire: commands and events are identical for both. An unknown role
logs and falls back to `sidebar`. The plugin treats a settings-role pane as content for tab-list
purposes — it is never adopted as a tab list and never closed as an orphan — but it **is**
authenticated exactly like a sidebar pane: its `ready` branch calls `sidebar.auth(pane)` too
(`plugin/vtabs/input.lua`). Both markers are stripped from the titles the sidebar renders.

## Size changes

| Platform | Source | Fallback |
| -------- | ------ | -------- |
| unix     | `SIGWINCH`, checked every 100 ms | full size poll every 2 s |
| other    | size poll every 250 ms | — |

# Sidebar backend protocol

The sidebar is a WezTerm pane running `wez-vtabs` (the Rust backend). The Lua
plugin owns all state and rendering; the backend is a thin terminal I/O bridge:

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
4. Emit user var `vtabs_role` = `sidebar` (plain, base64 of the literal string).
5. Set the pane title marker `wez-vtabs:<nonce>` (OSC 0 and OSC 2). The nonce is
   a per-process random id, never the auth token: window titles are readable by
   the whole desktop.
6. Emit event `{"t":"ready","v":1,"cols":N,"rows":M}`.
7. Loop until stdin EOF or `quit`.

On exit, restore everything (mouse off, cursor shown, main screen, cooked mode).

## Events (backend → Lua)

All events carry `"t"`. Columns/rows are 1-based cell coordinates.

| event  | shape                                                                                                                                                |
| ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| ready  | `{"t":"ready","v":1,"cols":N,"rows":M}` — `v` is the protocol version                                                                               |
| resize | `{"t":"resize","cols":N,"rows":M}`                                                                                                                   |
| mouse  | `{"t":"mouse","k":"down"\|"up"\|"drag"\|"move"\|"wheel","b":"left"\|"middle"\|"right"\|"none","x":C,"y":R,"dy":-1\|1,"mods":["shift","ctrl","alt"]}` |
| key    | `{"t":"key","key":"<name>","mods":["shift","ctrl","alt"],"raw":"<base64>"}`                                                                          |
| focus  | `{"t":"focus","in":true\|false}`                                                                                                                     |
| paste  | `{"t":"paste","data":"<base64>"}`, or `{"t":"paste","dropped":"size"}` past 64 KiB                                                                   |
| pong | `{"t":"pong","n":N}` — echoes the ping's `n` (omitted when the ping had none) |
| anim_done | `{"t":"anim_done","id":N}` — after the last frame, or at once when a command cancels the run |
| dropped | `{"t":"dropped","what":"anim","reason":"size"\|"bounds"}` — the command was refused whole |

- `dy` is only present for `wheel` (`-1` = up, `1` = down); `b` is `"none"` for wheel.
- `drag` = motion with a button held (SGR bit 32 + button), `move` = motion with no button.
- Consecutive `move`/`drag` events found in one read chunk are coalesced: only the last one is emitted.
- `mods` is omitted when empty.
- `raw` is the base64 of the exact bytes the key was decoded from (≤ 16), so the
  plugin can forward them to another pane verbatim. Omitted when empty.
- A CSI/SS3 sequence the parser does not name is `"key":"unknown"` with its
  bytes in `raw`; unrecognised mouse reports are dropped instead.
- Bracketed paste is one `paste` event, never key events. This parser does not
  interpret those bytes; what the pane they are forwarded to makes of them is
  that app's business.
- Key names: a single printable character (as typed), or one of
  `enter escape tab backspace delete up down left right home end pageup pagedown space`.
  Control characters map to their letter with `"ctrl"` in `mods` (`0x03` → `c` + ctrl;
  `0x00` → `space` + ctrl; `0x1c`–`0x1f` → `\ ] ^ _` + ctrl; `0x08` → `backspace`).
  A bare `ESC` that is not followed by more bytes within ~30ms is `escape`.
  An `ESC [` / `ESC O` introducer waits ~300ms for its final byte before giving up.
  Buttons 8+ report `"b":"none"`; horizontal wheel events are dropped.

## Commands (Lua → backend)

| command | shape                        | effect                                      |
| ------- | ---------------------------- | ------------------------------------------- |
| frame   | `{"t":"frame","data":"..."}` | write `data` verbatim to stdout, then flush |
| clear   | `{"t":"clear"}`              | clear screen, cursor home                   |
| quit    | `{"t":"quit"}`               | restore terminal and exit 0                 |
| ping | `{"t":"ping","n":N}` | reply with `pong` carrying the same `n`; WezTerm only fires `user-var-changed` when the value changes, so `n` must vary |
| auth | `{"t":"auth","token":"<hex>"}` | echo user var `vtabs_token`; the title is not touched |
| anim | `{"t":"anim","id":N,"ms":220,"fps":30,"ease":"outCubic","dir":"in","anchor":"#1e1e2e","rows":[{"y":4,"delay":0}],"data":"<final frame>"}` | interpolate to `data` on the backend's clock |

### anim

| field | meaning |
| --- | --- |
| `data` | the final frame for the animated rows, exactly as a `frame` payload: one `ESC[<row>;1H` per row |
| `anchor` | `#rrggbb` every animated colour is interpolated against |
| `dir` | `"in"` anchor → `data` (default), `"out"` `data` → anchor |
| `rows[].y` | row to animate; a row in `data` that is not listed is written unchanged |
| `rows[].delay` | per-row stagger in ms; row-local `t = clamp((elapsed - delay) / ms, 0, 1)` |
| `ease` | `linear` (default) \| `outCubic` \| `inOutQuad`; unknown → `linear` |
| `fps` | 15–60, default 30; frames are generated, so the payload never grows with it |

| rule | detail |
| --- | --- |
| start | the `t = 0` frame is written synchronously with the command |
| cancel | a new `anim`, or any `frame` / `clear` / `quit`, ends the run and emits `anim_done` for it |
| catch-up | a late wake generates the frame for *now*; skipped ticks are never replayed |
| self-containment | every generated frame redraws all animated rows, so skipping is safe |
| termination | the last frame is `data` verbatim, so the terminal ends exactly where Lua asked |
| bounds | `data` ≤ 24 KiB, `rows` 1–128, `ms` 1–2000, `fps` 15–60, `anchor` `#rrggbb` |
| refusal | outside those bounds nothing plays: one `dropped` event, `reason` `size` for `data`, `bounds` for the rest |
| colours | only `ESC[38;2;R;G;Bm` and `ESC[48;2;R;G;Bm` are rewritten; text, CUP, bold and reset pass through byte for byte, including any prefix before the first CUP |

`data` ≤ 24 KiB covers a 120-row sidebar at ~180 bytes per row. The bound exists because `pane:send_text` is a blocking write on the pty master and a
blocking RPC on a mux domain: it keeps the GUI thread's stall short.

`data` contains raw ANSI (CUP, SGR, …); it never contains a newline.
Unknown commands are ignored. Malformed JSON lines are ignored.

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
parsed on their own.

## Environment

## Roles

| Flag | Title marker | `vtabs_role` |
| --- | --- | --- |
| `--role sidebar` (default) | `wez-vtabs:<nonce>` | `sidebar` |
| `--role settings` | `wez-vtabs-settings:<nonce>` | `settings` |

The role changes nothing else: frames, keys with `raw`, paste, `anim`, `ping`/`auth` and every
event are identical. An unknown role logs and falls back to `sidebar`. The plugin treats a
settings-role pane as content — it is never adopted as a tab list, never authenticated, and
never closed as an orphan — and strips both markers from the titles it renders.

| Env             | Value                                                                            |
| --------------- | -------------------------------------------------------------------------------- |
| `VTABS_USERVAR` | user var name for events \| `vtabs`                                               |
| `VTABS_BG`      | `#rrggbb` painted before the first frame \| unset (no fill)                       |
| `VTABS_LOG`     | debug log file, appended; 0600, symlinks refused, key names redacted \| unset (no logging, never stderr) |

## Size changes

| Platform | Source | Fallback |
| -------- | ------ | -------- |
| unix     | `SIGWINCH`, checked every 100 ms | full size poll every 2 s |
| other    | size poll every 250 ms | — |

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
3. Enable mouse reporting: `?1000h ?1002h ?1003h ?1006h`, and focus reporting
   `?1004h`.
4. Emit user var `vtabs_role` = `sidebar` (plain, base64 of the literal string).
5. Set the pane title marker `wez-vtabs:0` (OSC 0 and OSC 2); `auth` replaces
   `0` with the token.
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
| pong | `{"t":"pong","n":N}` — echoes the ping's `n` (omitted when the ping had none) |

- `dy` is only present for `wheel` (`-1` = up, `1` = down); `b` is `"none"` for wheel.
- `drag` = motion with a button held (SGR bit 32 + button), `move` = motion with no button.
- Consecutive `move`/`drag` events found in one read chunk are coalesced: only the last one is emitted.
- `mods` is omitted when empty.
- `raw` is the base64 of the exact bytes the key was decoded from (≤ 16), so the
  plugin can forward them to another pane verbatim. Omitted when empty.
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
| auth | `{"t":"auth","token":"<hex>"}` | set pane title `wez-vtabs:<token>` (OSC 0 + OSC 2, non-hex stripped) and echo user var `vtabs_token` |

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

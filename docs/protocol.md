# Sidebar backend protocol

The sidebar is a WezTerm pane running `wez-vtabs`. Rust owns layout, paint, hit-testing, spaces
policy, theme resolution, the action menu, and the settings document. Lua owns the WezTerm API
boundary, mux facts, host mutations, and user-hook execution.

| Direction | Transport |
| --------- | --------- |
| backend → Lua | WezTerm user variable `vtabs`, containing base64-encoded JSON |
| Lua → backend | one framed command per line: `RS VTABS <token> <compact-json>\n`, where `RS` is byte `0x1e` |

The generated constants and intent inventory in `plugin/vtabs/gen/protocol.lua` are derived from
`vtabs-protocol`. Run `just generate` after changing the Rust contract.

## Session startup

The backend enters raw mode, switches to the alternate screen, hides the cursor, configures mouse,
focus, and paste reporting, then publishes its role and readiness:

```json
{
  "t": "ready",
  "cols": 28,
  "rows": 40,
  "pane": 42,
  "transport": { "inbox": "inbox-42-9f3a1b2c" },
  "n": 1
}
```

`pane` is the backend's server pane id. `transport` is absent when no inbox can be offered.

The first accepted command is an authenticated handshake:

```json
{"t":"auth","token":"<session-token>","keys":"server"}
```

The frame token and JSON token must match. Later records must prove the active session with their
frame token. An authenticated `auth` may rotate the token. Tokens are non-empty ASCII graphic
strings of at most 64 bytes. `keys:"server"` asks the backend to forward eligible key bytes through
the server's CLI bridge.

If a GUI attaches to a surviving mux pane without its user variables, its first token cannot match.
The backend republishes the token it holds at most once a second, allowing the plugin to authenticate
on the next attempt without treating the pane as content.

## Atomic publication

All semantic state crosses one transaction path:

```json
{"t":"begin"}
{"t":"config", ...}
{"t":"theme", ...}
{"t":"spaces", ...}
{"t":"model", ...}
{"t":"menu", ...}
{"t":"commit"}
```

A settings pane sends `settings` in place of `model` and does not send `menu`. A sidebar's first
published state requires `config`, `theme`, `spaces`, `model`, and `menu`; a settings pane requires
`config`, `theme`, `spaces`, and `settings`. Later `begin` commands clone committed sections so only
changed sections need to be staged. Nothing staged is painted or hit-tested before the matching
commit.

Only one transaction may be pending in a pane. A new `begin` replaces an incomplete transaction,
but a transaction waiting for a host hook must finish or time out before another can start. Invalid,
oversized, or retired shapes are rejected rather than adapted, and an invalid staged section
prevents that transaction from publishing.

Theme and route hooks pause the commit. A timeout completes deterministically with the resolved base
theme or nil route answers, then disables that hook for the authenticated session. This prevents a
late untagged answer from attaching to a later transaction.

Changing the auth token clears committed and pending sections, menu state, and pointer state before
the backend announces a fresh `ready`.

## Commands

| Command | Required fields | Effect |
| ------- | --------------- | ------ |
| `auth` | `token`; optional `keys` | establish or rotate the session |
| `begin` / `commit` | none | stage or publish one transaction |
| `config` | normalized render and behavior fields | stage renderer configuration |
| `theme` | `private`, palette/overrides, `hook` | stage raw theme facts |
| `spaces` | `window_id`, definitions and full tab census | stage routing and visibility facts |
| `model` | per-pane interaction and strip state | stage sidebar state |
| `settings` | canonical values and host facts | stage the settings document |
| `menu` | open state, subject and items | stage the action menu |
| `theme_hook_result` | `overrides` | answer the pending theme request |
| `space_route_hook_result` | `routes` | answer the pending routing request |
| `fx` | `phase`; optional `ms`, `fps` | run a pane animation |
| `notice` | `text`; optional `level` | append a diagnostic to the private log |
| `ping` | optional `n` | emit `pong` |
| `clear` / `quit` | none | repaint, or restore the terminal and exit |
| `kill` | `pane` | kill that server pane by id |
| `rescue` | `band`, `position` | move content panes out of the sidebar band |
| `adjust` | `target`, `min_content` | resize the backend's split from server-side facts |
| `transport_probe` / `transport_barrier` / `transport_stop` | `session` | negotiate or stop the inbox |

`kill`, `rescue`, and `adjust` use the server's own `wezterm cli --no-auto-start`, found through
`WEZTERM_EXECUTABLE_DIR`, `WEZTERM_UNIX_SOCKET`, and `WEZTERM_PANE`. This avoids acting through a
GUI mux mirror whose split tree may lag the server.

### Payload ownership

`config` contains only normalized fields. The public option schema and defaults are documented in
[`configuration.md`](configuration.md); the wire representation is intentionally not a second
configuration API.

`theme` carries the effective WezTerm palette, canonical overrides, the required window-private
flag, and whether `hooks.theme` is configured. Rust applies fallbacks, mixing, active-space layers,
and contrast enforcement. When a hook is needed it emits the full resolved base:

```json
{"t":"theme_hook_request","theme":{"bg":[23,23,35],"fg":[205,214,244]}}
```

Lua answers with canonical overrides. Rust resolves again, commits once, emits `theme_resolved`, and
paints once.

`spaces` is the sole full-window tab census. It carries definitions, raw tab facts, the active tab
and space, sticky assignments, and admitted dynamic spaces. Rust validates definitions, matches
rules, expands templates, owns routing and visibility, builds summaries, and selects the active
theme layer. Hook cache misses produce one `space_route_hook_request`; the answer must contain every
requested tab id exactly once.

`model` contains only pane-local sidebar state:

```json
{
  "t": "model",
  "rail": false,
  "active": 7,
  "focus": { "on": true, "index": 3 },
  "scroll": { "top": 4, "user": true },
  "strip": {
    "dpi": 144,
    "chrome": { "is_mac": true, "integrated_buttons": true },
    "buttons": [{ "id": "toggle_sidebar" }, { "id": "open_settings" }]
  },
  "footer": [{ "text": "New tab" }]
}
```

Rust combines it with the spaces result and theme-private state to build the renderer model. The
pane measures its own cells and pixels; Lua supplies only window DPI and chrome facts.

`settings` carries JSON-safe values, explicit dotted paths, host-owned keys, opaque paths, key
defaults, platform state, and the plugin release. Rust owns validation, mutation, presentation, the
changed set, and deterministic persistence JSON. Lua restores non-JSON config-as-code values by
reference at the host boundary and performs the final atomic private write.

`menu` carries state and subject ids, never presentation headers. Rust derives headers from the same
raw tab facts and title/meta policy used to render cards.

## Events

Every event receives a monotonically increasing `n` at emission.

| Event | Purpose |
| ----- | ------- |
| `ready` | pane size/id and optional inbox offer |
| `resize` | one size report after a resize burst settles |
| `intent` | flattened, typed renderer action selected by `a` |
| `key`, `paste` | guarded forwarding while sidebar focus mode is off |
| `theme_hook_request`, `space_route_hook_request` | host hook requests for the pending commit |
| `theme_resolved`, `spaces_resolved` | committed Rust policy results |
| `settings_commit`, `settings_copy` | canonical settings mutation or paste-ready Lua |
| `menu_refused` | typed refusal with optional reason/subject fields |
| `transport_ready`, `transport_refused` | inbox negotiation result |
| `pong` | ping response, with the request value in `echo` |
| `dropped` | bounded input, timeout, or inbox-gap report |
| `cli` | result of `kill`, `rescue`, or `adjust`; adjust may include measured `cols` |

Intents carry only fields meaningful to their action. Examples:

```json
{"t":"intent","a":"press_card","tab_id":7,"x":5,"y":6,"part":"title","n":9}
{"t":"intent","a":"set_rail_reserve","cols":9,"n":10}
{"t":"menu_refused","why":"rows","id":7,"level":"confirm","n":11}
```

Settings navigation, editing, validation, and mutation remain in Rust. A successful mutation emits
`settings_commit` with segmented paths, canonical set/remove changes, apply mode, and
the complete persistence JSON. Lua applies that result without rerunning policy.

## Inbox transport

Same-machine unix mux panes can avoid blocking `pane:send_text` calls by using a per-session inbox.

| Property | Rule |
| -------- | ---- |
| root | `VTABS_INBOX_ROOT`, chosen beneath the runtime or temporary directory |
| session directory | backend-created `0700` directory announced by `ready.transport.inbox` |
| message | zero-padded sequence file, written to a temporary name and atomically renamed |
| order | probe is sequence 1; no inbox message is applied before the stdin barrier |
| limits | `LINE_MAX` per record, six times that per file; a gap older than 100 ms is dropped |
| wakeup | inotify or kqueue, with bounded polling fallback |
| cleanup | processed files are deleted; the directory is removed on stop, re-auth, or exit |

A write, close, or rename failure drops the frame and makes Lua send `transport_stop`; stdin remains
the safe fallback. The backend never replays lost bytes as keyboard input.

## Input and bounds

The stdin parser distinguishes framed commands, SGR mouse reports, focus events, CSI/SS3 keys, and
UTF-8 text. Complete unframed JSON and prefix-like text are ordinary keyboard input. Control records
may span reads, but each record must end in a newline before the residual timeout or size bound.
Consecutive pointer-motion reports in one read chunk are coalesced before hit-testing.

| Bound | Value |
| ----- | ----- |
| tabs in `spaces` | `MODEL_MAX_TABS` (200), with unique ids |
| generated settings fields | `MODEL_MAX_FIELDS` (512) |
| dynamic spaces and last-tab rows | `MODEL_MAX_SPACES` (32), with unique ids |
| menu items | `MENU_MAX_ITEMS` (64) |
| JSON control payload | `LINE_MAX` (1 MiB), excluding framing |
| stdin residual buffer | 2 MiB or about 300 ms without a newline |
| persisted settings body | `SETTINGS_BODY_MAX_BYTES` (512 KiB), inclusive |
| paste payload | 64 KiB |

The bounds live in `backend/crates/vtabs-protocol/src/limits.rs` and are mirrored into Lua by
`just gen-protocol`.

## Environment and roles

| Name | Meaning |
| ---- | ------- |
| `VTABS_USERVAR` | event user-variable name; defaults to `vtabs` |
| `VTABS_BG` | initial `#rrggbb` fill before the first committed paint |
| `VTABS_LOG` | private debug log; mode 0600, symlinks refused, sensitive keys redacted |
| `VTABS_PANIC_ON_READY` | debug-only readiness panic used to test the panic hook |

| Role | Process flag | Title marker | `vtabs_role` |
| ---- | ------------ | ------------ | ------------ |
| sidebar | `--role sidebar` | `wez-vtabs:<nonce>` | `sidebar` |
| settings | `--role settings` | `wez-vtabs-settings:<nonce>` | `settings` |

The title nonce identifies pane role but is never an authorization token. Pane termination uses the
server pane id from `ready`.

On Unix, `SIGWINCH` is adopted after the burst pauses for 40 ms, no later than 150 ms from the first
frame, with a two-second full-size poll as fallback. Other platforms poll every 250 ms. Every paint
and animation tick rechecks the terminal size.

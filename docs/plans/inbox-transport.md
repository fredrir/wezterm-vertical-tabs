# Inbox transport for same-machine mux panes

Status: plan, revision 4, implemented (uncommitted). Reviewed by Codex (rev 1, rejected) and an independent
Claude session (rev 2, approve with changes). Owner decisions folded in: fresh token per reload;
every mux-pane operation covered, not only `send_text`.

## Problem

| Name | Value |
| --- | --- |
| Blocking call | `pane:send_text` on a client pane: `wezterm-client/src/pane/clientpane.rs:651` `PaneWriter::write` → `block_on`; the only `block_on` in the client crate |
| Observed | GUI deadlocked twice inside it during a mirror-rebuild storm (`/Library/Logs/DiagnosticReports/wezterm-gui_*.hang`, 2026-09-02 23:28, 2026-09-03 00:07) |
| Mitigation today | `plugin/vtabs/link.lua`: 300 ms hold per busy domain (d673c99); narrows the window, does not close it |
| Goal | no frame to a same-machine mux pane crosses the link on the GUI thread; every other mux-pane operation routed where one change is one rebuild |

## Mux-pane operations

Verified against WezTerm 78cd82db: `lua-api-crates/mux/src/pane.rs`, `tab.rs`,
`wezterm-gui/src/scripting/guiwin.rs`, `wezterm-client/src/pane/clientpane.rs`.

| Operation | Plugin site | Link behaviour | Route |
| --- | --- | --- | --- |
| `pane:send_text` frames to a sidebar | `sidebar_identity.lua` `send_raw` | `block_on` per call | inbox transport |
| `content:send_text` forwarded key | `input.lua` `forward_key` | `block_on` per key | backend forwards server-side (`cli send-text --no-paste`) and emits `key` with `delivered:true`; Lua only hands focus over; local domains unchanged |
| `content:paste` | `input.lua` `forward_paste` | `send_paste`, spawned RPC | keep |
| `base:split` attach | `sidebar_attach.lua` `attach` | `async_split`, awaited RPC, no block | keep; never while `link.busy(domain)` |
| `pane:activate`, `tab:activate` | many | local mux mutation, `advise_focus` spawned | keep |
| `perform_action` `AdjustPaneSize`, `ActivatePaneByIndex` | `geometry.lua` | async assignment; already server-side on mux | keep |
| `perform_action` `CloseCurrentPane`, `CloseCurrentTab` | `sidebar_attach.lua` `close_by_activation`, `settings.lua` `close` | async; the path the mux client has aborted after | mux: `quit` to every backend in the tab, then `kill` by server pane id via any backend on that server; activation close only with no backend left on the server |
| `perform_action` `MoveTab` | `actions.lua` | async | keep |
| `set_inner_size`, `move_to_new_window`, `inject_output` | `sidebar_attach.lua`, `actions.lua`, `input.lua` | local or async | keep |

## Decisions

| Name | Value |
| --- | --- |
| Transport | maildir-style inbox directory per backend session; one file per `send_raw` batch; write `<seq>.tmp`, close, `os.rename` to `<seq>.msg` |
| Rejected | shared append log (partial visibility, writer interleaving, unbounded growth); FIFO (Lua cannot open or write one without blocking when the reader is gone) |
| Eligible panes | `mux.domain(pane) ~= "local"` and `backend.is_local(domain, host)` |
| Local-domain panes | keep `send_text` (direct PTY write, no round trip) |
| Remote panes | keep `send_text` behind `link.lua` |
| Root | Lua chooses, passes `VTABS_INBOX_ROOT` at spawn: `$XDG_RUNTIME_DIR/wez-vtabs`, else `$TMPDIR/wez-vtabs`, else no transport; never `/tmp`; one `util.runtime_dir()` shared with `frame.lua` |
| Root validation | backend `mkdir 0700`, then `lstat`: directory, not symlink, owner uid, no group/other write |
| Session id | inbox basename `inbox-<pid>-<nonce>`, `^[%w%-]+$`, ≤ 64 bytes; no separate epoch |
| Ordering | Lua renames the probe before sending the barrier on stdin; the barrier triggers one scan; probe present → ack, absent → refuse |
| Commit point | Lua switches only on `transport_ready` carrying the session id it is negotiating |
| Sequence | `<seq>` zero-padded 8 digits, monotonic per session, starts at 1 (the probe); backend applies in order, deletes `seq <= last` unprocessed |
| Gap | 100 ms grace, then the missing seq is one lost message: backend emits `dropped`, advances, Lua `wire.reset_pane`; the transport stays active |
| Failed send | tmp write, close or rename error → remove tmp, return false, no replay; a message is delivered iff renamed |
| Leaving `active` | Lua `transport_stop` → backend drains the directory in order, then stops; Lua flushes its queue through stdin on abandonment; `wire.reset_pane` on any loss |
| Negotiation bound | 64 queued frames or 2 s without ack → abandon, flush queue via stdin |
| Decoder | `parse_control_line` per record only, ≤ `LINE_MAX` per record, ≤ 6 × `LINE_MAX` per file; never the stdin `Parser` |
| Wake | inotify `IN_MOVED_TO` (Linux) / kqueue `EVFILT_VNODE` on the dir fd (macOS) via `libc`; scan every 50 ms without a watcher, every 1 s with one |
| Reload | tokens live in the Lua VM only: a reload mints a fresh token per pane, `auth` → backend `reset_for_auth` → new `ready` → new negotiation; one clear per sidebar per reload, by owner decision |
| Fresh GUI | adoption mints a new token → same path |
| Server pane id | `ready` carries `pane` (the backend's `WEZTERM_PANE`); Lua keeps `store.server_pane[pid]` for `kill` by id |
| Cleanup | backend deletes each processed message; removes the dir on `quit`, `transport_stop`, new `auth`; at create, removes sibling `inbox-<pid>-*` whose pid is dead (`kill(pid, 0) == ESRCH`) and `*.tmp` older than 10 min; `scan()` never deletes `*.tmp` |
| Gates | `view.sync`, the remote adjust in `geometry.lua` and attach skip only what would use `send_text` or `split` on a busy domain; active panes publish and adjust at once |
| Config | `backend.inbox = true`; `false` disables the offer |
| Capability | `inbox_transport`, additive; old clients keep stdin |

## Protocol

| Message | Carried by | Direction |
| --- | --- | --- |
| `ready` + `"pane":N` + `"transport":{"inbox":"inbox-4242-9f3a"}` | user var | backend → Lua |
| `{"t":"transport_probe","session":"inbox-4242-9f3a"}` | inbox `00000001.msg` | Lua → backend |
| `{"t":"transport_barrier","session":"…"}` | stdin, last stdin frame | Lua → backend |
| `{"t":"transport_ready","session":"…"}` | user var | backend → Lua |
| `{"t":"transport_refused","session":"…","why":"probe"}` | user var | backend → Lua |
| `{"t":"transport_stop","session":"…"}` | stdin | Lua → backend |
| `{"t":"dropped","what":"message","seq":N}` | user var | backend → Lua |
| `{"t":"kill","pane":N}` | any transport | Lua → backend, alongside `kill` by `title` |
| `{"t":"key",…,"delivered":true}` | user var | backend → Lua, mux domains with a usable cli |
| `quit` | stdin, always | Lua → backend |

Message content: framed control records exactly as `send_raw` writes them today
(`CONTROL_PREFIX token record\n`, one or more).

## Backend

| File | Change |
| --- | --- |
| `crates/vtabs-protocol/src/event.rs` | `Ready.pane: Option<u64>`, `Ready.transport: Option<Transport>`; `TransportReady`, `TransportRefused`; `Dropped { what: "message", seq }`; `Key.delivered: bool` |
| `crates/vtabs-protocol/src/command.rs` | `TransportProbe`, `TransportBarrier`, `TransportStop`, each `{ session }`; `Kill { title: Option<String>, pane: Option<u64> }` |
| `crates/vtabs-runtime/src/inbox.rs` (new) | `Inbox::create(root)`: validate root, mkdir session dir 0700, sweep dead siblings; `scan() -> Vec<(u32, Vec<u8>)>` sorted, caps enforced, other files deleted except `*.tmp`; `remove(seq)`; `drain()`; `Drop` best effort |
| `crates/vtabs-runtime/src/inbox.rs` | reader thread: watcher fd or timeout → `Wake::Inbox(messages)` on the existing channel |
| `crates/vtabs-runtime/src/run.rs` | `Wake::Inbox`; `VTABS_INBOX_ROOT` → `Inbox::create`; decode with `parse_control_line` only |
| `crates/vtabs-runtime/src/app.rs` | `transport: Off \| Negotiating { session } \| Active { session, last_seq, gap_since }`; barrier → one scan → ready or refused; stop → drain then off; new `auth`/`quit` → off and remove dir |
| `crates/vtabs-runtime/src/app.rs` | key forwarding: on a mux domain with `cli`, `cli.send_text(content, bytes)` then `Key { delivered: true }`; content pane = the pane spanning the content column of the own tab, as `adjust_plan` finds it |
| `crates/vtabs-runtime/src/cli.rs` | `send_text(pane, bytes)` → `cli send-text --no-paste --pane-id`; `kill_pane(pane)` → `cli kill-pane --pane-id` |
| `crates/vtabs-runtime/src/inbox.rs` | `libc` inotify / kqueue setup, ~50 lines each |
| tests | scan order under shuffled creation; duplicate deleted unprocessed; gap → `dropped` after grace; oversized file deleted; barrier with and without probe; drain order on stop; `*.tmp` untouched; root refused when symlink / wrong owner / group-writable; decoder rejects non-control bytes; 50 ms scan with watcher disabled; key forwarded server-side only with cli; kill by pane id |

## Lua

| File | Change |
| --- | --- |
| `vtabs/transport.lua` (new) | per pane `off \| negotiating \| active`, `next_seq`, queue (≤ 64), 2 s timer; `offer(pane, ready)`, `write(pane, text)`, `stop(pane)`, `forget(pane_id)`; `fs = { open, rename, remove }` injectable |
| `vtabs/transport.lua` | `write`: `util.private_open` tmp, `assert(write)`, `assert(close)`, `os.rename`; any failure → remove tmp, `stop`, return false |
| `vtabs/sidebar_identity.lua` `send_raw` | `active` → `transport.write`; `negotiating` → enqueue, return true; else `link.defer` then `send_text` |
| `vtabs/input.lua` | `ready.transport` → `transport.offer`; `ready.pane` → `store.server_pane`; `transport_ready` → switch and flush queue in order; `transport_refused` → flush via stdin; `dropped what=message` → `wire.reset_pane`; `key.delivered` → hand focus over only |
| `vtabs/backend.lua` `env` | `VTABS_INBOX_ROOT` for eligible spawns via `util.runtime_dir()` |
| `vtabs/view.lua`, `vtabs/geometry.lua`, `vtabs/sidebar_attach.lua` | busy-domain gates apply only to panes not `active`; attach waits for a quiet link |
| `vtabs/state.lua` | tokens no longer saved to `wezterm.GLOBAL` or disk; a fresh VM mints fresh tokens |
| `vtabs/sidebar_attach.lua`, `vtabs/settings.lua` | mux close: `quit` every backend in the tab, `kill` by server pane id through a helper, activation only when the server has no backend; `transport.forget` on pane close; retirement keeps `quit` on stdin |
| `vtabs/store.lua` | `server_pane = "pane"` |
| `vtabs/config.lua` schema | `backend.inbox` |
| `tests/fake_mux.lua` | fake `fs`; `ready` with `pane` and `transport`; obey messages in seq order; refuse when probe missing; `kill` by pane; `key` delivered server-side |
| tests | stdin-before-inbox order; queue flush order on ready and on refusal; 2 s timeout; write failure → false, no replay, stop sent; basename validation; local-domain pane never offered; reload mints new tokens and renegotiates; gates bypassed for active panes; `link.lua` holding the barrier; attach deferred on a busy link; mux close without activation |

## End to end

| Check | How |
| --- | --- |
| localmux sidebars run on the inbox | `wezterm-e2e.lua` probe reports `transport == "active"` for every mux tab |
| no link crossing after the switch | GUI log shows no `send_text` for those panes |
| forwarded key on mux | typed at the sidebar, lands in the content pane, `key.delivered` true |
| mux close | settings tab and an orphan sidebar close without `CloseCurrentPane` |
| disabled knob | `backend.inbox = false`, full suite passes |
| ssh mux | `test_ssh_mux.py` asserts no offer to the remote pane |
| reload | plugin edit under the e2e instance: sidebars clear once, come back on the inbox |
| the original trigger | `just dev --mux`, fn+ctrl+arrow tiling rounds, twice, no hang report |

## Order

| Step | Owner | Scope | Gate |
| --- | --- | --- | --- |
| 1 | Rust agent | protocol types, generators, `cli.rs` additions | `cargo test`, `gen-lua --check`, `gen-config --check` |
| 2a | Rust agent | inbox, transport state, key forwarding | Rust unit tests above, clippy, fmt |
| 2b | Lua agent | transport, fake, gates, reload, close paths | Lua suite, luacheck, stylua |
| 3 | lead | docs, e2e probes and tests, e2e twice, manual session | table above |

Estimate: two to three days.

## Review findings and resolution

| Finding | Resolution |
| --- | --- |
| Codex 1: backend path = arbitrary append primitive | root chosen by Lua, basename only, validated both sides |
| Codex 2: no commit point | switch only on `transport_ready` with matching session |
| Codex 3: replay of side-effecting commands | delivered iff renamed; failure returns false, never replays |
| Codex 4: multi-writer append | one file per batch, atomic rename |
| Codex 5: unbounded journal | processed messages deleted; dead-sibling sweep |
| Codex 6: stdin parser as ingress | control-only decoder |
| Codex 7: cleanup and reload | fresh tokens per VM; `forget` hooks; `quit` on stdin |
| Codex: epoch numbers | dropped; session id is the directory |
| Codex: 25-50 ms scan primary | notifications primary, 50 ms scan only without a watcher |
| Third review H1: reload loses transport | fresh token per reload (owner decision) |
| H2: frames lost on stop / abandon | drain on stop, stdin flush on abandon, `wire.reset_pane` on loss |
| H3: negotiation unbounded | 2 s timeout, immediate refusal |
| H4: busy gates defeat the transport | gates only for `send_text` panes |
| H5: 2 MiB file cap below a legal batch | 6 × `LINE_MAX` |
| H6, H7, H8, H9 | dead-sibling sweep; backend creates and validates the root; `assert(close)`; `scan()` never deletes `*.tmp` |
| H10: other mux operations | table above; none park the thread, the blocking one besides frames is the forwarded key, moved server-side; closes moved off activation on mux |
| `transport_lost` | cut; a gap is one `dropped` message |
| `notify` crate | cut; `libc` inotify / kqueue |

## Implemented deviations

| Plan | Implementation | Why |
| --- | --- | --- |
| `quit` alone stays on stdin | `auth` too, with `keys:"server"` only for eligible mux sidebars | a new token tears the inbox down, so auth cannot ride the transport it resets |
| queue flushed via stdin on abandonment | a fresh `ready` drops the old session's queue; timeout, overflow and write failure still flush | the backend reset on that `ready` and `wire.reset_pane` republishes everything |
| failed write: no replay | also `wire.reset_pane` | the wire must forget the batch it believed delivered |
| `dropped {what:"message",seq}` | `reason:"gap"` kept alongside | `reason` is mandatory on every `dropped` today |
| `transport_refused why:"probe"` | also `"session"` and `"state"` | a stale or stateless barrier needs an answer; only `"probe"` withdraws the offer |
| one negotiation per backend | two at startup: the first `ready` is refused with `why:"session"`, the second lands | the backend re-announces on the first token, as before this change |
| `key.delivered:false` | field omitted when false | one less byte per key; Lua tests `== true` |
| settings page reload | one fresh token per VM from `ensure_window`; `settings.open` re-registers | the plan named no owner |

## Post-implementation review

Independent Claude session over the working tree, after two green end-to-end passes.

| Finding | Class | Resolution |
| --- | --- | --- |
| `ready` shape in `docs/protocol.md` lacked `inbox_transport`, `pane`, `transport` | defect | fixed |
| `hover = "press"`: the backend typed keys server-side while Lua kept focus on the sidebar | defect | `identity.keys_mode` decides `auth.keys` the way `forward_key` decides; `input.tick` re-auths with the same token when the mode changes; test added |
| no e2e assertion that an active pane never crosses the link | test gap | `link_crossings` counter in the e2e config; the transport test asserts 0 |
| per-key `wezterm cli` spawn has no rate limit on the server path | discussable | open; bounded by the backend handling keys one at a time |
| a window's publish waits for a sibling still negotiating | discussable | open; transient |
| one inbox session created and dropped per spawn | discussable | open; inherent to the first-token re-announce |
| `emit_key` delivered branch and `cli send-text` / `kill-pane` have no unit test | test gap | open; they spawn `wezterm`, covered by e2e and the manual session |
| owner-uid root check untestable without a second uid | note | open |

## Notes and limitations

- The sidebar is a real pane in each tab. Pane-directional navigation should
  use `vtabs.action.activate_pane_direction` so the sidebar is skipped.
  `TogglePaneZoom` hides it until unzoomed.
- Tabs spawned by bindings other than the plugin's get their sidebar on the
  next poll (`poll_ms`). Use `vtabs.action.new_tab` for an instant one.
- The sidebar process runs in the tab's domain; WezTerm cannot host a local
  pane inside a remote-mux tab. On remote multiplexer domains (ssh/tls mux) the
  plugin runs an inline bootstrap that downloads a release build for the remote
  architecture; until a release exists, build `wez-vtabs` on the remote host and
  point `backend.path[domain]` at it. A domain whose backend never answers gets
  one warning and no further sidebars there for 60 s.
- Only pins and closed-tab history persist, to
  `$XDG_STATE_HOME/wez-vtabs/state.json` (default `~/.local/state`, mode 0600,
  written tmp + rename, symlinks refused). Pane/tab/window ids and tokens never
  reach disk. Pins are keyed by tab id, so they are dropped at startup unless a
  sidebar pane survived in the mux; collapsed, focus and private are per GUI
  process.
- "Move to new window" (drag to the inner edge, menu, or `tear_off`) only works
  for tabs with a single content pane; multi-pane tabs show a notification.
- Private windows unset shell history via environment variables; shells whose
  rc files re-set `HISTFILE` will still write history. Closed private tabs are
  not recorded for reopening. This is not an isolation boundary.
- Only panes that echoed a token this process minted may drive the sidebar; user
  vars from other panes are ignored. Titles are stripped of control characters
  before rendering.
- Every sidebar backend, including one on a remote mux host, receives every
  tab's title and cwd in its frames.
- A sidebar whose backend stops answering pings for 20 s is restarted.

## Sidebar identity

| Evidence | Rank | Grants |
| --- | --- | --- |
| pane echoed a token this process minted for it | 3 | frames, events, close |
| plugin split the pane in this process | 2 | layout only, until the echo |
| pane title `wez-vtabs:<hex>` | 1 | adoption: one `auth`, 5 tries, then back to content |
| anything else | 0 | content |

One pane per tab holds the role: the highest rank wins and every other pane is
content, so a faked title can neither empty a tab nor displace a live sidebar.

A pane that fakes the title marker is sent an `auth` command on its own stdin,
so it can echo the token and become the tab's sidebar. It then receives frames
(every tab's title and cwd in this window) and its events drive tab management
for that window. This is not a security boundary and is not meant to be: any
process running as you already has full mux control through `wezterm cli`, and a
backend spawned in a remote tab's domain is trusted by design.

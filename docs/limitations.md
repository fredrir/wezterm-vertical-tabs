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

| Evidence | Grants | Notes |
| --- | --- | --- |
| pane echoed a token this process minted for it | frames, events, close | only trusted state |
| pane title `wez-vtabs:<hex>` | adoption: re-auth with a fresh token | any process can set a title |
| plugin split the pane this process | layout only, until the echo | |

A pane that fakes the title marker is adopted as a sidebar and then waits for an
echo it cannot produce: it is never sent frames, its events are ignored, its tab
is never closed, and no keystroke is ever forwarded to it. It does cost that tab
its sidebar.
